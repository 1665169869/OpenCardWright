//! 发卡参数的业务边界。
//!
//! 卡内天数编码分两段（见 `docs/TEMIC门锁卡技术规格.md` §4.2）：
//!
//! * `0x80 | 天数` —— 简单段。客卡 1–62，总控卡 1–63。
//! * `0xC0 | 月数` —— 长有效期，`月数 = round(天数 / 31)`，原软件只给固定档位。
//!
//! 为什么客卡是 62、总控是 63：客卡的小时编码是 `0x40|(小时+1)`，占掉了 63 这一格，
//! 天/小时两套上限在原软件里保持一致的分界。这里照抄，不要"顺手放宽"。

/// 超过简单段后允许的固定档位。原软件界面只提供这些值，超范围的天数在界面上根本不存在。
pub const LONG_DAY_TIERS: &[u16] = &[
    90, 120, 150, 180, 210, 240, 300, 360, 390, 420, 480, 540, 600, 720, 900, 1095,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardTier {
    /// 客卡（日租 / 钟点）
    Guest,
    /// 总控卡
    Master,
}

impl CardTier {
    /// 简单段（`0x80|天数`）能表示的最大天数。
    pub fn max_simple_days(self) -> u16 {
        match self {
            Self::Guest => 62,
            Self::Master => 63,
        }
    }

    /// 小时编码能表示的最大小时数。
    pub fn max_hours(self) -> u16 {
        match self {
            Self::Guest => 62,
            Self::Master => 63,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Guest => "客卡",
            Self::Master => "总控卡",
        }
    }
}

/// 长有效期卡（月码段）发卡前必须输入的确认词。
///
/// 为什么要有这道门：卡刷一次门就会把该房间的"当前有效卡号"顶到三年后，
/// 之后发的正常卡号比它小，会**被锁当成旧卡拒开**，只能给全部门锁刷复位卡才能恢复。
pub const LONG_CONFIRM_WORD: &str = "顶高水位";

/// 是否属于"会顶高水位"的长有效期卡（天数进入 `0xC0` 月码段）。
pub fn needs_long_confirm(tier: CardTier, days: u16) -> bool {
    days > tier.max_simple_days()
}

/// 校验确认词。不需要确认的档位直接通过；需要时**必须**一字不差。
pub fn check_long_confirm(tier: CardTier, days: u16, word: Option<&str>) -> Result<(), String> {
    if !needs_long_confirm(tier, days) {
        return Ok(());
    }
    if word.map(|w| w.trim()) == Some(LONG_CONFIRM_WORD) {
        return Ok(());
    }
    Err(format!(
        "{days} 天属于长有效期卡：刷一次门会把该房间的有效卡号顶到 {} 年后，之后正常卡全部失效，         只能用复位卡恢复。确认请原样输入「{LONG_CONFIRM_WORD}」",
        (days as f32 / 365.0).ceil() as u16
    ))
}

fn tiers_text() -> String {
    LONG_DAY_TIERS
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join(" / ")
}

/// 校验天数。`0` 非法；简单段内任意值；超过后必须是固定档位。
pub fn validate_days(tier: CardTier, days: u16) -> Result<(), String> {
    if days == 0 {
        return Err("天数必须 ≥ 1".to_string());
    }
    if days <= tier.max_simple_days() {
        return Ok(());
    }
    if LONG_DAY_TIERS.contains(&days) {
        return Ok(());
    }
    Err(format!(
        "{}最多 {} 天；更长的有效期只能从固定档位选择：{}",
        tier.label(),
        tier.max_simple_days(),
        tiers_text()
    ))
}

/// 校验小时数。客卡 1–62，总控卡 1–63。
pub fn validate_hours(tier: CardTier, hours: u16) -> Result<(), String> {
    if hours == 0 {
        return Err("小时数必须 ≥ 1".to_string());
    }
    if hours > tier.max_hours() {
        return Err(format!(
            "{}最多 {} 小时（0x40 段上限）",
            tier.label(),
            tier.max_hours()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guest_days_boundary_is_62() {
        assert!(validate_days(CardTier::Guest, 1).is_ok());
        assert!(validate_days(CardTier::Guest, 62).is_ok());
        assert!(validate_days(CardTier::Guest, 63).is_err(), "客卡 63 天必须拒绝");
        assert!(validate_days(CardTier::Guest, 0).is_err());
    }

    #[test]
    fn master_days_boundary_is_63() {
        assert!(validate_days(CardTier::Master, 63).is_ok());
        assert!(validate_days(CardTier::Master, 64).is_err());
    }

    #[test]
    fn long_durations_must_be_fixed_tiers() {
        for d in [90, 120, 150, 180, 210, 240, 360, 390, 1095] {
            assert!(validate_days(CardTier::Guest, d).is_ok(), "{d} 应是合法档位");
            assert!(validate_days(CardTier::Master, d).is_ok(), "{d} 应是合法档位");
        }
        assert!(validate_days(CardTier::Guest, 64).is_err());
        assert!(validate_days(CardTier::Guest, 100).is_err());
        assert!(validate_days(CardTier::Master, 1000).is_err());
    }

    #[test]
    fn hour_limits_differ_by_one() {
        assert!(validate_hours(CardTier::Guest, 62).is_ok());
        assert!(validate_hours(CardTier::Guest, 63).is_err());
        assert!(validate_hours(CardTier::Master, 63).is_ok());
        assert!(validate_hours(CardTier::Master, 64).is_err());
    }

    #[test]
    fn error_message_lists_tiers() {
        let e = validate_days(CardTier::Guest, 64).unwrap_err();
        assert!(e.contains("62"), "应说明客卡上限");
        assert!(e.contains("90"), "应列出固定档位");
    }
    #[test]
    fn long_confirm_required_only_beyond_simple_tier() {
        assert!(!needs_long_confirm(CardTier::Guest, 62));
        assert!(needs_long_confirm(CardTier::Guest, 90));
        assert!(!needs_long_confirm(CardTier::Master, 63));
        assert!(needs_long_confirm(CardTier::Master, 90));

        assert!(check_long_confirm(CardTier::Guest, 62, None).is_ok());
        assert!(check_long_confirm(CardTier::Guest, 90, None).is_err());
        assert!(check_long_confirm(CardTier::Guest, 90, Some("顶高水位")).is_ok());
        assert!(check_long_confirm(CardTier::Guest, 90, Some(" 顶高水位 ")).is_ok(), "允许首尾空格");
        assert!(check_long_confirm(CardTier::Guest, 90, Some("顶高")).is_err());
        assert!(check_long_confirm(CardTier::Guest, 1095, Some("顶高水位")).is_ok());
    }

    #[test]
    fn long_confirm_message_explains_recovery() {
        let msg = check_long_confirm(CardTier::Guest, 1095, None).unwrap_err();
        assert!(msg.contains("复位卡"), "必须写清恢复手段：{msg}");
        assert!(msg.contains("顶高水位"), "必须给出确认词：{msg}");
        assert!(msg.contains("3 年"), "要写清顶到几年：{msg}");
    }

}
