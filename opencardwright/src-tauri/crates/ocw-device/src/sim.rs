//! 模拟设备：用固定样本回放，让 CI 与前端开发不依赖读卡器。
//!
//! 样本取自 `sdk/` 的既有输出格式（见 `docs/抓包样本与验证记录.md` 的黄金样本），
//! 但这里是**输出契约**的样本，不描述卡内字节规则。

use crate::parse::{self, CardSnapshot, DeviceError, PortProbe, SelfTest};

/// 220 房 1 天日租卡。
pub const GUEST_DAY_JSON: &str = r#"{"ok":true,"card":{"kind":"日租客卡","room":"220","roomIndex":15,"days":1,"hours":0,"cardNo":"0x2404","raw":"5BECF100810424AB0000000000000000000000000000000000000000"}}"#;

/// 220 房 3 小时钟点卡。
pub const GUEST_HOUR_JSON: &str = r#"{"ok":true,"card":{"kind":"钟点客卡","room":"220","roomIndex":15,"days":0,"hours":3,"cardNo":"0x2289","raw":"5BECF100448922AB0000000000000000000000000000000000000000"}}"#;

/// 空白卡：A 区仍要有数据可展示。
pub const BLANK_JSON: &str = r#"{"ok":true,"card":{"kind":"空白卡","room":null,"roomIndex":-1,"days":0,"hours":0,"cardNo":null,"raw":"FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF"}}"#;

/// 管理卡（安装卡）：无卡号。
pub const INSTALL_JSON: &str = r#"{"ok":true,"card":{"kind":"安装卡","room":"220","roomIndex":15,"days":0,"hours":0,"cardNo":null,"raw":"5BECFC0046F200010000000000000000000000000000000000000000"}}"#;

/// 无卡。
pub const NO_CARD_JSON: &str = r#"{"ok":false,"error":{"code":3,"msg":"未检测到卡片或卡型不符"}}"#;

/// 自检全绿。
pub const SELFTEST_OK: &str = "自检结果: 27 通过, 0 失败\n";

/// 自检有失败。
pub const SELFTEST_BAD: &str = "自检结果: 26 通过, 1 失败\n";

/// 两个口，第二个能用。
pub const PORTS_TEXT: &str = "COM6  可打开但异常  读卡器握手失败\nCOM7  可用   读卡器=4E57520492008A7F\n";

#[derive(Default, Clone)]
pub struct SimDevice {
    /// 下一次 `read_card` 返回哪个样本；None 用默认的日租卡。
    pub next_read: Option<&'static str>,
    /// 运行时目录（含 `OCWCardBuild.exe` 与 `CardNo.state`）。
    /// 给了它，`issue_guest` 就会像真机一样**离线构包并推进计数器**；没给则退回固定样本。
    pub runtime: Option<std::path::PathBuf>,
    /// 计数器起点（CardNo.state 缺失时用）
    pub start: u16,
}

impl SimDevice {
    pub fn new() -> Self {
        Self::default()
    }

    /// 绑定运行时目录：模拟发卡会用这里的构包器和计数器。
    pub fn at(runtime: &std::path::Path) -> Self {
        Self {
            runtime: Some(runtime.to_path_buf()),
            start: 0x2403,
            ..Self::default()
        }
    }

    pub fn read_card(&self) -> Result<CardSnapshot, DeviceError> {
        parse::parse_card(self.next_read.unwrap_or(GUEST_DAY_JSON), 0)
    }

    /// 模拟发卡：调用 `OCWCardBuild.exe`（内部用 SDK 的 CardBuilder + CardNoStore）。
    ///
    /// 返回 `None` 表示"没有可用的构包器"，调用方应退回固定样本。
    pub fn issue_guest(
        &self,
        room_index: u8,
        room_no: &str,
        days: u16,
        hours: u16,
    ) -> Option<Result<CardSnapshot, DeviceError>> {
        let dir = self.runtime.as_ref()?;
        let exe = dir.join("OCWCardBuild.exe");
        if !exe.is_file() {
            return None;
        }

        let state = dir.join("CardNo.state");
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("--state")
            .arg(&state)
            .arg("--start")
            .arg(format!("{:04X}", self.start))
            .arg("--index")
            .arg(room_index.to_string());
        if hours > 0 {
            cmd.arg("--hours").arg(hours.to_string());
        } else {
            cmd.arg("--days").arg(days.to_string());
        }
        // 不弹黑框（和 CLI 设备保持一致）
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }

        let out = cmd.output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let raw = json_field(&text, "raw")?;
        let card_no = json_field(&text, "cardNo")?;

