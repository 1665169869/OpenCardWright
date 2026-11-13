//! 卡号计数器（`CardNo.state`）与时间戳卡号递推。
//!
//! # 编码规则不在本模块
//!
//! `card_no = (日 << 11) | (时 << 6) | 小时内序号` 是**卡内协议**，现在统一由
//! `rc_device::timestamp_card_no` / `decode_timestamp_card_no` 实现——那边有 46 项
//! 黄金向量守着。本模块只保留产品独有的两件事：
//!
//! 1. `CardNo.state` 的落盘（现场数据，升级/重装都不该覆盖）；
//! 2. **时钟回拨不推进卡号**这条现场安全规则：回拨时如果照常发号，会把已被占用的
//!    卡号重新发一遍，等于用新卡覆盖旧卡的授权。
//!
//! # 进位为什么交给 `next_stamp`
//!
//! 「本小时满 63 张 → 小时 +1」会顺带撞上 23 点 → 次日 0 点，再往上是跨月、跨年、
//! 闰年。这些日历进位在 `rc_device::next_stamp` 里已经实现并有单测，本模块不重复实现
//! （以前重复实现的版本在 23 点会算出「24 点」这种不存在的卡号）。

use std::cmp::Ordering;
use std::fs;
use std::io;
use std::path::PathBuf;

use chrono::{Datelike, Local, Timelike};
use rc_device::{decode_timestamp_card_no, next_stamp, timestamp_card_no, CardIssuedAt};

/// 从卡号解出 `(日, 时, 小时内序号)`。
pub fn decode(card_no: u16) -> (u16, u16, u16) {
    let stamp = decode_timestamp_card_no(card_no);
    (
        u16::from(stamp.day),
        u16::from(stamp.hour),
        u16::from(stamp.seq),
    )
}

/// 本地时刻 → 硬件层的发卡时刻。
fn issued_at(now: chrono::DateTime<Local>) -> CardIssuedAt {
    CardIssuedAt {
        year: now.year(),
        month: u8::try_from(now.month()).unwrap_or(1),
        day: u8::try_from(now.day()).unwrap_or(1),
        hour: u8::try_from(now.hour()).unwrap_or(0),
    }
}

/// `CardNo.state` 的读写封装（16 进制文本 + 原子替换）。
pub struct CardNoState {
    path: PathBuf,
}

impl CardNoState {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// 读当前计数值；文件不存在或内容非法时返回 `None`。
    pub fn load(&self) -> Option<u16> {
        let text = fs::read_to_string(&self.path).ok()?;
        u16::from_str_radix(text.trim(), 16).ok()
    }

