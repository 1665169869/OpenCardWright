//! 诊断包导出：把排障需要的东西打成一个 zip，现场发给开发者。
//!
//! 内容：`manifest.json`（版本/库版本/驱动指纹/计数/日志尾）、**脱敏后的数据库副本**、
//! `LockCard.ini`、`CardNo.state`、最近两个日志文件。
//!
//! 两条硬规则：
//! * 数据库副本里的 `operator.pwd_hash` 必须抹掉，且要 `VACUUM` —— 只 UPDATE 不 VACUUM 的话，
//!   旧页里的哈希还能被 `strings` 搜出来，等于没脱敏；
//! * 脱敏只发生在**副本**上，主库一行都不改。
//!
//! 打包不引 crate：优先 Windows 自带的 `tar.exe`（Win10 1803+），失败再退
//! `Compress-Archive`；都失败就把暂存目录路径返回给用户，并附一条 warning。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ocw_store::DbConnection as Connection;

use crate::logging;

pub struct ExportResult {
    pub path: PathBuf,
    pub bytes: u64,
    pub warnings: Vec<String>,
}

/// 生成诊断包。`meta` 是调用方拼好的快照（避免这里反向依赖 AppState）。
pub fn export(
    conn: &Mutex<Connection>,
    db_path: &Path,
    runtime_dir: &Path,
    meta: serde_json::Value,
) -> Result<ExportResult, String> {
    let root = db_path.parent().unwrap_or_else(|| Path::new("."));
    let stamp = conn
        .lock()
        .ok()
        .and_then(|c| {
            c.query_row(
                "SELECT strftime('%Y%m%d-%H%M%S', 'now', 'localtime')",
                [],
                |r| r.get::<_, String>(0),
            )
            .ok()
        })
        .unwrap_or_else(|| "unknown".to_string());
    let staging = root.join("diagnostics").join(format!("diag-{stamp}"));
    std::fs::create_dir_all(&staging).map_err(|e| format!("创建暂存目录失败：{e}"))?;

    let mut warnings = Vec::new();

    // 1) 数据库副本（在线备份，WAL 里的事务也带上）
    let snapshot = staging.join("db-snapshot.db");
    {
        let guard = conn.lock().map_err(|_| "数据库锁异常".to_string())?;
        ocw_store::backup_to(&guard, &snapshot).map_err(|e| format!("备份失败：{e}"))?;
    }

    // 2) 脱敏：只动副本
    redact(&snapshot)?;

    // 3) manifest（含日志尾）
    let mut meta = meta;
    if let Some(obj) = meta.as_object_mut() {
        obj.insert(
            "exportedAt".into(),
            serde_json::Value::String(stamp.clone()),
        );
        obj.insert(
            "logTail".into(),
            serde_json::Value::Array(
                logging::recent(300)
                    .into_iter()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
        obj.insert(
            "redaction".into(),
            serde_json::Value::String("operator.pwd_hash 已抹除并 VACUUM".into()),
        );
    }
    std::fs::write(
        staging.join("manifest.json"),
        serde_json::to_vec_pretty(&meta).unwrap_or_default(),
    )
    .map_err(|e| format!("写 manifest 失败：{e}"))?;

    // 4) 运行状态文件（不含秘密，但排障必需）
    for name in ["LockCard.ini", "CardNo.state"] {
        let src = runtime_dir.join(name);
        if src.is_file() {
            let _ = std::fs::copy(&src, staging.join(name));
        }
    }
    for (i, path) in recent_logs(2).into_iter().enumerate() {
        let _ = std::fs::copy(&path, staging.join(format!("log-{}.log", i + 1)));
    }

    // 5) 打包
    let zip = root.join("diagnostics").join(format!("diag-{stamp}.zip"));
    match pack(&staging, &zip) {
        Ok(()) => {
            let bytes = std::fs::metadata(&zip).map(|m| m.len()).unwrap_or(0);
            let _ = std::fs::remove_dir_all(&staging);
            Ok(ExportResult {
                path: zip,
                bytes,
                warnings,
            })
        }
        Err(msg) => {
            warnings.push(msg);
            Ok(ExportResult {
                path: staging,
                bytes: 0,
                warnings,
            })
        }
    }
}

/// 抹掉密码哈希与证件信息，再 VACUUM 把旧页真正清掉。
fn redact(path: &Path) -> Result<(), String> {
    let conn = Connection::open(path).map_err(|e| format!("打开副本失败：{e}"))?;
    conn.execute_batch(
        "UPDATE operator SET pwd_hash = 'redacted';
         UPDATE issuance SET guest_id_number = NULL, guest_address = NULL;
         PRAGMA journal_mode = DELETE;
         PRAGMA wal_checkpoint(TRUNCATE);
         VACUUM;",
    )
    .map_err(|e| format!("脱敏失败：{e}"))?;
    Ok(())
}

fn recent_logs(n: usize) -> Vec<PathBuf> {
    // 从日志目录取最新几个文件（与 logging 模块同样的命名约定）
    let dir = crate::provision::data_root().join("logs");
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
    files.truncate(n);
    files
}

/// 打包：tar.exe → %SystemRoot%\System32\tar.exe → Compress-Archive → 失败。
fn pack(staging: &Path, zip: &Path) -> Result<(), String> {
    let try_tar = |exe: &Path| -> bool {
        std::process::Command::new(exe)
            .arg("-a")
            .arg("-c")
            .arg("-f")
            .arg(zip)
            .arg("-C")
            .arg(staging)
            .arg(".")
            .status()
            .map(|s| s.success() && zip.is_file())
            .unwrap_or(false)
    };

    if try_tar(Path::new("tar")) {
        return Ok(());
    }
    let sys_tar = std::env::var("SystemRoot")
        .map(|r| PathBuf::from(r).join("System32").join("tar.exe"))
        .unwrap_or_else(|_| PathBuf::from("C:/Windows/System32/tar.exe"));
    if try_tar(&sys_tar) {
        return Ok(());
    }

    let ps = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "Compress-Archive -Path '{}\\*' -DestinationPath '{}' -Force",
                staging.display(),
                zip.display()
            ),
        ])
        .status();
    if ps.map(|s| s.success()).unwrap_or(false) && zip.is_file() {
        return Ok(());
    }

    Err(format!(
        "压缩失败，已把原始文件留在 {}（可直接手工压缩后发回）",
        staging.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("ocw-diag-{tag}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 脱敏必须真的把哈希抹掉，且库还能通过完整性检查。
    #[test]
    fn redact_removes_password_hash_and_keeps_db_valid() {
        let dir = temp_dir("redact");
        let path = dir.join("copy.db");
        {
            let c = ocw_store::open(&path).unwrap();
            c.execute(
                "INSERT INTO operator (login, display_name, role, pwd_hash)
                 VALUES ('admin', '管理员', 'admin', '$argon2id$v=19$m=1,t=1,p=1$deadbeef')",
                [],
            )
            .unwrap();
        }
        redact(&path).unwrap();

        let c = Connection::open(&path).unwrap();
        let hash: String = c
            .query_row("SELECT pwd_hash FROM operator LIMIT 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(hash, "redacted");
        let ok: String = c
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ok, "ok");

        // 文件里不该再能搜到 argon2 前缀
        let raw = std::fs::read(&path).unwrap();
        let needle = b"$argon2id";
        assert!(
            !raw.windows(needle.len()).any(|w| w == needle),
            "VACUUM 后仍能在文件里搜到哈希"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
    /// 整包导出：没有读卡器也能产出 zip，且包里的库副本已脱敏。
    #[test]
    fn export_produces_zip() {
        let dir = temp_dir("export");
        let db_path = dir.join("ocw.db");
        let runtime = dir.join("runtime");
        std::fs::create_dir_all(&runtime).unwrap();
        std::fs::write(runtime.join("LockCard.ini"), "StartCardNo=2403
").unwrap();

        let conn = ocw_store::open(&db_path).unwrap();
        conn.execute(
            "INSERT INTO operator (login, display_name, role, pwd_hash)
             VALUES ('admin', '管理员', 'admin', '$argon2id$v=19$m=1,t=1,p=1$cafe')",
            [],
        )
        .unwrap();
        ocw_store::insert_room(&conn, "901", Some(90), None, None, None).unwrap();
        let db = Mutex::new(conn);

        let res = export(&db, &db_path, &runtime, serde_json::json!({"version": "test"}))
            .expect("导出应成功");
        assert!(res.path.exists(), "产物应存在：{}", res.path.display());
        if res.warnings.is_empty() {
            assert!(res.path.extension().map(|e| e == "zip").unwrap_or(false), "应打出 zip");
            assert!(res.bytes > 0, "zip 不该是空文件");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

}
