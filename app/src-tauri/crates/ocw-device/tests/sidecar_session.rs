//! 设备层端到端测试：真起 sidecar 进程（`--sim`），走 NDJSON 协议。
//!
//! 这些测试验证的是**会话层**：进程能起、请求能配对、超时会杀进程并重建、
//! 错误负载能原样透传。卡字节的正确性由 `rc-device` 的 46 项黄金向量负责，
//! 这里只断言「写进去的字节能原样读回来」。
//!
//! # 前置
//!
//! 需要 `src-tauri/resources/device/rc-device-sidecar.exe`（`pnpm build:device` 产出）。
//! 找不到就跳过——CI 会先构建，本地没构建时不应该红一片。

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::time::Duration;

use ocw_device::{
    CardKind, CheckoutHourSpec, DeviceError, GuestDaySpec, GuestHourSpec, MasterSpec, PlainCardSpec,
    RawSpec, SidecarDevice, SimDevice, TimeSyncSpec,
};
use rc_device::{CardDuration, CardIssuedAt};

fn sidecar_exe() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../resources/device/rc-device-sidecar.exe")
}

/// 取 sidecar 路径；没有就跳过整个用例（不是失败）。
macro_rules! require_sidecar {
    () => {{
        let exe = sidecar_exe();
        if !exe.is_file() {
            eprintln!("跳过：没有 rc-device-sidecar.exe（先跑 pnpm build:device）");
            return;
        }
        exe
    }};
}

fn issued_at() -> CardIssuedAt {
    CardIssuedAt {
        year: 2026,
        month: 11,
        day: 8,
        hour: 14,
    }
}

fn guest_day(index: u16, seq: u8) -> GuestDaySpec {
    GuestDaySpec {
        room_index: index,
        days: 1,
        card_no: rc_device::timestamp_card_no(&issued_at(), seq).unwrap(),
        lost: false,
        issued_at: issued_at(),
    }
}

/// 一次完整会话：读空白卡 → 写日租卡 → 原样读回 → 擦卡。
#[test]
fn sim_session_round_trips_cards() {
    let exe = require_sidecar!();
    let dev = SidecarDevice::sim(exe);

    let blank = dev.read_card().expect("模拟卡槽默认应有空白卡");
    assert_eq!(blank.kind, CardKind::Blank);

    let written = dev.write_guest_day(&guest_day(15, 0)).expect("写卡应当成功");
    assert_eq!(written.kind, CardKind::GuestDay);
    assert_eq!(written.room_index, 15);
    assert_eq!(written.days, 1);

    let back = dev.read_card().expect("读卡应当成功");
    assert_eq!(back.raw_hex, written.raw_hex, "写进去的字节必须原样读回来");

    let erased = dev.erase_card().expect("擦卡应当成功");
    assert_eq!(erased.kind, CardKind::Blank);
    assert!(erased.raw_hex.chars().all(|c| c == '0'));

    dev.shutdown();
}

/// 46 项自检在 sidecar 进程里也必须全绿。
#[test]
fn selftest_over_sidecar_is_46_green() {
    let exe = require_sidecar!();
    let dev = SidecarDevice::sim(exe);

    let report = dev.selftest().expect("自检应当成功");
    assert_eq!(report.passed, 46);
    assert_eq!(report.failed, 0);
    assert!(report.is_ok());

    dev.shutdown();
}

/// 没放卡时，硬件层的 `no_card` 负载要原样透传到产品侧：
/// 码 5、分类 `no_card`、有建议、且不算设备故障。
#[test]
fn no_card_error_passes_through_payload() {
    let exe = require_sidecar!();
    let dev = SidecarDevice::sim_with_initial_card(exe, "none");

    let err = dev.read_card().expect_err("没放卡必须失败");
    assert_eq!(err.exit_code(), 5);
    assert_eq!(err.kind(), "no_card");
    assert!(!err.is_device_fault());
    assert!(!err.hint().trim().is_empty(), "必须带现场建议");

    dev.shutdown();
}

/// 超时会杀掉 sidecar，并且下一次调用能自动重新拉起（会话重建）。
///
/// 这条覆盖了「崩溃后下次调用重启」的实质：超时与崩溃走的是同一条清理路径。
#[test]
fn timeout_kills_session_and_next_call_restarts() {
    let exe = require_sidecar!();
    // 0 超时 = 请求发出后立刻放弃等待：内存模拟也不可能在这一瞬间回包。
    // open 走 15s 上限，所以会话本身能建起来，超时只落在随后的读卡上。
    let dev = SidecarDevice::sim(exe).with_card_timeout(Duration::ZERO);

    let err = dev.read_card().expect_err("0 超时应当失败");
    assert!(matches!(err, DeviceError::Timeout(_)), "{err:?}");

    // 会话已被丢弃；device_info 走 15s 上限，能重新拉起进程
    dev.open().expect("超时后应能重新拉起 sidecar");
    let info = dev.device_info().expect("会话应已重建");
    assert_eq!(info.backend, rc_device::DeviceBackend::Mock);

    dev.shutdown();
}

