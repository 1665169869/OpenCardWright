//! 对着**真实**的 `PMSLockCard.exe selftest` 输出跑一遍解析器。
//!
//! 这是唯一不需要读卡器就能端到端验证"设备通道契约"的测试：
//! 它同时证明（a）产物真的能跑，（b）27 条黄金向量全绿，（c）我们把它的输出读对了。
//!
//! 没构建 SDK 时自动跳过（比如只做 `cargo test -p ocw-device` 的裸环境）。

use std::path::PathBuf;
use std::process::Command;

fn sdk_exe() -> PathBuf {
    // CARGO_MANIFEST_DIR = <repo>/opencardwright/src-tauri/crates/ocw-device
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../src-tauri/resources/sdk/PMSLockCard.exe")
}

#[test]
fn real_selftest_is_all_green_and_parses() {
    let exe = sdk_exe();
    if !exe.is_file() {
        eprintln!("跳过：{} 不存在（先跑 npm run build:sdk）", exe.display());
        return;
    }

    let output = Command::new(&exe)
        .arg("selftest")
        .output()
        .expect("应能启动 selftest");

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let parsed = ocw_device::parse_selftest(&stdout)
        .unwrap_or_else(|e| panic!("无法解析 selftest 输出: {e}\n--- 原文 ---\n{stdout}"));

    assert_eq!(parsed.passed, 27, "应跑满 27 条向量\n{stdout}");
    assert_eq!(parsed.failed, 0, "不允许有失败向量\n{stdout}");
    assert!(parsed.is_ok());
    assert!(output.status.success(), "selftest 退出码非 0");
}
