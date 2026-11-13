//! 硬件层快照 → 界面 DTO。
//!
//! # 为什么还要一层映射
//!
//! `rc_device::CardSnapshot` 是**硬件契约**：它只描述卡里有什么（卡型、12 位序号、
//! 时长、卡号、原始字节），不含房号文本，字段也不为界面排版服务。
//! 前端要的是「天数 / 小时数分开、发卡日时分拆开、中文卡型名、房号」这些展示口径。
//!
//! 把映射集中在 `to_dto` 里，前端契约就只依赖本文件：硬件层升级字段时，
//! 需要改的地方只有这一个函数。
//!
//! # 房号不在这里补
//!
//! `room` 字段恒为 `None`。房号文本的真值在业务库（`room.room_no`），
//! 只能由命令层拿 `room_index` 回查——硬件层不认房号，本层也就不该替他猜。

use serde::{Deserialize, Serialize};

use rc_device::{
    CardDuration, CardKind as HwCardKind, CardSnapshot as HwSnapshot, PortProbe as HwPortProbe,
    SelfTestReport as HwSelfTest,
};

/// 卡型。取值与 `rc_device::CardKind` 一一对应，前端 `api.ts` 直接照抄这些名字。
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
    pub fn from_hw(kind: HwCardKind) -> Self {
        match kind {
            HwCardKind::GuestDay => Self::GuestDay,
            HwCardKind::GuestHour => Self::GuestHour,
            HwCardKind::Master => Self::Master,
            HwCardKind::Reset => Self::Reset,
            HwCardKind::Clear => Self::Clear,
            HwCardKind::Install => Self::Install,
            HwCardKind::CheckoutHour => Self::CheckoutHour,
            HwCardKind::TimeSync => Self::TimeSync,
            HwCardKind::Blank => Self::Blank,
            HwCardKind::Unknown => Self::Unknown,
        }
    }

    /// 从中文卡型名反查枚举。
    ///
    /// 发卡记录（`issuance.kind`）只存中文名，回放历史记录时用它还原枚举。
    /// 认不出来的一律 `Unknown`——**不允许猜测**成客卡。
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

    /// 是否属于「管理卡」——前端据此画危险色、要求 admin 权限。
    ///
    /// **这只是卡型分类，不是权限判断**；权限由命令层的角色矩阵决定。
    pub fn is_admin_card(self) -> bool {
        matches!(
            self,
            Self::Master
                | Self::Reset
                | Self::Clear
                | Self::Install
                | Self::CheckoutHour
                | Self::TimeSync
        )
    }

    /// 空白卡没有卡号，读卡页的「记录区」要显示「无卡号」而不是「未查到」。
    pub fn is_blank(self) -> bool {
        matches!(self, Self::Blank)
    }
}

/// 一张卡的界面快照。字段与 `app/src/api.ts` 的 `CardSnapshot` 一一对应。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardSnapshot {
    pub kind: CardKind,
    /// 中文卡型名，直接上屏（`日租客卡` / `总控卡` …）。
    pub kind_label: String,
    /// 房号文本。**恒为 `None`**，由命令层按 `room_index` 回查业务库后填充。
    pub room: Option<String>,
    /// 房间序号；`-1` = 该卡型不带序号（总控 / 复位 / 清除 / 校时 / 空白）。
    pub room_index: i32,
    /// 天数（日租客卡、按天编码的总控卡，以及月码换算出的估算天数）。
    pub days: i32,
    /// 小时数（钟点客卡、按小时编码的总控卡）。
    pub hours: i32,
    pub card_no: Option<u16>,
    /// 挂失卡（页1[2] 低 4 位 = 1）。
    pub is_lost: bool,
    /// 发卡日 / 小时 / 小时内序号：从卡号解出；无卡号时都是 `-1`。
    pub issue_day: i32,
    pub issue_hour: i32,
    pub issue_seq: i32,
    /// 页2[3]：高 4 位 `(年−2000)&0x0F`、低 4 位月。年份还原由前端按当前年做。
    pub card_ym: u8,
    /// 28 字节原始数据的大写十六进制（56 个字符）。
    pub raw_hex: String,
}

impl CardSnapshot {
    /// 原始数据是否是合法的 28 字节十六进制。不合规时前端会提示「数据异常」。
    pub fn raw_is_28_bytes(&self) -> bool {
        self.raw_hex.len() == 56 && self.raw_hex.chars().all(|c| c.is_ascii_hexdigit())
    }
}