    /// 写回计数值（先写 `.tmp` 再 `rename`，避免半截文件）。
    pub fn save(&self, value: u16) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            if !dir.as_os_str().is_empty() {
                fs::create_dir_all(dir)?;
            }
        }
        let tmp = self.path.with_extension("state.tmp");
        fs::write(&tmp, format!("{value:04X}"))?;
        if self.path.exists() {
            let _ = fs::remove_file(&self.path);
        }
        fs::rename(&tmp, &self.path)
    }

    /// 推进一张卡：返回 `(卡号, 发卡时刻)`，并落盘。
    ///
    /// # 为什么两者要一起返回
    ///
    /// 页2[3] 的年月与卡号里的日/时必须来自**同一个**时刻。分开算就会出现
    /// 「卡号说 15 点、年月还是上个月」这种自相矛盾的卡——门锁按卡内时间算到期，
    /// 差一个月等于差一个月租期。
    ///
    /// # 规则
    ///
    /// - 同一「日/时」→ 序号 +1；满 63 则小时 +1、序号归 0（跨日/月/年由 `next_stamp` 处理）
    /// - `now` 更晚 → 用 `now` 的 `(日, 时, 0)`
    /// - `now` 更早（时钟回拨，含跨月首日）→ **沿用上次值，不回退**
    ///
    /// 跨月首日会被判成「回拨」而沿用上次卡号：这是旧实现就有的保守行为——
    /// 宁可少发一张、让前台重试，也不要在时间异常时把旧卡号重新发出去。
    pub fn next_card(&self, now: chrono::DateTime<Local>) -> (u16, CardIssuedAt) {
        let current = self.load().unwrap_or(0);
        let at = issued_at(now);

        let (value, issued) = if current == 0 {
            // 没有历史：本小时第 1 张（序号 0）
            (timestamp_card_no(&at, 0).unwrap_or(0), at)
        } else {
            let stamp = decode_timestamp_card_no(current);
            let now_ord = (u16::from(at.day), u16::from(at.hour));
            let last_ord = (u16::from(stamp.day), u16::from(stamp.hour));

            match now_ord.cmp(&last_ord) {
                // 换小时 / 换天：从新时刻的第 1 张开始
                Ordering::Greater => (timestamp_card_no(&at, 0).unwrap_or(current), at),
                // 同一小时：序号 +1，满 63 进位（含跨日、跨月、闰年）
                Ordering::Equal => {
                    let (next, nth) = next_stamp(current, &at);
                    let card_no = timestamp_card_no(&next, nth.saturating_sub(1)).unwrap_or(current);
                    (card_no, next)
                }
                // 时钟回拨：沿用上次卡号，时刻沿用卡号里的日/时（年月取当前，仅用于页2[3]）
                Ordering::Less => (
                    current,
                    CardIssuedAt {
                        day: stamp.day,
                        hour: stamp.hour,
                        ..at
                    },
                ),
            }
        };

        let _ = self.save(value);
        (value, issued)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(year: i32, month: u32, day: u32, hour: u32) -> chrono::DateTime<Local> {
        Local
            .with_ymd_and_hms(year, month, day, hour, 0, 0)
            .single()
            .expect("测试时刻必须存在")
    }

    fn temp_state(name: &str) -> CardNoState {
        let path = std::env::temp_dir().join(format!(
            "ocw-cardno-{}-{name}.state",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        CardNoState::new(path)
    }

    fn next(state: &CardNoState, now: chrono::DateTime<Local>) -> u16 {
        state.next_card(now).0
    }

    /// 编解码必须与硬件层一致（含真机样本 0x5B34 = 11 日 12 点第 52 张）。
    #[test]
    fn decode_matches_hardware_layer() {
        assert_eq!(decode(0x44C0), (8, 19, 0));
        assert_eq!(decode(0x5CC0), (11, 19, 0));
        assert_eq!(decode(0x5B34), (11, 12, 52));
    }

    /// 没有状态文件时从当前小时第 1 张开始。
    #[test]
    fn first_issue_starts_at_seq_zero() {
        let state = temp_state("first");
        let value = next(&state, at(2026, 11, 8, 14));
        assert_eq!(decode(value), (8, 14, 0));
        assert_eq!(state.load(), Some(value), "必须落盘");
    }

    /// 同一小时内逐张递增。
    #[test]
    fn same_hour_increments() {
        let state = temp_state("same-hour");
        let a = next(&state, at(2026, 11, 8, 14));
        let b = next(&state, at(2026, 11, 8, 14));
        let c = next(&state, at(2026, 11, 8, 14));

        assert_eq!(decode(a), (8, 14, 0));
        assert_eq!(decode(b), (8, 14, 1));
        assert_eq!(decode(c), (8, 14, 2));
    }

    /// 换小时重新从 0 开始。
    #[test]
    fn new_hour_resets_seq() {
        let state = temp_state("new-hour");
        next(&state, at(2026, 11, 8, 14));
        next(&state, at(2026, 11, 8, 14));
        let value = next(&state, at(2026, 11, 8, 15));
        assert_eq!(decode(value), (8, 15, 0));
    }

    /// 满 63 张进位到下一小时（序号归 0）。
    #[test]
    fn overflow_moves_to_next_hour() {
        let state = temp_state("overflow");
        let _ = state.save((8 << 11) | (14 << 6) | 62);
        let value = next(&state, at(2026, 11, 8, 14));
        assert_eq!(decode(value), (8, 15, 0));
    }

    /// 23 点满 63 张必须进位到**次日 0 点**，不能算出「24 点」。
    #[test]
    fn overflow_at_23_crosses_to_next_day() {
        let state = temp_state("midnight");
        let _ = state.save((8 << 11) | (23 << 6) | 62);
        let value = next(&state, at(2026, 11, 8, 23));
        assert_eq!(decode(value).0, 9, "必须跨到次日");
        assert_eq!(decode(value).1, 0, "小时必须是 0，不能是 24");
        assert_eq!(decode(value).2, 0);
    }

    /// 时钟回拨：沿用上次值，绝不发一个更早的卡号。
    #[test]
    fn clock_rollback_does_not_rewind() {
        let state = temp_state("rollback");
        let issued = next(&state, at(2026, 11, 8, 14));
        let rolled_back = next(&state, at(2026, 11, 7, 9));

        assert_eq!(
            rolled_back, issued,
            "回拨时必须沿用上次卡号，不能重发旧号"
        );
    }

    /// 返回的发卡时刻必须与卡号里的日/时一致（页2[3] 的年月靠它）。
    #[test]
    fn issued_at_matches_card_no() {
        let state = temp_state("same-source");
        let (card_no, issued) = state.next_card(at(2026, 11, 8, 14));
        let (day, hour, _) = decode(card_no);
        assert_eq!(u16::from(issued.day), day);
        assert_eq!(u16::from(issued.hour), hour);
        assert_eq!(issued.year, 2026);
        assert_eq!(issued.month, 11);
    }

    /// 进位后时刻也要跟着走：23 点溢出那张卡的 `issued_at` 必须是次日 0 点。
    #[test]
    fn issued_at_follows_midnight_carry() {
        let state = temp_state("same-source-midnight");
        let _ = state.save((8 << 11) | (23 << 6) | 62);
        let (card_no, issued) = state.next_card(at(2026, 11, 8, 23));
        let (day, hour, _) = decode(card_no);
        assert_eq!(issued.day, 9);
        assert_eq!(issued.hour, 0);
        assert_eq!(u16::from(issued.day), day);
        assert_eq!(u16::from(issued.hour), hour);
    }
}
