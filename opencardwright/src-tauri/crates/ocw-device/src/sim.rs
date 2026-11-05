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

#[derive(Default)]
pub struct SimDevice {
    /// 下一次 `read_card` 返回哪个样本；None 用默认的日租卡。
    pub next_read: Option<&'static str>,
}

impl SimDevice {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read_card(&self) -> Result<CardSnapshot, DeviceError> {
        parse::parse_card(self.next_read.unwrap_or(GUEST_DAY_JSON), 0)
    }

    pub fn selftest(&self) -> Result<SelfTest, DeviceError> {
        parse::parse_selftest(SELFTEST_OK)
    }

    pub fn probe_ports(&self) -> Result<Vec<PortProbe>, DeviceError> {
        Ok(parse::parse_ports(PORTS_TEXT))
    }
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
