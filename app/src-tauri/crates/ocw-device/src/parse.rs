//! 解析 `PMSLockCard.exe` 的输出。
//!
//! 三种输出形态：
//! * `read` / `issue`：单行 JSON（`--json`）
//! * `selftest`：多行文本，末行 `自检结果: N 通过, M 失败`
//! * `ports`：多行文本，每行 `COM<n>  可用   读卡器=<id>`
//!
//! 原则：**解析不出来就是错误，绝不"猜测成功"**。半截输出、空输出、被别的东西
//! 写进 stdout 的日志，都必须报 `Protocol` 而不是当成一张卡。

use serde::{Deserialize, Serialize};

/// CLI 未能给出退出码时使用（进程被信号终止、或根本没能启动）。
pub const EXIT_CODE_UNKNOWN: i32 = -1;

/// 卡型。取值与 SDK 的 `CardKind` 一一对应，识别的是 CLI 输出的中文卡型名。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CardKind {
    GuestDay,
    GuestHour,
    Master,
    Reset,
    Clear,
    Install,
    CheckoutHour,
    TimeSync,
    Blank,
    Unknown,
}

impl CardKind {
    pub fn from_label(label: &str) -> Self {
        match label.trim() {
            "日租客卡" => Self::GuestDay,
            "钟点客卡" => Self::GuestHour,
            "总控卡" => Self::Master,
            "复位卡" => Self::Reset,
            "清除卡" => Self::Clear,
            "安装卡" => Self::Install,
            "退房钟点设置卡" => Self::CheckoutHour,
            "校时卡" => Self::TimeSync,
            "空白卡" => Self::Blank,
            _ => Self::Unknown,
        }
    }

    /// 是否属于"管理卡"——前端据此把它画成危险色、要求 admin 权限。
    pub fn is_admin_card(self) -> bool {
        matches!(
            self,
            Self::Master | Self::Reset | Self::Clear | Self::Install | Self::CheckoutHour | Self::TimeSync
        )
    }

