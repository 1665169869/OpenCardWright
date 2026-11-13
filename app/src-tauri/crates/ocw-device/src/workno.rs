//! 作业流水号（10 位 `YYMMDDHHII`）的拼装。
//!
//! 卡内的流水号拆成两处存：
//! - `页2[3]`：`YYMM`（高 4 位 = `(年−2000) & 0x0F`、低 4 位 = 月，**16 年一循环**）；
//! - 16 位卡号：`DDHHII`（日 5 位 | 时 5 位 | 小时内序号 6 位）。
//!
//! 界面要显示**完整 10 位**，所以这里把两段拼回去；年份按 16 年周期还原
//! （取「当前年所属周期 + 卡内高 4 位」，算成未来年份就退一个周期）。

use std::time::{SystemTime, UNIX_EPOCH};

/// 拼出 10 位作业流水号 `YYMMDDHHII`。
///
/// `card_no` 为空（安装卡 / 空白卡 / 原始直写）时不占号，返回 `None`；
/// `raw_hex` 缺失或页2[3] 的月份非法时也返回 `None`（宁可显示「无流水号」也不猜）。
pub fn work_no(card_no: Option<u16>, raw_hex: Option<&str>) -> Option<String> {
    let card_no = card_no?;
    let (yy, mm) = year_month_of(raw_hex?)?;
    let stamp = rc_device::decode_timestamp_card_no(card_no);
    Some(format!(
        "{yy:02}{mm:02}{:02}{:02}{:02}",
        stamp.day, stamp.hour, stamp.seq
    ))
}

/// 从整卡指纹里取 `YYMM`：页2 的第 4 个字节，即 `raw_hex` 的下标 7。
fn year_month_of(raw_hex: &str) -> Option<(u32, u32)> {
    let bytes = parse_hex(raw_hex)?;
    let b = *bytes.get(7)?;
    let month = u32::from(b & 0x0F);
    if !(1..=12).contains(&month) {
        return None;
    }
    let high = u32::from((b >> 4) & 0x0F);
    let now = current_year();
    // 16 年一循环：先在当前年所属周期里还原，算成未来年份就退一个周期。
    let mut year = 2000 + 16 * (now.saturating_sub(2000) / 16) + high;
    if year > now {
        year -= 16;
    }
    Some((year % 100, month))
}

fn parse_hex(text: &str) -> Option<Vec<u8>> {
    let text = text.trim();
    if text.is_empty() || text.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    for i in (0..text.len()).step_by(2) {
        out.push(u8::from_str_radix(text.get(i..i + 2)?, 16).ok()?);
    }
    Some(out)
}

/// 当前公历年（不引 chrono：Howard Hinnant 的 `civil_from_days`）。
fn current_year() -> u32 {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_no_joins_year_month_with_day_hour_seq() {
        // 2026-11-14 02:xx 第 1 张：页2[3] = 0xAB（26 年 11 月），卡号 0x7080 → 日 14 / 时 02 / 序 00
        assert_eq!(
            work_no(Some(0x7080), Some("5BECF000810424AB")),
            Some("2611140200".to_string())
        );
    }

    #[test]
    fn missing_card_no_means_no_work_number() {
        // 安装卡 / 空白卡 / 原始直写不占号
        assert_eq!(work_no(None, Some("5BECFC0046F20001")), None);
    }

    #[test]
    fn missing_or_bad_raw_hex_yields_none() {
        assert_eq!(work_no(Some(0x7080), None), None);
        assert_eq!(
            work_no(Some(0x7080), Some("5BECF000")),
            None,
            "长度不足没有页2[3]"
        );
        assert_eq!(work_no(Some(0x7080), Some("zz")), None, "非十六进制");
        assert_eq!(
            work_no(Some(0x7080), Some("5BECF000810424A0")),
            None,
            "月份 0 不合法"
        );
    }
}