/// 硬件层快照 → 界面快照。
pub fn to_dto(snap: HwSnapshot) -> CardSnapshot {
    let mut dto = CardSnapshot {
        kind: CardKind::from_hw(snap.kind),
        kind_label: snap.kind.label().to_string(),
        room: None,
        room_index: snap.room_index.map_or(-1, i32::from),
        days: 0,
        hours: 0,
        card_no: snap.card_no,
        is_lost: snap.lost,
        issue_day: -1,
        issue_hour: -1,
        issue_seq: -1,
        card_ym: snap.year_month_byte,
        raw_hex: snap.raw_hex(),
    };

    if let Some(duration) = snap.duration {
        match duration {
            CardDuration::Days(d) => dto.days = i32::from(d),
            CardDuration::Hours(h) => dto.hours = i32::from(h),
        }
    }
    if let Some(stamp) = snap.stamp {
        dto.issue_day = i32::from(stamp.day);
        dto.issue_hour = i32::from(stamp.hour);
        dto.issue_seq = i32::from(stamp.seq);
    }

    fix_master_marker(dto, &snap.raw)
}

/// 把「页1 = `5B EC F1 FF`」的总控卡纠正回总控卡，并统一总控卡时长口径。
///
/// # 为什么需要这层纠正
///
/// 硬件层把「页1 第 3 字节 = `F0`、第 4 字节 = `FF`」判为总控卡。真机上见过第 3 字节
/// 是 `F1` 的总控卡（低 4 位是 1，与钟点卡的 0/1 未解规律同源），硬件层会把它按位打包
/// 解成「客卡 序号 4095」。
///
/// # 判据：解出的序号必须是 4095
///
/// 序号是 **12 位**（`B2` 高 4 位 << 4 | `B3`），业务可分配 0–4094，**4095 专门留给总控卡**。
/// 所以「序号 = 4095」等价于 `B2 高 4 位 == 0x0F && B3 == 0xFF`。
///
/// **不能只看 `B3 == 0xFF`**：序号 4080–4094 的 `B3` 同样是 `0xFF`，只看这一字节会把
/// 这些房间的客卡误判成总控卡——产品现在允许分配到 4094，这不是理论风险。
///
/// # 时长口径
///
/// 总控卡的月码（`0xC_` 段）按 **31 天/月**估算展示（3 月 ≈ 93 天）；
/// 硬件层按协议规定用 30 天/月。这里保留产品的展示口径，避免同一张卡
/// 在旧版与新版上显示不同的剩余天数。
fn fix_master_marker(mut dto: CardSnapshot, raw: &[u8; rc_device::CARD_BYTES]) -> CardSnapshot {
    if dto.raw_hex.len() != 56 {
        return dto;
    }

    // 判据是「解出的 12 位序号 == 4095」，不是单纯的 `B3 == 0xFF`。
    // 空白卡（全 FF）的 B2 高 4 位也是 0xF，但它不是客卡，会走 `_` 分支直接返回。
    let is_master_page1 = raw[3] == 0xFF && (raw[2] >> 4) == 0x0F;
    match dto.kind {
        CardKind::GuestDay | CardKind::GuestHour if is_master_page1 => {
            dto.kind = CardKind::Master;
            dto.kind_label = HwCardKind::Master.label().to_string();
            dto.room = None;
            dto.room_index = -1;
        }
        CardKind::Master => {}
        _ => return dto,
    }

    // 只要最终是总控卡，就按总控编码统一补算时长。
    dto.days = 0;
    dto.hours = 0;
    match raw[4] & 0xC0 {
        0x40 => dto.hours = i32::from(raw[4] & 0x3F),
        0x80 => dto.days = i32::from(raw[4] & 0x3F),
        0xC0 => dto.days = i32::from(raw[4] & 0x3F) * 31,
        _ => {}
    }
    dto
}

/// 串口探测结果。字段名沿用前端既有契约（`com` 而不是 `comName`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortProbe {
    /// 串口索引 0–7（0 = COM1）。
    pub port: u8,
    /// 串口名，例如 `"COM7"`。
    pub com: String,
    pub available: bool,
    pub reader_id: Option<String>,
    /// 不可用时的原因；可用时为 `None`。
    pub failure: Option<String>,
}

