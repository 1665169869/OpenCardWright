//! 模拟设备必须像真机一样**推进卡号计数器**。
//!
//! 这条用真实的 `OCWCardBuild.exe`（随资源构建）跑两遍，确认：
//! 1. 第二次的卡号 ≠ 第一次；2. 卡号写进了字节；3. `CardNo.state` 真的前进了。
//!
//! 之前 `Device::Sim` 是回放固定样本（永远 0x2404），这条测试就是防它退化。

use ocw_device::SimDevice;
use std::fs;
use std::path::Path;

fn cardbuild_dir() -> Option<std::path::PathBuf> {
    // 构建产物：src-tauri/resources/sdk/OCWCardBuild.exe
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("resources")
        .join("sdk");
    if dir.join("OCWCardBuild.exe").is_file() && dir.join("PMSLockSDK.dll").is_file() {
        Some(dir)
    } else {
        None
    }
}

#[test]
fn sim_issue_advances_card_number() {
    let Some(src) = cardbuild_dir() else {
        eprintln!("跳过：没有 OCWCardBuild.exe（先跑 npm run build:sdk）");
        return;
    };

    let dir = std::env::temp_dir().join(format!("ocw-sim-counter-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("建临时目录");

    // 把构包器和它的依赖拷到临时目录，避免动真实运行目录里的 CardNo.state
    for f in ["OCWCardBuild.exe", "PMSLockSDK.dll"] {
        fs::copy(src.join(f), dir.join(f)).expect("拷贝构包器");
    }
    fs::write(dir.join("CardNo.state"), "240B").expect("写初始计数器");

    let sim = SimDevice::at(&dir);

    let first = sim
        .issue_guest(0, "201", 1, 0)
        .expect("模拟设备应提供构包器")
        .expect("第一张卡应构出来");
    let second = sim
        .issue_guest(0, "201", 1, 0)
        .expect("模拟设备应提供构包器")
        .expect("第二张卡应构出来");

    assert_eq!(first.card_no, Some(0x240C), "应从 240B 前进到 240C");
    assert_eq!(second.card_no, Some(0x240D), "第二张必须再前进");
    assert_ne!(
        first.raw_hex, second.raw_hex,
        "卡号变了，原始字节也必须跟着变"
    );
    assert!(first.raw_is_28_bytes());
    // 卡号低/高字节应出现在原始数据里（0x240C → 0C 24）
    assert!(
        first.raw_hex.contains("0C24"),
        "原始数据里应含卡号字节 0C24，实际 {}",
        first.raw_hex
    );

    let state = fs::read_to_string(dir.join("CardNo.state")).expect("读计数器");
    assert_eq!(state.trim(), "240D", "计数器文件应被推进");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn sim_issue_honours_room_index_and_duration() {
    let Some(src) = cardbuild_dir() else {
        eprintln!("跳过：没有 OCWCardBuild.exe");
        return;
    };
    let dir = std::env::temp_dir().join(format!("ocw-sim-room-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    for f in ["OCWCardBuild.exe", "PMSLockSDK.dll"] {
        fs::copy(src.join(f), dir.join(f)).unwrap();
    }
    fs::write(dir.join("CardNo.state"), "240B").unwrap();

    let sim = SimDevice::at(&dir);
    let day = sim.issue_guest(2, "219", 3, 0).unwrap().unwrap();
    let hour = sim.issue_guest(2, "219", 0, 3).unwrap().unwrap();

    assert_eq!(day.days, 3);
    assert_eq!(day.hours, 0);
    assert_eq!(hour.hours, 3);
    assert_eq!(hour.days, 0);
    // 序号 2 应写进页1：低 4 位 << 4 | 高位
    assert!(day.raw_hex.contains("5BEC21"), "页1 应带序号 2，实际 {}", day.raw_hex);

    let _ = fs::remove_dir_all(&dir);
}
