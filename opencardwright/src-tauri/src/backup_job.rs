//! 自动备份后台任务。
//!
//! 独立线程每 30 秒醒一次，读设置判断"到点没有"，到点就用 `ocw_store::backup_to`
//! 在线备份一次。选线程轮询而不是定时器，是为了**设置改完不用重启**：
//! 间隔、保留份数都从 `setting` 表现读，用户改完下一轮就生效。
//!
//! 两条保守规则：
//! * 只清理自己产生的 `auto-*.db`，手动备份与 `.before-restore-*.bak` 永远不动；
//! * 有任何一步出错只打日志，不 panic —— 备份任务挂掉不能拖垮发卡主流程。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ocw_store::DbConnection as Connection;

/// 设置键：自动备份间隔（分钟，0 = 关闭）
pub const SETTING_INTERVAL: &str = "backupIntervalMinutes";
/// 设置键：自动备份保留份数（只对 `auto-*.db` 生效）
pub const SETTING_KEEP: &str = "backupKeep";
/// 设置键：上次成功备份的 Unix 秒（手动备份也会更新它，避免刚备份完又自动备份一次）
pub const SETTING_LAST_AT: &str = "backupLastAt";

pub const DEFAULT_INTERVAL_MINUTES: i64 = 60;
pub const DEFAULT_KEEP: i64 = 24;

/// 备份目录：与数据库同级的 `backups\`。
pub fn backups_dir(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("backups")
}

fn now_epoch(conn: &Connection) -> i64 {
    conn.query_row("SELECT CAST(strftime('%s','now') AS INTEGER)", [], |r| r.get(0))
        .unwrap_or(0)
}

fn read_i64(conn: &Connection, key: &str, default: i64) -> i64 {
    ocw_store::get_setting(conn, key)
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(default)
}

/// 自动备份一轮：到点就备份 + 顺带清理超额的 `auto-*.db`。
///
/// 返回是否真的备份了（给单元测试与日志用）。
pub fn tick(conn: &Connection, db_path: &Path) -> bool {
    let interval = read_i64(conn, SETTING_INTERVAL, DEFAULT_INTERVAL_MINUTES);
    if interval <= 0 {
        return false;
    }
    let keep = read_i64(conn, SETTING_KEEP, DEFAULT_KEEP).clamp(1, 500);

    let now = now_epoch(conn);
    let last = read_i64(conn, SETTING_LAST_AT, 0);
    if last > 0 && now - last < interval * 60 {
        return false;
    }

    let dir = backups_dir(db_path);
    let stamp = conn
        .query_row(
            "SELECT strftime('%Y%m%d-%H%M%S', 'now', 'localtime')",
            [],
            |r| r.get::<_, String>(0),
        )
        .unwrap_or_else(|_| format!("{now}"));
    let target = dir.join(format!("auto-{stamp}.db"));

    match ocw_store::backup_to(conn, &target) {
        Ok(bytes) => {
            eprintln!("[backup] 自动备份完成：{}（{} 字节）", target.display(), bytes);
            let _ = ocw_store::set_setting(conn, SETTING_LAST_AT, &now.to_string());
            prune_auto(&dir, keep);
            true
        }
        Err(e) => {
            eprintln!("[backup] 自动备份失败：{e}（下一轮重试，不动现有数据）");
            false
        }
    }
}

/// 只保留最近 `keep` 份自动备份。**只删 `auto-` 前缀**，手动备份与安全副本不碰。
fn prune_auto(dir: &Path, keep: i64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut autos: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .map(|n| n.to_string_lossy().starts_with("auto-"))
                    .unwrap_or(false)
        })
        .collect();
    // 文件名里的时间戳可按字典序排序（auto-YYYYmmdd-HHmmss.db）
    autos.sort();
    let excess = autos.len() as i64 - keep;
    if excess <= 0 {
        return;
    }
    for path in autos.into_iter().take(excess as usize) {
        if std::fs::remove_file(&path).is_ok() {
            eprintln!("[backup] 清理旧自动备份：{}", path.display());
        }
    }
}

/// 启动后台线程（进程生命周期内常驻）。
pub fn spawn(db: Arc<Mutex<Connection>>, db_path: PathBuf) {
    std::thread::Builder::new()
        .name("ocw-auto-backup".into())
        .spawn(move || {
            // 启动后先等一轮，避免和开库、迁移抢锁
            loop {
                std::thread::sleep(Duration::from_secs(30));
                let Ok(conn) = db.lock() else {
                    continue;
                };
                let _ = tick(&conn, &db_path);
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(tag: &str) -> PathBuf {
        // 不引 uuid：纳秒时间戳足够区分测试目录
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("ocw-{tag}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 没有上次备份时间 → 第一轮就该备；紧接着第二轮不该重复备。
    #[test]
    fn tick_backs_up_once_then_waits_for_interval() {
        let dir = temp_dir("autobak");
        let db_path = dir.join("ocw.db");
        let conn = ocw_store::open(&db_path).unwrap();

        assert!(tick(&conn, &db_path), "首次应备份");
        let files: Vec<_> = fs::read_dir(backups_dir(&db_path)).unwrap().flatten().collect();
        assert_eq!(files.len(), 1, "应生成 1 个 auto-*.db");

        assert!(!tick(&conn, &db_path), "刚备完不应再备一次");
        let files: Vec<_> = fs::read_dir(backups_dir(&db_path)).unwrap().flatten().collect();
        assert_eq!(files.len(), 1, "没有被重复写入");

        drop(conn);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 间隔 0 = 关闭：一个文件都不该产生。
    #[test]
    fn interval_zero_disables_auto_backup() {
        let dir = temp_dir("autobak0");
        let db_path = dir.join("ocw.db");
        let conn = ocw_store::open(&db_path).unwrap();
        ocw_store::set_setting(&conn, SETTING_INTERVAL, "0").unwrap();

        assert!(!tick(&conn, &db_path));
        assert!(!backups_dir(&db_path).exists(), "关闭时不该建目录");

        drop(conn);
        let _ = fs::remove_dir_all(&dir);
    }

    /// 清理只删 auto-*，手动备份与安全副本一根头发都不能动。
    #[test]
    fn prune_only_touches_auto_files() {
        let dir = temp_dir("prune");
        let d = dir.join("backups");
        fs::create_dir_all(&d).unwrap();
        for i in 1..=5 {
            fs::write(d.join(format!("auto-2026010{i}-000000.db")), b"x").unwrap();
        }
        fs::write(d.join("ocw-20260101-000000.db"), b"manual").unwrap();
        fs::write(d.join("opencardwright.db.before-restore-20260101-000000.bak"), b"safety").unwrap();

        prune_auto(&d, 2);

        let autos = fs::read_dir(&d)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("auto-"))
            .count();
        assert_eq!(autos, 2, "自动备份只留 2 份（最新的）");
        assert!(d.join("ocw-20260101-000000.db").exists(), "手动备份不能被删");
        assert!(
            d.join("opencardwright.db.before-restore-20260101-000000.bak").exists(),
            "安全副本不能被删"
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
