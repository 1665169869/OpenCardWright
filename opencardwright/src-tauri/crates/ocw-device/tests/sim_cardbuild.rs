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

/// 准备一个只装了构包器的临时目录（绝不碰仓库/运行目录里的 CardNo.state）。
fn temp_sim(tag: &str, start: &str) -> Option<std::path::PathBuf> {
    let src = cardbuild_dir()?;
    let dir = std::env::temp_dir().join(format!("ocw-sim-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).ok()?;
    for f in ["OCWCardBuild.exe", "PMSLockSDK.dll"] {
        fs::copy(src.join(f), dir.join(f)).ok()?;
    }
    fs::write(dir.join("CardNo.state"), start).ok()?;
    Some(dir)
}

/// 六类管理卡的原始字节必须两两不同（以前是"发什么卡都一样"）。
#[test]
fn admin_cards_are_pairwise_different() {
    let Some(dir) = temp_sim("admin", "240B") else {
        eprintln!("跳过：没有 OCWCardBuild.exe（先跑 npm run build:sdk）");
        return;
    };
    let sim = SimDevice::at(&dir);

    let cases: Vec<(&str, Vec<&str>)> = vec![
        ("总控卡", vec!["--type", "master", "--days", "1"]),
        ("复位卡", vec!["--type", "reset"]),
        ("清除卡", vec!["--type", "clear"]),
        ("安装卡", vec!["--type", "install"]),
        ("退房钟点", vec!["--type", "checkout", "--hour", "12"]),
        ("校时卡", vec!["--type", "timesync", "--hour", "10", "--minute", "48"]),
    ];

    let mut raws = Vec::new();
    for (name, args) in &cases {
        let card = sim
            .issue_admin(args, Some(15))
            .expect("模拟设备应提供构包器")
            .unwrap_or_else(|e| panic!("{name} 构包失败：{e}"));
        assert!(card.raw_is_28_bytes(), "{name} 不是 28 字节");
        raws.push((name.to_string(), card.raw_hex.clone()));
    }

    for i in 0..raws.len() {
        for j in (i + 1)..raws.len() {
            assert_ne!(
                raws[i].1, raws[j].1,
                "{} 与 {} 的字节相同，卡型没有真正区分",
                raws[i].0, raws[j].0
            );
        }
    }

    let _ = fs::remove_dir_all(&dir);
}

/// 安装卡不含卡号，**不该**消耗 CardNo.state（真机同样不吃卡号）。
#[test]
fn install_does_not_advance_counter() {
    let Some(dir) = temp_sim("install", "240B") else {
        eprintln!("跳过：没有 OCWCardBuild.exe");
        return;
    };
    let sim = SimDevice::at(&dir);

    let a = sim.issue_admin(&["--type", "install"], Some(15)).unwrap().unwrap();
    let b = sim.issue_admin(&["--type", "install"], Some(15)).unwrap().unwrap();
    let _ = (a, b);

    let state = fs::read_to_string(dir.join("CardNo.state")).unwrap();
    assert_eq!(state.trim(), "240B", "安装卡不该推进卡号计数器");

    let _ = fs::remove_dir_all(&dir);
}

/// 关键字节对齐已知向量（期望值抄 `docs/抓包样本与验证记录.md`，不要自己编）。
#[test]
fn admin_bytes_match_known_vectors() {
    let Some(dir) = temp_sim("vectors", "240B") else {
        eprintln!("跳过：没有 OCWCardBuild.exe");
        return;
    };
    let sim = SimDevice::at(&dir);
    let hex = |args: Vec<&str>, idx: Option<u8>| -> String {
        sim.issue_admin(&args, idx).unwrap().unwrap().raw_hex
    };

    // 总控卡 1 天：页1 = 5BECF0FF，页2 首字节 = 81
    let master = hex(vec!["--type", "master", "--days", "1"], None);
    assert!(master.starts_with("5BECF0FF"), "总控卡页1 应为 5BECF0FF，实际 {master}");
    assert_eq!(&master[8..10], "81", "总控 1 天时长码应为 81");

    // 总控卡 1095 天：月码段 0xE3
    let master_long = hex(vec!["--type", "master", "--days", "1095"], None);
    assert_eq!(&master_long[8..10], "E3", "1095 天应落在 0xC0|35 段（E3）");

    // 复位卡 / 清除卡页1 固定
    assert!(hex(vec!["--type", "reset"], None).starts_with("5BEC0A00"));
    assert!(hex(vec!["--type", "clear"], None).starts_with("5BEC9BE5"));

    // 安装卡：页2 固定 46F20001，且不带卡号
    let install = hex(vec!["--type", "install"], Some(15));
    assert_eq!(&install[8..16], "46F20001", "安装卡页2 应固定为 46F20001");

    // 校时卡 10:48 → v = 16*(648)+953 = 0x2C39，低字节在前
    let ts = hex(vec!["--type", "timesync", "--hour", "10", "--minute", "48"], None);
    assert!(ts.starts_with("5BEC392C"), "校时卡页1 应为 5BEC392C，实际 {ts}");

    let _ = fs::remove_dir_all(&dir);
}