    /// 空白卡没有卡号，读卡页的"记录区"要显示成"无卡号"而不是"未查到"。
    pub fn is_blank(self) -> bool {
        matches!(self, Self::Blank)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardSnapshot {
    pub kind: CardKind,
    pub kind_label: String,
    /// 房号。CLI 用 RoomIndex.ini 反查；查不到时为 None（此时只能看序号）。
    pub room: Option<String>,
    /// 房间序号，-1 表示不适用（总控/复位等）。
    pub room_index: i32,
    pub days: i32,
    pub hours: i32,
    pub card_no: Option<u16>,
    /// 挂失卡（页1[2] 低 4 位 = 1；TD-150 实测）
    pub is_lost: bool,
    /// 发卡日 / 小时 / 小时内序号：从卡号解码（卡号 = 日5 + 时5 + 序号6）
    pub issue_day: i32,
    pub issue_hour: i32,
    pub issue_seq: i32,
    /// 卡内「年 + 月」字节（`页2[3]`）：高 4 位 = `(年−2000)&0x0F`、低 4 位 = 月。
    /// 只存原始字节，年份的还原（16 年循环）由前端按当前年做。
    pub card_ym: u8,
    /// 28 字节的十六进制原文（56 个字符）。
    pub raw_hex: String,
}

impl CardSnapshot {
    /// 读卡器返回的原始数据是否形如 28 字节。不合规时前端会给出"数据异常"提示。
    pub fn raw_is_28_bytes(&self) -> bool {
        self.raw_hex.len() == 56 && self.raw_hex.chars().all(|c| c.is_ascii_hexdigit())
    }

    /// 纠正总控卡的识别。
    ///
    /// SDK 只把页1 = `5B EC F0 FF` 认作总控卡；真机上见过 **`5B EC F1 FF`** 的总控卡
    /// （低 4 位为 1，和钟点卡低4位 0/1 的未解规律同源）。这种卡会被 SDK 解析成
    /// "钟点客卡 序号 4095"。
    ///
    /// 判据是可靠的：房间序号最大 255，`B3` 不可能等于 `0xFF`，只有总控卡会把 B3 置 FF。
    /// 时长编码也跟着换：总控卡小时是 `0x40|小时`，客卡才是 `0x40|(小时+1)`。
    pub fn fix_master_marker(mut self) -> Self {
        if self.raw_hex.len() != 56 {
            return self;
        }
        let byte = |i: usize| u8::from_str_radix(&self.raw_hex[i * 2..i * 2 + 2], 16).ok();
        let (Some(_b2), Some(b3), Some(d0)) = (byte(2), byte(3), byte(4)) else {
            return self;
        };

        // 只有被 SDK 误判成客卡的才需要纠正卡型；空白卡（全 FF）的 B3 也是 FF，不能碰。
        if matches!(self.kind, CardKind::GuestDay | CardKind::GuestHour) {
            if b3 != 0xFF {
                return self;
            }
            self.kind = CardKind::Master;
            self.kind_label = "总控卡".to_string();
            self.room = None;
            self.room_index = -1;
        } else if self.kind != CardKind::Master {
            // 复位/清除/安装/空白等其它卡型不动。
            return self;
        }

        // SDK 的读侧不解码总控卡时长码（实测 0x81 总控卡 → days=0, hours=0），
        // 只要最终是总控卡，就按总控编码统一补算时长。
        match d0 & 0xC0 {
            0x40 => {
                self.hours = (d0 & 0x3F) as i32;
                self.days = 0;
            }
            0x80 => {
                self.days = (d0 & 0x3F) as i32;
                self.hours = 0;
            }
            0xC0 => {
                // 月码：1 月 ≈ 31 天，只用于展示
                self.days = (d0 & 0x3F) as i32 * 31;
                self.hours = 0;
            }
            _ => {}
        }

        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelfTest {
    pub passed: i32,
    pub failed: i32,
}

impl SelfTest {
    pub fn is_ok(&self) -> bool {
        self.failed == 0 && self.passed > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortProbe {
    /// 串口索引，0 = COM1（与 CLI 的 `--port` 同一套编号）。
    pub port: u8,
    pub com: String,
    pub available: bool,
    pub reader_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
    #[error("设备返回错误 {code}: {message}")]
    Cli { code: i32, message: String },

    #[error("无法启动设备进程: {0}")]
    Spawn(String),

    #[error("设备操作超时（{0} 秒）")]
    Timeout(u64),

    #[error("设备输出无法解析: {0}")]
    Protocol(String),

    #[error("IO 错误: {0}")]
    Io(String),
}

impl DeviceError {
    /// 透传给前端的错误码，统一为 **SDK 语义**（0–11）。
    ///
    /// CLI 的**进程退出码**是另一套（0–7），只有 JSON 里没有 `error.code` 时才需要换算。
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Cli { code, .. } => *code,
            _ => EXIT_CODE_UNKNOWN,
        }
    }

    /// 给人看的处理建议。前端直接展示这段文本。
    ///
    /// 按 **SDK 码**（`LockResult`）匹配，不能按 CLI 退出码 —— 两者在 2/3 上错位：
    /// SDK 3 = 串口打开失败，CLI 3 = 无卡。
    pub fn hint(&self) -> &'static str {
        match self {
            Self::Cli { code, .. } => match *code {
                1 => "参数不合法。检查房号、天数/小时是否在允许范围内。",
                2 => "驱动 DLL 加载失败。检查运行时目录里的 LC32RFRW.dll。",
                3 => "串口打开失败。多半是原门锁软件占着串口，先关掉它（NacreHis.exe）。",
                4 => "读卡器握手失败。检查 USB 连接与供电。",
                5 => "没有检测到卡片，或卡不是 125 kHz TEMIC 卡。",
                6 => "读卡失败。卡可能没放稳，或读卡器异常。",
                7 => "写卡失败。检查卡是否被其它系统加过密，或换一张卡。",
                8 => "写后回读不一致。卡可能没放稳，或卡已损坏。",
                9 => "卡号无效（越界或小于当前水位）。",
                10 => "该卡型当前不支持。",
                11 => "卡号状态文件读写失败。检查运行时目录权限。",
                _ => "请查看诊断日志。",
            },
            Self::Spawn(_) => "找不到 PMSLockCard.exe，或它被安全软件拦截。检查运行时目录。",
            Self::Timeout(_) => "设备无响应。请检查串口线，然后重启程序。",
            Self::Protocol(_) => "读卡器返回了无法识别的内容。请导出诊断包反馈。",
            Self::Io(_) => "与设备进程通信失败。",
        }
    }
}

// ---------------------------------------------------------------- JSON 形态

#[derive(Debug, Deserialize)]
struct Envelope {
    ok: bool,
    #[serde(default)]
    card: Option<RawCard>,
    #[serde(default)]
    error: Option<RawError>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawCard {
    kind: String,
    #[serde(default)]
    room: Option<String>,
    #[serde(default)]
    room_index: i32,
    #[serde(default)]
    days: i32,
    #[serde(default)]
    hours: i32,
    #[serde(default)]
    card_no: Option<String>,
    #[serde(default)]
    raw: String,
}

#[derive(Debug, Deserialize)]
struct RawError {
    #[serde(default)]
    code: i32,
    #[serde(default)]
    msg: String,
}

/// 解析一次 `read` / `issue` 的输出。
///
/// `exit_code` 只在 JSON 解析失败时用作兜底：CLI 有时会在写 JSON 之前就退出。
pub fn parse_card(stdout: &str, exit_code: i32) -> Result<CardSnapshot, DeviceError> {
    let text = stdout.trim();
    if text.is_empty() {
        return Err(fallback(exit_code, "设备没有输出任何内容"));
    }

    // 容忍 stdout 前面混入其它行（驱动偶尔会打印调试信息），但 JSON 必须能完整解析。
    let start = match text.find('{') {
        Some(i) => i,
        None => return Err(fallback(exit_code, "输出里没有 JSON")),
    };

    let envelope: Envelope = match serde_json::from_str(&text[start..]) {
        Ok(v) => v,
        Err(e) => return Err(fallback(exit_code, &format!("JSON 解析失败: {e}"))),
    };

    if !envelope.ok {
        let (code, message) = match envelope.error {
            Some(e) => (
                if e.code != 0 { e.code } else { exit_code },
                if e.msg.is_empty() { "未知错误".to_string() } else { e.msg },
            ),
            None => (exit_code, "设备返回失败但没有错误详情".to_string()),
        };
        return Err(DeviceError::Cli { code, message });
    }

    let raw = envelope
        .card
        .ok_or_else(|| DeviceError::Protocol("返回 ok 但没有 card 字段".into()))?;

    let kind = CardKind::from_label(&raw.kind);
    let card_no = raw.card_no.as_deref().and_then(parse_card_no);
    let raw_hex = raw.raw.trim().to_uppercase();
    // 页1[2] 的低 4 位 = 挂失标志（TD-150 实测）；raw_hex 每字节两个 hex 字符
    let is_lost = raw_hex.len() >= 6
        && u8::from_str_radix(&raw_hex[4..6], 16)
            .map(|b| b & 0x0F != 0)
            .unwrap_or(false);
    // 卡号 = 日(5) + 时(5) + 小时内序号(6)，解出发卡时刻
    let (issue_day, issue_hour, issue_seq) = match card_no {
        Some(n) => (
            ((n >> 11) & 0x1F) as i32,
            ((n >> 6) & 0x1F) as i32,
            (n & 0x3F) as i32,
        ),
        None => (-1, -1, -1),
    };
    // 页2[3] = 年(高4位)+月(低4位)；raw_hex 第 8 个字节 = 字符 14..16
    let card_ym = if raw_hex.len() >= 16 {
        u8::from_str_radix(&raw_hex[14..16], 16).unwrap_or(0)
    } else {
        0
    };
    Ok(CardSnapshot {
        kind,
        kind_label: raw.kind.trim().to_string(),
        room: raw.room.filter(|r| !r.trim().is_empty()),
        room_index: raw.room_index,
        days: raw.days,
        hours: raw.hours,
        card_no,
        card_ym,
        is_lost,
        issue_day,
        issue_hour,
        issue_seq,
        raw_hex,
    }
    .fix_master_marker())
}

fn fallback(exit_code: i32, detail: &str) -> DeviceError {
    if exit_code == 0 {
        DeviceError::Protocol(detail.to_string())
    } else {
        // 没有 JSON 时只有 CLI 退出码可用，统一换算成 SDK 码，避免两套码混在同一个字段里
        DeviceError::Cli {
            code: sdk_code_from_exit(exit_code),
            message: format!("{}（{}）", exit_message(exit_code), detail),
        }
    }
}

/// CLI 退出码（0–7）→ SDK 码（0–11）。
///
/// 与 `PMSLockCard.cs` 的 `Fail()` 是反向映射，两边必须一起改。
pub fn sdk_code_from_exit(exit_code: i32) -> i32 {
    match exit_code {
        1 => 1,  // 参数
        2 => 3,  // 驱动/串口/握手 → 归到"串口打开失败"（现场最常见）
        3 => 5,  // 无卡 / 读卡失败
        4 => 7,  // 写卡失败
        5 => 8,  // 回读校验
        6 => 9,  // 卡号无效
        7 => 10, // 不支持
        _ => EXIT_CODE_UNKNOWN,
    }
}

/// CLI 退出码 → 文案。与 `PMSLockCard.cs` 的 `Fail()` 映射一致。
pub fn exit_message(code: i32) -> &'static str {
    match code {
        0 => "成功",
        1 => "参数错误",
        2 => "驱动或串口失败",
        3 => "无卡或读卡失败",
        4 => "写卡失败",
        5 => "写后回读校验失败",
        6 => "卡号无效",
        7 => "不支持的卡型",
        _ => "未知设备错误",
    }
}

/// `"0x2404"` / `"2404"` → 0x2404。越界或非十六进制返回 None。
pub fn parse_card_no(text: &str) -> Option<u16> {
    let t = text.trim();
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    if t.is_empty() {
        return None;
    }
    u32::from_str_radix(t, 16).ok().filter(|v| *v <= 0xFFFF).map(|v| v as u16)
}

// ---------------------------------------------------------------- 文本形态

/// 解析 `selftest` 输出。找不到汇总行即视为失败——**不允许把"没看懂"当成"通过"**。
pub fn parse_selftest(stdout: &str) -> Result<SelfTest, DeviceError> {
    for line in stdout.lines() {
        if !line.contains("自检结果") {
            continue;
        }
        let passed = number_before(line, "通过");
        let failed = number_before(line, "失败");
        if let (Some(p), Some(f)) = (passed, failed) {
            return Ok(SelfTest { passed: p, failed: f });
        }
    }
    Err(DeviceError::Protocol(
        "selftest 输出里找不到 `自检结果: N 通过, M 失败`".into(),
    ))
}

/// 取 `marker` 前面紧邻的十进制整数：`"自检结果: 27 通过, 0 失败"` + `"通过"` → 27。
fn number_before(hay: &str, marker: &str) -> Option<i32> {
    let idx = hay.find(marker)?;
    let head = hay[..idx].as_bytes();

    let mut end = head.len();
    while end > 0 && (head[end - 1] == b' ' || head[end - 1] == b'\t') {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && head[start - 1].is_ascii_digit() {
        start -= 1;
    }
    if start == end {
        return None;
    }
    hay[start..end].parse().ok()
}

/// 解析 `ports` 输出。没有匹配行时返回空表（而不是错误），由调用方决定怎么提示。
pub fn parse_ports(stdout: &str) -> Vec<PortProbe> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let t = line.trim();
        if !t.starts_with("COM") {
            continue;
        }
        let com = match t.split_whitespace().next() {
            Some(c) => c,
            None => continue,
        };
        let number: u32 = match com[3..].parse() {
            Ok(n) if n >= 1 => n,
            _ => continue,
        };
        if number > 256 {
            continue;
        }
        let available = t.contains("可用");
        let reader_id = t
            .split("读卡器=")
            .nth(1)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s != "(未知)");

        out.push(PortProbe {
            port: (number - 1) as u8,
            com: com.to_string(),
            available,
            reader_id,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const GUEST_DAY: &str = r#"{"ok":true,"card":{"kind":"日租客卡","room":"220","roomIndex":15,"days":1,"hours":0,"cardNo":"0x2404","raw":"5BECF100810424AB0000000000000000000000000000000000000000"}}"#;
    const BLANK: &str = r#"{"ok":true,"card":{"kind":"空白卡","room":null,"roomIndex":-1,"days":0,"hours":0,"cardNo":null,"raw":"FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF"}}"#;
    const NO_CARD: &str = r#"{"ok":false,"error":{"code":3,"msg":"未检测到卡片或卡型不符"}}"#;

    #[test]
    fn parses_guest_day() {
        let c = parse_card(GUEST_DAY, 0).unwrap();
        assert_eq!(c.kind, CardKind::GuestDay);
        assert_eq!(c.room.as_deref(), Some("220"));
        assert_eq!(c.room_index, 15);
        assert_eq!(c.days, 1);
        assert_eq!(c.card_no, Some(0x2404));
        assert!(c.raw_is_28_bytes());
        assert!(!c.kind.is_admin_card());
    }

    #[test]
    fn parses_blank_card_without_card_no() {
        let c = parse_card(BLANK, 0).unwrap();
        assert_eq!(c.kind, CardKind::Blank);
        assert_eq!(c.card_no, None);
        assert!(c.kind.is_blank());
    }

    #[test]
    fn cli_error_keeps_code_and_hint() {
        let e = parse_card(NO_CARD, 3).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(matches!(e, DeviceError::Cli { code: 3, .. }));
        assert!(e.to_string().contains("未检测到卡片"));
    }

    #[test]
    fn empty_output_is_protocol_error_not_success() {
        let e = parse_card("   \n ", 0).unwrap_err();
        assert!(matches!(e, DeviceError::Protocol(_)));
    }

    #[test]
    fn garbage_with_nonzero_exit_maps_to_cli_error() {
        let e = parse_card("some driver noise", 2).unwrap_err();
        // CLI 退出码 2（驱动/串口）换算成 SDK 码 3（串口打开失败）
        assert_eq!(e.exit_code(), 3);
        assert!(e.to_string().contains("驱动或串口失败"));
    }

    #[test]
    fn sdk_code_hint_is_not_confused_with_exit_code() {
        // 真机实测：无读卡器时 CLI 输出 {"code":3,...} 且进程退出码 2。
        // 3 在 SDK 语义里是"串口打开失败"，绝不能显示成"没读到卡"。
        let e = parse_card(NO_CARD, 2).unwrap_err();
        assert_eq!(e.exit_code(), 3);
        assert!(
            e.hint().contains("串口"),
            "SDK 码 3 应提示串口问题，实际: {}",
            e.hint()
        );
    }

    #[test]
    fn exit_code_mapping_is_total() {        assert_eq!(sdk_code_from_exit(1), 1);
        assert_eq!(sdk_code_from_exit(2), 3);
        assert_eq!(sdk_code_from_exit(3), 5);
        assert_eq!(sdk_code_from_exit(4), 7);
        assert_eq!(sdk_code_from_exit(5), 8);
        assert_eq!(sdk_code_from_exit(6), 9);
        assert_eq!(sdk_code_from_exit(7), 10);
    }

    #[test]
    fn json_prefixed_by_noise_still_parses() {
        let text = format!("driver debug: hello\n{GUEST_DAY}");
        let c = parse_card(&text, 0).unwrap();
        assert_eq!(c.card_no, Some(0x2404));
    }

    #[test]
    fn card_no_parsing_edges() {
        assert_eq!(parse_card_no("0x0000"), Some(0));
        assert_eq!(parse_card_no("FFFF"), Some(0xFFFF));
        assert_eq!(parse_card_no("0x10000"), None);
        assert_eq!(parse_card_no(""), None);
        assert_eq!(parse_card_no("zz"), None);
    }

    #[test]
    fn selftest_pass() {
        let text = "  ✓ 220/5天      5BECF100 / 858822AB\n\n自检结果: 27 通过, 0 失败\n";
        let st = parse_selftest(text).unwrap();
        assert_eq!(st.passed, 27);
        assert_eq!(st.failed, 0);
        assert!(st.is_ok());
    }

    #[test]
    fn selftest_fail_is_not_ok() {
        let st = parse_selftest("自检结果: 26 通过, 1 失败").unwrap();
        assert!(!st.is_ok());
    }

    #[test]
    fn selftest_missing_summary_is_error() {
        let e = parse_selftest("全部正常").unwrap_err();
        assert!(matches!(e, DeviceError::Protocol(_)));
    }

    #[test]
    fn ports_parsing() {
        let text = "COM6  可打开但异常  读卡器握手失败\nCOM7  可用   读卡器=4E57520492008A7F\n";
        let p = parse_ports(text);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].port, 5);
        assert!(!p[0].available);
        assert_eq!(p[1].port, 6);
        assert!(p[1].available);
        assert_eq!(p[1].reader_id.as_deref(), Some("4E57520492008A7F"));
    }

    #[test]
    fn ports_no_match_is_empty_not_error() {
        assert!(parse_ports("未找到可用读卡器").is_empty());
    }

    /// 真机样本：总控卡的页1 是 `5BECF1FF`（低 4 位为 1），SDK 只认 `F0FF`，
    /// 会解析成"钟点客卡 序号 4095"。产品侧必须把它纠正回总控卡。
    #[test]
    fn real_master_card_with_f1ff_page_is_recognised() {
        let raw = format!("5BECF1FF41CF2BAB{}", "0".repeat(40));
        let json = format!(
            r#"{{"ok":true,"card":{{"kind":"钟点客卡","room":null,"roomIndex":4095,"days":0,"hours":0,"cardNo":"0x2BCF","raw":"{raw}"}}}}"#
        );

        let c = parse_card(&json, 0).unwrap();
        assert_eq!(c.kind, CardKind::Master, "B3=0xFF 必须判为总控卡");
        assert_eq!(c.kind_label, "总控卡");
        assert_eq!(c.room_index, -1, "总控卡没有房间序号");
        assert!(c.room.is_none());
        assert_eq!(c.hours, 1, "总控卡 0x41 = 1 小时（客卡才 +1）");
        assert_eq!(c.days, 0);
        assert_eq!(c.card_no, Some(0x2BCF));
    }

    #[test]
    fn master_day_code_is_decoded_without_plus_one() {
        // 页2 0x81 → 总控 1 天
        let raw = format!("5BECF0FF81C72BAB{}", "0".repeat(40));
        let json = format!(
            r#"{{"ok":true,"card":{{"kind":"日租客卡","room":null,"roomIndex":4095,"days":0,"hours":0,"cardNo":"0x2BC7","raw":"{raw}"}}}}"#
        );
        let c = parse_card(&json, 0).unwrap();
        assert_eq!(c.kind, CardKind::Master);
        assert_eq!(c.days, 1);
        assert_eq!(c.hours, 0);
    }

    #[test]
    fn ordinary_room_card_is_not_touched_by_master_marker() {
        let c = parse_card(GUEST_DAY, 0).unwrap();
        assert_eq!(c.kind, CardKind::GuestDay, "B3=0x00 的普通房卡不能被改写");
        assert_eq!(c.room_index, 15);
        assert_eq!(c.days, 1);
    }

    /// 真机样本（2026-11-06）：SDK 正确识别的总控卡（页1 `F0FF`）**不经过**纠错分支，
    /// 而 SDK 读侧对总控时长码不解码（0x81 → days=0）。只要最终是总控卡，
    /// 就必须补算时长，否则读卡页「有效期」显示「—」。
    #[test]
    fn sdk_recognised_master_card_gets_duration_decoded() {
        // 页2 0x81 → 总控 1 天（SDK 原样返回 days=0, hours=0）
        let raw = format!("5BECF0FF811024AB{}", "0".repeat(40));
        let json = format!(
            r#"{{"ok":true,"card":{{"kind":"总控卡","room":null,"roomIndex":-1,"days":0,"hours":0,"cardNo":"0x2410","raw":"{raw}"}}}}"#
        );
        let c = parse_card(&json, 0).unwrap();
        assert_eq!(c.kind, CardKind::Master);
        assert_eq!(c.days, 1, "SDK 识别出的总控卡也要解码天数");
        assert_eq!(c.hours, 0);
    }

    #[test]
    fn sdk_recognised_master_card_hour_and_month_codes() {
        // 页2 0x42 → 总控 2 小时（客卡 0x42 是 1 小时，总控不加 1）
        let raw_h = format!("5BECF0FF42C72BAB{}", "0".repeat(40));
        let json_h = format!(
            r#"{{"ok":true,"card":{{"kind":"总控卡","room":null,"roomIndex":-1,"days":0,"hours":0,"cardNo":"0x2BC7","raw":"{raw_h}"}}}}"#
        );
        let c = parse_card(&json_h, 0).unwrap();
        assert_eq!(c.hours, 2, "总控小时码不加 1");
        assert_eq!(c.days, 0);

        // 页2 0xC3 → 月码 3 → 约 93 天（只用于展示）
        let raw_m = format!("5BECF0FFC3C82BAB{}", "0".repeat(40));
        let json_m = format!(
            r#"{{"ok":true,"card":{{"kind":"总控卡","room":null,"roomIndex":-1,"days":0,"hours":0,"cardNo":"0x2BC8","raw":"{raw_m}"}}}}"#
        );
        let c = parse_card(&json_m, 0).unwrap();
        assert_eq!(c.days, 93, "月码 ×31");
        assert_eq!(c.hours, 0);
    }

    /// 复位卡 / 空白卡等非总控卡型不能被时长补算误伤。
    #[test]
    fn non_master_kinds_keep_zero_duration() {
        // 复位卡：页2 首字节 0x00
        let raw = format!("5BEC0A00004124AB{}", "0".repeat(40));
        let json = format!(
            r#"{{"ok":true,"card":{{"kind":"复位卡","room":null,"roomIndex":-1,"days":0,"hours":0,"cardNo":null,"raw":"{raw}"}}}}"#
        );
        let c = parse_card(&json, 0).unwrap();
        assert_eq!(c.days, 0);
        assert_eq!(c.hours, 0);
    }
}