        // 复用 CLI 的 JSON 形状，交给现有解析器，避免这里再造一份样本结构
        let kind = if hours > 0 { "钟点客卡" } else { "日租客卡" };
        let json = format!(
            r#"{{"ok":true,"card":{{"kind":"{kind}","room":"{room_no}","roomIndex":{room_index},"days":{days},"hours":{hours},"cardNo":"{card_no}","raw":"{raw}"}}}}"#
        );
        Some(parse::parse_card(&json, 0))
    }

    /// 模拟发**管理卡**（总控 / 复位 / 清除 / 安装 / 退房钟点 / 校时 / raw）。
    ///
    /// 参数原样转发给 `OCWCardBuild.exe`，只做两件事：把 `--room <房号>` 换成
    /// `--index <序号>`（构包器按序号算页1 第三/四字节），以及把输出翻译成统一的
    /// 卡快照 JSON。返回 `None` 表示没有构包器，调用方决定回退还是报错。
    pub fn issue_admin(
        &self,
        args: &[&str],
        room_index: Option<u8>,
    ) -> Option<Result<CardSnapshot, DeviceError>> {
        let dir = self.runtime.as_ref()?;
        let exe = dir.join("OCWCardBuild.exe");
        if !exe.is_file() {
            return None;
        }

        let state = dir.join("CardNo.state");
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("--state")
            .arg(&state)
            .arg("--start")
            .arg(format!("{:04X}", self.start));

        let mut kind_type = String::new();
        let mut days: u16 = 0;
        let mut hours: u16 = 0;
        let mut i = 0;
        while i < args.len() {
            let a = args[i];
            // 构包器不认识房号（卡内只有序号），跳过它的值
            if a == "--room" {
                i += 2;
                continue;
            }
            if a == "--type" && i + 1 < args.len() {
                kind_type = args[i + 1].to_string();
            } else if a == "--days" && i + 1 < args.len() {
                days = args[i + 1].parse().unwrap_or(0);
            } else if a == "--hours" && i + 1 < args.len() {
                hours = args[i + 1].parse().unwrap_or(0);
            }
            cmd.arg(a);
            i += 1;
        }
        if let Some(idx) = room_index {
            cmd.arg("--index").arg(idx.to_string());
        }

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }

        let out = cmd.output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        if !text.contains("\"ok\":true") {
            let msg = json_field(&text, "msg").unwrap_or_else(|| text.to_string());
            return Some(Err(DeviceError::Protocol(format!(
                "模拟构包失败：{msg}"
            ))));
        }

        let raw = match json_field(&text, "raw") {
            Some(v) => v,
            None => return Some(Err(DeviceError::Protocol("构包器输出缺少 raw".into()))),
        };
        // 安装卡 / raw 卡不含卡号：构包器给的是 null，解析器能接受
        let card_no = json_field(&text, "cardNo").unwrap_or_else(|| "null".into());

        // 卡型中文名要和 parse.rs 认的名字一致（退房是「退房钟点设置卡」全称）
        let kind = match kind_type.as_str() {
            "master" => "总控卡",
            "reset" => "复位卡",
            "clear" => "清除卡",
            "install" => "安装卡",
            "checkout" => "退房钟点设置卡",
            "timesync" => "校时卡",
            "raw" => "自定义卡",
            _ => "管理卡",
        };
        let idx = room_index.map(|v| v as i32).unwrap_or(-1);
        let json = format!(
            r#"{{"ok":true,"card":{{"kind":"{kind}","room":null,"roomIndex":{idx},"days":{days},"hours":{hours},"cardNo":{card_no},"raw":"{raw}"}}}}"#
        );
        Some(parse::parse_card(&json, 0))
    }

    pub fn selftest(&self) -> Result<SelfTest, DeviceError> {
        parse::parse_selftest(SELFTEST_OK)
    }

    pub fn probe_ports(&self) -> Result<Vec<PortProbe>, DeviceError> {
        Ok(parse::parse_ports(PORTS_TEXT))
    }
}

/// 从构包器的 JSON 里抠一个字符串/数字字段（输出形状是我们自己定的，够用）。
fn json_field(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    if let Some(start) = text.find(&needle) {
        let rest = &text[start + needle.len()..];
        let end = rest.find('"')?;
        return Some(rest[..end].to_string());
    }
    // 数字字段（roomIndex）
    let needle = format!("\"{key}\":");
    let start = text.find(&needle)? + needle.len();
    let rest = &text[start..];
    let end = rest.find(|c: char| c == ',' || c == '}')?;
    Some(rest[..end].trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::CardKind;

    #[test]
    fn sim_returns_guest_day_by_default() {
        let c = SimDevice::new().read_card().unwrap();
        assert_eq!(c.kind, CardKind::GuestDay);
        assert_eq!(c.card_no, Some(0x2404));
    }

    #[test]
    fn sim_can_replay_blank_card() {
        let mut sim = SimDevice::new();
        sim.next_read = Some(BLANK_JSON);
        let c = sim.read_card().unwrap();
        assert!(c.kind.is_blank());
    }

    #[test]
    fn all_samples_parse() {
        for s in [GUEST_DAY_JSON, GUEST_HOUR_JSON, BLANK_JSON, INSTALL_JSON] {
            parse::parse_card(s, 0).expect("样本必须能解析");
        }
        assert!(parse::parse_card(NO_CARD_JSON, 3).is_err());
    }

    /// 样本是契约：28 字节 = 56 个十六进制字符。改样本时这条会立刻抓住手滑。
    #[test]
    fn every_sample_raw_is_28_bytes() {
        for s in [GUEST_DAY_JSON, GUEST_HOUR_JSON, BLANK_JSON, INSTALL_JSON] {
            let card = parse::parse_card(s, 0).unwrap();
            assert!(
                card.raw_is_28_bytes(),
                "{} 的原始数据长度应为 56，实际 {}",
                card.kind_label,
                card.raw_hex.len()
            );
        }
    }
}
