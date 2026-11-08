//! 卡号计数器（`CardNo.state`）与时间戳卡号递推。
//!
//! 2026-11-08 重构：SDK 不再维护卡号状态（`CardNoStore` 已删），改由产品侧接管。
//! CLI 只接收「日期时间 + 小时内序号 + 卡类型」，卡号由调用方算好或用本模块递推。
//!
//! 卡号编码（与 `sdk/PMSLockSDK.cs` 的 `CardBuilder.TimestampCardNo` 一致）：
//! ```text
//! card_no = (日 << 11) | (时 << 6) | 小时内序号
//!           5 位         5 位        6 位（0–62）
//! ```
//! 满 63 张则小时 +1、序号归 0；**进位产生的"未来小时"成为下一次基准，不回退**。

use std::fs;
use std::io;
use std::path::PathBuf;

use chrono::{Datelike, Local, Timelike};

/// 一天内最多可发的卡号数（序号 0–62）
pub const SEQS_PER_HOUR: u16 = 63;

/// 把 `(日, 时, 序号)` 编码成 16 位卡号。
pub fn encode(day: u16, hour: u16, seq: u16) -> u16 {
    ((day & 0x1F) << 11) | ((hour & 0x1F) << 6) | (seq & 0x3F)
}

/// 从卡号解出 `(日, 时, 序号)`。
pub fn decode(card_no: u16) -> (u16, u16, u16) {
    ((card_no >> 11) & 0x1F, (card_no >> 6) & 0x1F, card_no & 0x3F)
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

    /// 按 `now` 递推下一个卡号并落盘。
    ///
    /// 规则：
    /// - 与上次同一小时 → 序号 +1；满 63 则小时 +1、序号归 0
    /// - `now` 更晚 → 用 `now` 的 (日, 时, 0)
    /// - `now` 更早（时钟回拨）→ **沿用上次值**，不回退
    pub fn next_at(&self, now: chrono::DateTime<Local>) -> u16 {
        let last = self.load().unwrap_or(0);
        let (ld, lh, lq) = decode(last);
        let nd = now.day() as u16;
        let nh = now.hour() as u16;

        let (d, h, q) = if (nd, nh) > (ld, lh) {
            (nd, nh, 0)
        } else if (nd, nh) == (ld, lh) {
            if lq + 1 >= SEQS_PER_HOUR {
                (nd, nh + 1, 0)
            } else {
                (nd, nh, lq + 1)
            }
        } else {
            (ld, lh, lq)
        };

        let value = encode(d, h, q);
        let _ = self.save(value);
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_roundtrip() {
        assert_eq!(encode(8, 19, 0), 0x44C0);
        assert_eq!(decode(0x44C0), (8, 19, 0));
        assert_eq!(encode(11, 19, 0), 0x5CC0);
        assert_eq!(decode(0x5CC0), (11, 19, 0));
        // 实测样本：0x5B34 = 11 日 12 点第 52 张
        assert_eq!(decode(0x5B34), (11, 12, 52));
    }
}