/// 一次会话里把六种写卡操作都走一遍：确保线上的 `spec` 形状与产品类型对得上。
///
/// 这是「协议编解码」在产品侧的守门测试：字段名/结构漂移会在这里立刻红。
#[test]
fn every_write_operation_has_a_matching_wire_shape() {
    let exe = require_sidecar!();
    let dev = SidecarDevice::sim(exe);

    let card_no = rc_device::timestamp_card_no(&issued_at(), 5).unwrap();

    let master = dev
        .write_master(&MasterSpec {
            duration: CardDuration::Hours(2),
            card_no,
            issued_at: issued_at(),
        })
        .expect("总控卡应当能写");
    assert_eq!(master.kind, CardKind::Master);

    let install = dev
        .write_install(&rc_device::InstallSpec { room_index: 39 })
        .expect("安装卡应当能写");
    assert_eq!(install.kind, CardKind::Install);
    assert_eq!(install.room_index, 39);

    let reset = dev
        .write_reset(&PlainCardSpec {
            card_no,
            issued_at: issued_at(),
        })
        .expect("复位卡应当能写");
    assert_eq!(reset.kind, CardKind::Reset);

    let clear = dev
        .write_clear(&PlainCardSpec {
            card_no,
            issued_at: issued_at(),
        })
        .expect("清除卡应当能写");
    assert_eq!(clear.kind, CardKind::Clear);

    let checkout = dev
        .write_checkout_hour(&CheckoutHourSpec {
            hour: 12,
            card_no,
            issued_at: issued_at(),
        })
        .expect("退房钟点卡应当能写");
    assert_eq!(checkout.kind, CardKind::CheckoutHour);
    assert_eq!(checkout.hours, 0);

    let timesync = dev
        .write_timesync(&TimeSyncSpec {
            hour: 10,
            minute: 48,
            card_no,
            issued_at: issued_at(),
        })
        .expect("校时卡应当能写");
    assert_eq!(timesync.kind, CardKind::TimeSync);

    dev.shutdown();
}

/// 钟点卡：时长进 `hours`，且不能出现「天数也被填上」的错觉。
#[test]
fn guest_hour_card_is_written_and_read_back() {
    let exe = require_sidecar!();
    let dev = SidecarDevice::sim(exe);

    let spec = GuestHourSpec {
        room_index: 6,
        hours: 3,
        card_no: rc_device::timestamp_card_no(&issued_at(), 6).unwrap(),
        lost: false,
        issued_at: issued_at(),
    };
    let written = dev.write_guest_hour(&spec).expect("钟点卡应当能写");
    assert_eq!(written.kind, CardKind::GuestHour);
    assert_eq!(written.hours, 3);
    assert_eq!(written.days, 0);

    dev.shutdown();
}

/// 原始直写在生产链路上也能走通：写进去的 8 字节就是卡上的 8 字节。
#[test]
fn raw_write_reaches_the_card_unchanged() {
    let exe = require_sidecar!();
    let dev = SidecarDevice::sim(exe);

    let spec = RawSpec {
        p1: [0x5B, 0xEC, 0xF0, 0x00],
        p2: [0x81, 0x04, 0x24, 0xAB],
    };
    let written = dev.write_raw(&spec).expect("原始直写应当成功");
    assert_eq!(written.kind, CardKind::GuestDay);
    assert_eq!(written.room_index, 15);
    assert!(written.raw_hex.starts_with("5BECF000810424AB"));

    dev.shutdown();
}

/// 进程内模拟后端与 sidecar 模拟后端必须给出同一个结果。
///
/// 两条模拟路径各写各的实现时最容易漂移，这里用同一张卡把它们钉在一起。
#[test]
fn in_process_sim_matches_sidecar_sim() {
    let exe = require_sidecar!();
    let sidecar = SidecarDevice::sim(exe);
    let local = SimDevice::new();

    let spec = guest_day(39, 1);
    let a = sidecar.write_guest_day(&spec).expect("sidecar 写卡应当成功");
    let b = local.write_guest_day(&spec).expect("进程内写卡应当成功");

    assert_eq!(a.raw_hex, b.raw_hex, "两种模拟必须写出同样的字节");
    assert_eq!(a.kind, b.kind);
    assert_eq!(a.room_index, b.room_index);

    sidecar.shutdown();
}

/// 进程内模拟后端自己也要跑一遍 46 项自检 —— 模拟可信的唯一依据。
#[test]
fn in_process_sim_selftest_is_46_green() {
    let report = SimDevice::new().selftest().expect("自检应当成功");
    assert_eq!(report.passed, 46);
    assert_eq!(report.failed, 0);
    assert!(report.is_ok());
}

/// DTO 的 JSON 形状就是前端契约：camelCase、字段名固定。
#[test]
fn dto_serializes_with_frontend_field_names() {
    let dev = SimDevice::new();
    let card = dev.write_guest_day(&guest_day(15, 2)).expect("写卡应当成功");
    let json = serde_json::to_value(&card).expect("DTO 必须可序列化");

    for key in [
        "kind",
        "kindLabel",
        "room",
        "roomIndex",
        "days",
        "hours",
        "cardNo",
        "isLost",
        "issueDay",
        "issueHour",
        "issueSeq",
        "cardYm",
        "rawHex",
    ] {
        assert!(json.get(key).is_some(), "缺少字段 {key}");
    }
    assert_eq!(json["kind"], "guestDay");
    assert_eq!(json["roomIndex"], 15);
    assert_eq!(json["room"], serde_json::Value::Null, "房号由命令层补，DTO 恒为空");
}
