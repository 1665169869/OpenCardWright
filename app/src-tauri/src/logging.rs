//! 文件日志：按天落盘 + 超过 5 MB 轮转 + 只留最近 5 份。
//!
//! 为什么不引 `tauri-plugin-log`：本项目一路零额外依赖，而这里需要的东西很少 ——
//! 一个串行化的写句柄、一个按天文件名、一次阈值检查。真正需要小心的反而是**不能让日志拖垮发卡**：
//! 所有 IO 错误一律吞掉（只回声到 stderr），绝不让写日志的失败冒泡到业务路径。
//!
//! 时间：不引 chrono。启动时由调用方给一次「本地时区相对 UTC 的偏移秒数」（用 SQLite 的
//! `strftime(...,'localtime')` 求出来），之后自己把 epoch 转公历。中国无夏令时，固定偏移安全。

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// 单文件上限：超过就改名归档
const MAX_BYTES: u64 = 5 * 1024 * 1024;
/// 保留份数（含当天这份）
const KEEP: usize = 5;

struct Logger {
    dir: PathBuf,
    day: String,
    file: File,
    size: u64,
    wrote: u64,
    offset_secs: i64,
}

static LOG: OnceLock<Mutex<Logger>> = OnceLock::new();

/// 初始化（进程内只需调一次；重复调用无副作用）。
pub fn init(dir: PathBuf, offset_secs: i64) {
    if LOG.get().is_some() {
        return;
    }
    let _ = std::fs::create_dir_all(&dir);
    let day = day_string(offset_secs);
    let path = dir.join(format!("ocw-{day}.log"));
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .or_else(|_| File::create(&path))
        .ok();
    let Some(file) = file else {
        eprintln!("[log] 无法创建日志文件：{}（后续只回声到终端）", path.display());
        return;
    };
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    let logger = Logger {
        dir,
        day,
        file,
        size,
        wrote: 0,
        offset_secs,
    };
    let _ = LOG.set(Mutex::new(logger));
}

/// 写一行。失败只回声 stderr，不影响调用方。
pub fn line(level: &str, tag: &str, msg: &str) {
    let Some(lock) = LOG.get() else {
        eprintln!("[{tag}][{level}] {msg}");
        return;
    };
    let Ok(mut g) = lock.lock() else { return };
    let (stamp, day) = now_stamp(g.offset_secs);
    let text = format!("{stamp} {level:<5} [{tag}] {}\n", msg.replace(['\r', '\n'], " "));

    // 跨天或超限就换文件（跨天在多数场景下一天一个文件，轮转只兜"一天写爆"的情况）
    if day != g.day {
        if let Some(next) = reopen(&mut g, &day, false) {
            let _ = next;
        }
    } else if g.size > MAX_BYTES {
        let _ = reopen(&mut g, &day, true);
    }

    if g.file.write_all(text.as_bytes()).is_ok() {
        g.size += text.len() as u64;
        g.wrote += 1;
        // 每 64 行才核对一次真实大小，避免每行一次 metadata 系统调用
        if g.wrote % 64 == 0 {
            if let Ok(meta) = g.file.metadata() {
                g.size = meta.len();
            }
        }
    }
}

/// 换新文件。`archive` = 超限轮转（把旧文件改名保留），否则只是跨天换名。
fn reopen(g: &mut Logger, day: &str, archive: bool) -> Option<()> {
    if archive {
        let _ = g.file.flush();
        let stamp = now_stamp(g.offset_secs).0.replace([':', '-'], "").replace(' ', "-");
        let old = g.dir.join(format!("ocw-{day}-{stamp}.log"));
        let _ = std::fs::rename(g.dir.join(format!("ocw-{day}.log")), &old);
    }
    let path = g.dir.join(format!("ocw-{day}.log"));
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()?;
    g.file = file;
    g.day = day.to_string();
    g.size = g.file.metadata().map(|m| m.len()).unwrap_or(0);
    g.wrote = 0;
    if archive {
        prune(&g.dir);
    }
    Some(())
}

/// 只保留最近 `KEEP` 个日志文件（按文件名排序，`ocw-YYYYMMDD...` 天然有序）。
fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .map(|n| {
                        let n = n.to_string_lossy();
                        n.starts_with("ocw-") && n.ends_with(".log")
                    })
                    .unwrap_or(false)
        })
        .collect();
    files.sort();
    let excess = files.len().saturating_sub(KEEP);
    for p in files.into_iter().take(excess) {
        let _ = std::fs::remove_file(p);
    }
}

/// 读最近 `n` 行（给诊断包用）。只读当天与归档文件，按文件名倒序。
pub fn recent(n: usize) -> Vec<String> {
    let Some(lock) = LOG.get() else {
        return Vec::new();
    };
    let Ok(g) = lock.lock() else { return Vec::new() };
    let dir = g.dir.clone();
    drop(g);

    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "log").unwrap_or(false))
        .collect();
    files.sort();
    files.reverse();

    let mut out: Vec<String> = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for l in text.lines().rev() {
            out.push(l.to_string());
            if out.len() >= n {
                break;
            }
        }
        if out.len() >= n {
            break;
        }
    }
    out.reverse();
    out
}

fn now_stamp(offset_secs: i64) -> (String, String) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        + offset_secs;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let hh = rem / 3600;
    let mm = (rem % 3600) / 60;
    let ss = rem % 60;
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_millis())
        .unwrap_or(0);
    (
        format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}.{millis:03}"),
        format!("{y:04}{m:02}{d:02}"),
    )
}

fn day_string(offset_secs: i64) -> String {
    now_stamp(offset_secs).1
}

/// Howard Hinnant 的 `civil_from_days`：把「epoch 起的天数」转成公历年月日。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_000), (2022, 1, 8));
        // 2026-11-06 = 1970-01-01 起第 20763 天（用 Python datetime 核过）
        assert_eq!(civil_from_days(20_763), (2026, 11, 6));
    }

    #[test]
    fn logger_writes_and_rotates() {
        let dir = std::env::temp_dir().join(format!(
            "ocw-log-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // 直接构造一个已超限的 logger，避免真的写 5MB
        std::fs::create_dir_all(&dir).unwrap();
        let day = day_string(0);
        let path = dir.join(format!("ocw-{day}.log"));
        std::fs::write(&path, "x").unwrap();
        let file = OpenOptions::new().append(true).open(&path).unwrap();
        let mut g = Logger {
            dir: dir.clone(),
            day: day.clone(),
            file,
            size: MAX_BYTES + 1,
            wrote: 0,
            offset_secs: 0,
        };
        reopen(&mut g, &day, true).expect("轮转应成功");

        let files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert!(files.len() >= 2, "轮转后应同时有归档与当天文件");
        assert!(
            files
                .iter()
                .any(|f| f.file_name().to_string_lossy().contains(&format!("ocw-{day}-"))),
            "应生成带时间戳的归档文件"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_keeps_only_recent_five() {
        let dir = std::env::temp_dir().join(format!(
            "ocw-logp-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        for i in 1..=8 {
            std::fs::write(dir.join(format!("ocw-2026010{i}.log")), "x").unwrap();
        }
        std::fs::write(dir.join("other.txt"), "x").unwrap();
        prune(&dir);
        let logs = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".log"))
            .count();
        assert_eq!(logs, KEEP);
        assert!(dir.join("other.txt").exists(), "非日志文件不能删");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
