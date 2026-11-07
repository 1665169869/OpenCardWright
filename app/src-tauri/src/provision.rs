//! 运行时准备：把安装目录里的 `resources/sdk` 复制到 `%ProgramData%` 下的一份可写副本。
//!
//! 为什么不直接跑安装目录的 exe：`CardNo.state` 要写盘，而 `Program Files` 对普通用户只读。

use std::path::{Path, PathBuf};
use tauri::Manager;

pub struct RuntimeInfo {
    pub dir: PathBuf,
    pub driver_sha256: Option<String>,
    /// SDK 是从哪来的：`env` / `resource` / `dev` / 无
    pub source: Option<String>,
    /// 是否开发构建（dev 下会额外找源码树的 resources/sdk）
    pub dev: bool,
}

pub fn data_root() -> PathBuf {
    std::env::var_os("PROGRAMDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("OpenCardWright")
}

/// 按优先级列出可能的 SDK 目录。
///
/// dev 模式（`npm run tauri dev`）下资源目录里没有 `resources/sdk`，
/// 但 `npm run build:sdk` 会把它放在 `src-tauri/resources/sdk`，这里回退过去。
/// 另外允许 `OCW_SDK_DIR` 显式指定，方便接真机调试。
fn candidate_sources(app: &tauri::AppHandle) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = Vec::new();

    if let Some(p) = std::env::var_os("OCW_SDK_DIR") {
        out.push(("env".to_string(), PathBuf::from(p)));
    }
    if let Ok(res) = app.path().resource_dir() {
        out.push(("resource".to_string(), res.join("resources").join("sdk")));
    }
    #[cfg(debug_assertions)]
    {
        out.push((
            "dev".to_string(),
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources")
                .join("sdk"),
        ));
    }

    out
}

/// 确保运行时副本存在且与来源一致。失败不 panic——退化为模拟设备，界面会明确提示。
pub fn ensure_runtime(app: &tauri::AppHandle, start_card_no: &str) -> RuntimeInfo {
    let dir = data_root().join("runtime");
    let dev = cfg!(debug_assertions);
    let unavailable = || RuntimeInfo {
        dir: dir.clone(),
        driver_sha256: None,
        source: None,
        dev,
    };

    let picked = candidate_sources(app)
        .into_iter()
        .find(|(_, p)| p.join("PMSLockCard.exe").is_file());

    let (source, src) = match picked {
        Some(v) => v,
        None => {
            eprintln!(
                "[ocw] 未找到内置 SDK（试过 OCW_SDK_DIR / 资源目录 / 开发目录）。先跑 `npm run build:sdk`。"
            );
            return unavailable();
        }
    };
    eprintln!("[ocw] SDK 来源: {source} -> {}", src.display());

    if !dir.join("PMSLockCard.exe").is_file() || manifest_differs(&src, &dir) {
        if let Err(e) = copy_tree(&src, &dir) {
            eprintln!("[ocw] 复制运行时失败: {e}");
            return unavailable();
        }
    }

    let ini = dir.join("LockCard.ini");
    if ini.is_file() {
        if let Err(e) = apply_start_card_no(&ini, start_card_no) {
            eprintln!("[ocw] 写入起始卡号失败: {e}");
        }
    }

    let driver_sha256 = manifest_driver_sha(&dir).or_else(|| manifest_driver_sha(&src));

    RuntimeInfo {
        dir,
        driver_sha256,
        source: Some(source),
        dev,
    }
}

fn manifest_differs(src: &Path, dst: &Path) -> bool {
    let a = std::fs::read(src.join("manifest.json"));
    let b = std::fs::read(dst.join("manifest.json"));
    match (a, b) {
        (Ok(a), Ok(b)) => a != b,
        _ => true,
    }
}

fn manifest_driver_sha(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("manifest.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    v.get("driverSha256")?.as_str().map(|s| s.to_string())
}

/// 递归复制。**跳过 `CardNo.state`**——那是现场数据，升级时不能被安装包覆盖。
fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy().to_string();
        if name_str == "CardNo.state" || name_str == "CardNo.state.tmp" {
            continue;
        }

        let from = entry.path();
        let to = dst.join(&name);
        if entry.file_type()?.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// 把 `LockCard.ini` 的 `StartCardNo` 改成用户确认过的起始卡号。
///
/// 这是"起始卡号保护"的落地点：安装包不带 `CardNo.state`，
/// 首次发卡的水位由这里决定，避免把别人的现场水位带进来。
fn apply_start_card_no(ini: &Path, value_hex: &str) -> std::io::Result<()> {
    let value = value_hex.trim().trim_start_matches("0x").trim_start_matches("0X").to_uppercase();
    if value.is_empty() || !value.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("起始卡号不是合法的十六进制: {value_hex}"),
        ));
    }

    let text = std::fs::read_to_string(ini)?;
    let mut replaced = false;
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        if t.to_ascii_lowercase().starts_with("startcardno") && t.contains('=') {
            out.push(format!("StartCardNo={value}"));
            replaced = true;
        } else {
            out.push(line.to_string());
        }
    }
    if !replaced {
        out.push(format!("StartCardNo={value}"));
    }

    let new_text = out.join("\r\n") + "\r\n";
    if new_text != text {
        std::fs::write(ini, new_text)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_card_no_is_rewritten_in_place() {
        let dir = std::env::temp_dir().join(format!("ocw-ini-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ini = dir.join("LockCard.ini");
        std::fs::write(&ini, "[Card]\r\n; 注释\r\nStartCardNo=2403\r\n").unwrap();

        apply_start_card_no(&ini, "0x2B90").unwrap();
        let text = std::fs::read_to_string(&ini).unwrap();
        assert!(text.contains("StartCardNo=2B90"));
        assert!(!text.contains("2403"));
        assert!(text.contains("; 注释"), "注释必须保留");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_start_card_no_is_rejected() {
        let dir = std::env::temp_dir().join(format!("ocw-ini-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ini = dir.join("LockCard.ini");
        std::fs::write(&ini, "StartCardNo=2403\r\n").unwrap();
        assert!(apply_start_card_no(&ini, "zz").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