impl From<HwPortProbe> for PortProbe {
    fn from(p: HwPortProbe) -> Self {
        Self {
            port: p.port,
            com: p.com_name,
            available: p.available,
            reader_id: p.reader_id,
            failure: p.failure,
        }
    }
}

/// 离线自检结果。前端只关心「通过多少、失败多少」。
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

impl From<HwSelfTest> for SelfTest {
    fn from(r: HwSelfTest) -> Self {
        Self {
            passed: i32::try_from(r.passed).unwrap_or(i32::MAX),
            failed: i32::try_from(r.failed).unwrap_or(i32::MAX),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use rc_device::{
        build_guest_day, build_guest_hour, build_master, to_raw28, timestamp_card_no, CardIssuedAt,
        GuestDaySpec, GuestHourSpec, MasterSpec, CARD_BYTES,
    };

    fn issued_at() -> CardIssuedAt {
        CardIssuedAt {
            year: 2026,
            month: 11,
            day: 8,
            hour: 14,
        }
    }

    fn snapshot_of(p1: [u8; 4], p2: [u8; 4]) -> HwSnapshot {
        rc_device::parser::parse(&to_raw28(&p1, &p2)).expect("样本必须能解析")
    }

    /// 日租客卡：天数在 `days`，小时为 0，房号文本留给命令层补。
    #[test]
    fn guest_day_maps_days_and_seq() {
        let spec = GuestDaySpec {
            room_index: 15,
            days: 1,
            card_no: timestamp_card_no(&issued_at(), 0).unwrap(),
            lost: false,
            issued_at: issued_at(),
        };
        let (p1, p2) = build_guest_day(&spec).unwrap();
        let dto = to_dto(snapshot_of(p1, p2));

        assert_eq!(dto.kind, CardKind::GuestDay);
        assert_eq!(dto.kind_label, "日租客卡");
        assert_eq!(dto.room_index, 15);
        assert_eq!(dto.days, 1);
        assert_eq!(dto.hours, 0);
        assert_eq!(dto.card_no, Some(spec.card_no));
        assert!(dto.issue_day >= 1);
        assert!(dto.room.is_none(), "房号文本不该由硬件层给出");
        assert!(dto.raw_is_28_bytes());
    }

    /// 钟点客卡：小时在 `hours`，天数 0。
    #[test]
    fn guest_hour_maps_hours() {
        let spec = GuestHourSpec {
            room_index: 6,
            hours: 3,
            card_no: timestamp_card_no(&issued_at(), 1).unwrap(),
            lost: false,
            issued_at: issued_at(),
        };
        let (p1, p2) = build_guest_hour(&spec).unwrap();
        let dto = to_dto(snapshot_of(p1, p2));

        assert_eq!(dto.kind, CardKind::GuestHour);
        assert_eq!(dto.hours, 3);
        assert_eq!(dto.days, 0);
    }

    /// 挂失卡的低 4 位必须映射成 `is_lost`，而不是混进序号。
    #[test]
    fn lost_flag_is_not_part_of_room_index() {
        let spec = GuestDaySpec {
            room_index: 15,
            days: 1,
            card_no: timestamp_card_no(&issued_at(), 2).unwrap(),
            lost: true,
            issued_at: issued_at(),
        };
        let (p1, p2) = build_guest_day(&spec).unwrap();
        let dto = to_dto(snapshot_of(p1, p2));

        assert!(dto.is_lost);
        assert_eq!(dto.room_index, 15, "挂失位不能混进序号");
    }

    /// 真机样本：页1 = `5B EC F1 FF` 的总控卡会被硬件层解成「客卡 序号 4095」，
    /// 产品侧必须改判回总控卡，并按总控编码重算时长（小时不加 1）。
    #[test]
    fn real_master_card_with_f1ff_page_is_recognised() {
        let raw = snapshot_of([0x5B, 0xEC, 0xF1, 0xFF], [0x41, 0xCF, 0x2B, 0xAB]);
        let dto = to_dto(raw);

        assert_eq!(dto.kind, CardKind::Master, "B3=0xFF 必须判为总控卡");
        assert_eq!(dto.kind_label, "总控卡");
        assert_eq!(dto.room_index, -1);
        assert_eq!(dto.hours, 1, "总控 0x41 = 1 小时（客卡才 +1）");
        assert_eq!(dto.days, 0);
        assert!(dto.card_no.is_some());
    }

    /// 硬件层已经认出的总控卡（页1 `F0FF`）也要走同一套时长口径：
    /// 月码 `0xC3` 按 31 天/月估算 = 93 天。
    #[test]
    fn master_month_code_uses_thirty_one_days() {
        let spec = MasterSpec {
            duration: CardDuration::Days(90),
            card_no: timestamp_card_no(&issued_at(), 3).unwrap(),
            issued_at: issued_at(),
        };
        let (p1, p2) = build_master(&spec).unwrap();
        let dto = to_dto(snapshot_of(p1, p2));

        assert_eq!(dto.kind, CardKind::Master);
        assert_eq!(dto.days, 93, "90 天 → 3 月 → 展示 93 天");
        assert_eq!(dto.hours, 0);
    }

    /// 普通房卡（B3 = 0x00）不能被总控纠正误伤。
    #[test]
    fn ordinary_room_card_is_not_touched() {
        let spec = GuestDaySpec {
            room_index: 15,
            days: 5,
            card_no: timestamp_card_no(&issued_at(), 4).unwrap(),
            lost: false,
            issued_at: issued_at(),
        };
        let (p1, p2) = build_guest_day(&spec).unwrap();
        let dto = to_dto(snapshot_of(p1, p2));

        assert_eq!(dto.kind, CardKind::GuestDay);
        assert_eq!(dto.room_index, 15);
        assert_eq!(dto.days, 5);
    }

    /// 空白卡与「未知卡型」必须分开：擦卡写零后是 `Blank`，而从没写过的全 FF 卡
    /// 在硬件层是 `Unknown`（门锁同样不认它）。把后者显示成「空白卡」会让现场
    /// 以为这张卡还能直接用。
    #[test]
    fn blank_and_unknown_are_distinguished() {
        let zero = rc_device::parser::parse(&[0u8; CARD_BYTES]).unwrap();
        assert_eq!(to_dto(zero).kind, CardKind::Blank);

        let erased_by_ff = rc_device::parser::parse(&[0xFF; CARD_BYTES]).unwrap();
        assert_eq!(to_dto(erased_by_ff).kind, CardKind::Unknown);
    }

    /// 4080–4094 的客卡 `B3` 同样是 `0xFF`：判据必须落在「序号 = 4095」上。
    ///
    /// 这些号现在允许分配给真实房间，误判会把客卡读成总控卡（开不了门还显示成管理卡）。
    #[test]
    fn high_room_index_cards_are_not_mistaken_for_master() {
        for index in [4080u16, 4081, 4094] {
            let spec = GuestDaySpec {
                room_index: index,
                days: 1,
                card_no: timestamp_card_no(&issued_at(), 0).unwrap(),
                lost: false,
                issued_at: issued_at(),
            };
            let (p1, p2) = build_guest_day(&spec).unwrap();
            assert_eq!(p1[3], 0xFF, "前提：序号 {index} 的 B3 是 0xFF");

            let dto = to_dto(snapshot_of(p1, p2));
            assert_eq!(dto.kind, CardKind::GuestDay, "序号 {index} 是客卡，不能改判");
            assert_eq!(dto.kind_label, "日租客卡");
            assert_eq!(dto.room_index, i32::from(index));
            assert_eq!(dto.days, 1);
        }
    }

    /// 真正的总控卡（页1 `F0FF`，序号 4095）仍判总控，且不分配房间序号。
    #[test]
    fn true_master_card_still_has_no_room_index() {
        let spec = MasterSpec {
            duration: CardDuration::Days(1),
            card_no: timestamp_card_no(&issued_at(), 0).unwrap(),
            issued_at: issued_at(),
        };
        let (p1, p2) = build_master(&spec).unwrap();
        assert_eq!(p1, [0x5B, 0xEC, 0xF0, 0xFF], "总控卡页1 恒定");

        let dto = to_dto(snapshot_of(p1, p2));
        assert_eq!(dto.kind, CardKind::Master);
        assert_eq!(dto.room_index, -1, "总控卡没有房间序号");
        assert_eq!(dto.days, 1, "总控卡天数码不加 1");
    }

    /// 卡型分类与前端文案口径必须一致。
    #[test]
    fn kind_classification_matches_ui() {
        assert!(CardKind::Master.is_admin_card());
        assert!(CardKind::Install.is_admin_card());
        assert!(!CardKind::GuestDay.is_admin_card());
        assert!(CardKind::Blank.is_blank());
        assert_eq!(CardKind::from_hw(HwCardKind::CheckoutHour), CardKind::CheckoutHour);
    }
}
