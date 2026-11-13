//! 运行时准备：把安装目录里的 `resources/device` 复制到 `%ProgramData%` 下的一份可写副本。
//!
//! 为什么不直接跑安装目录的 sidecar：
//!
//! - `CardNo.state` 要写盘，而 `Program Files` 对普通用户只读；
//! - sidecar 的 `stderr` 日志与临时文件也落在同目录；
//! - 升级安装包时不该去动正在被读卡器占用的 exe。
//!
//! 复制的是**整个硬件层目录**：`rc-device-sidecar.exe` + `drivers/LC2000/*`。
//! sidecar 按「exe 同目录的 `drivers/LC2000`」找驱动，所以复制后不需要再传 `--driver-dir`。
//!
//! # 为什么不再依赖 `manifest.json`
//!
//! 硬件层由开发者手动用 cargo 编出来（见 `AGENTS.md` §2），没有构建脚本去写清单。
//! 驱动指纹改为**打开驱动文件现算 SHA-256**：既不会因为清单过期而误报，
//! 也不需要为了改一行版本号重新生成一份 JSON。

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tauri::Manager;

pub struct RuntimeInfo {
    pub dir: PathBuf,
    pub driver_sha256: Option<String>,
    /// 硬件层是从哪来的：`env` / `resource` / `dev` / 无
    pub source: Option<String>,
    /// 是否开发构建（dev 下会额外找源码树的 resources/device）
    pub dev: bool,
}

/// 硬件层 sidecar 的文件名。运行时目录里有没有它，决定真机还是模拟。
pub const SIDECAR_EXE: &str = "rc-device-sidecar.exe";

/// 驱动在硬件层目录里的相对路径（sidecar 默认就从这里加载）。
const DRIVER_DLL_REL: &str = "drivers/LC2000/LC32RFRW.dll";

pub fn data_root() -> PathBuf {
    // 调试构建、DEV 安装包与正式版用**不同的数据目录**，避免互相改坏数据：
    //   cargo run / tauri dev / DEV 安装包 → %PROGRAMDATA%\OpenCardWright-dev\
    //   正式安装包                          → %PROGRAMDATA%\OpenCardWright\
    let dir_name = if channel() == "dev" {
        "OpenCardWright-dev"
    } else {
        "OpenCardWright"
    };
    std::env::var_os("PROGRAMDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(dir_name)
}

/// 发布通道：`dev` = 调试构建，或按 `OCW_CHANNEL=dev` 编出来的 DEV 安装包。
///
/// DEV 安装包是 **release 构建**，`cfg!(debug_assertions)` 区分不出来；
/// 所以 `build-release.ps1 -Channel dev` 会设置 `OCW_CHANNEL=dev` 再编译。
/// 本函数同时决定数据目录（`data_root`）与顶栏的「正式版 / DEV」标签。
pub fn channel() -> &'static str {
    if cfg!(debug_assertions) || option_env!("OCW_CHANNEL") == Some("dev") {
        "dev"
    } else {
        "release"
    }
}

/// 按优先级列出可能的硬件层目录。
///
/// dev 模式（`pnpm tauri dev`）下资源目录里没有 `resources/device`，
/// 所以回退到源码树的 `src-tauri/resources/device`（手动构建的产物就放那里）。
/// 另外允许 `OCW_DEVICE_DIR` 显式指定，方便接真机调试。
fn candidate_sources(app: &tauri::AppHandle) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = Vec::new();

    if let Some(p) = std::env::var_os("OCW_DEVICE_DIR") {
        out.push(("env".to_string(), PathBuf::from(p)));
    }
    if let Ok(res) = app.path().resource_dir() {
        out.push(("resource".to_string(), res.join("resources").join("device")));
    }
    #[cfg(debug_assertions)]
    {
        out.push((
            "dev".to_string(),
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources")
                .join("device"),
        ));
    }

    out
}

/// 确保运行时副本存在且与来源一致。失败不 panic——退化为模拟设备，界面会明确提示。
pub fn ensure_runtime(app: &tauri::AppHandle) -> RuntimeInfo {
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
        .find(|(_, p)| p.join(SIDECAR_EXE).is_file());

    let (source, src) = match picked {
        Some(v) => v,
        None => {
            eprintln!(
                "[ocw] 未找到硬件层（试过 OCW_DEVICE_DIR / 资源目录 / 开发目录）；\
                 先按 AGENTS.md §2 手动编 sidecar 并拷到 src-tauri/resources/device。"
            );
            return unavailable();
        }
    };
    eprintln!("[ocw] 硬件层来源: {source} -> {}", src.display());

    if !dir.join(SIDECAR_EXE).is_file() || payload_differs(&src, &dir) {
        if let Err(e) = copy_tree(&src, &dir) {
            eprintln!("[ocw] 复制运行时失败: {e}");
            return unavailable();
        }
    }

    // 不写 CardNo.state：它是现场数据，升级/首启都不应覆盖。
    // 新装环境由发卡时的时间戳卡号从当前时刻开始生成。

    let driver_sha256 = driver_sha(&dir).or_else(|| driver_sha(&src));

    RuntimeInfo {
        dir,
        driver_sha256,
        source: Some(source),
        dev,
    }
}

/// 源目录与运行时副本是否不一致。
///
/// 只比两个会随版本变的文件（sidecar 与驱动）——它们占了整个目录的 99%，
/// 逐字节比一次几十毫秒，比「每次启动都全量重拷」更划算。
fn payload_differs(src: &Path, dst: &Path) -> bool {
    for rel in [SIDECAR_EXE, DRIVER_DLL_REL] {
        let (a, b) = (src.join(rel), dst.join(rel));
        match (file_digest(&a), file_digest(&b)) {
            (Some(x), Some(y)) if x == y => {}
            _ => return true,
        }
    }
    false
}

fn file_digest(path: &Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok()?;
    Some(Sha256::digest(&bytes).to_vec())
}

/// 驱动文件的 SHA-256（大写十六进制）。读不到返回 `None`。
fn driver_sha(dir: &Path) -> Option<String> {
    let digest = file_digest(&dir.join(DRIVER_DLL_REL))?;
    Some(digest.iter().map(|b| format!("{b:02X}")).collect())
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
