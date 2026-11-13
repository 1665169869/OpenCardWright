//! 全局状态与设备门面。
//!
//! # 门面只有两个分支，且行为一致
//!
//! - [`Device::Sidecar`]：真机。64 位主程序加载不了 32 位驱动，所以门锁卡操作
//!   全部交给常驻的 `rc-device-sidecar.exe`（NDJSON 协议）。
//! - [`Device::Sim`]：没有读卡器时（`OCW_SIM=1` 或 sidecar 缺失）用进程内模拟后端。
//!
//! 两者返回同一套 DTO 与错误，命令层不需要知道当前是哪一个。
//! 以前这里是 `Cli(PMSLockCard.exe) | Sim(OCWCardBuild.exe)`，两条路径各有各的字节规则；
//! 现在字节规则只在 `rc-device` 里，模拟与真机写出来的卡逐字节相同。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ocw_device::{
    CardSnapshot, CheckoutHourSpec, DeviceError, GuestDaySpec, GuestHourSpec, InstallSpec,
    MasterSpec, PlainCardSpec, PortProbe, RawSpec, SelfTest, SidecarDevice, SimDevice, TimeSyncSpec,
};
use ocw_store::DbConnection as Connection;

use crate::auth::Session;

/// 设备门面。内部用 `Arc`，复制成本与路径句柄相当。
#[derive(Clone)]
pub enum Device {
    Sidecar(Arc<SidecarDevice>),
    Sim(Arc<SimDevice>),
}

impl Device {
    /// 当前是否为模拟后端（持卡退房时对擦卡失败的处理不同）。
    pub fn is_sim(&self) -> bool {
        matches!(self, Self::Sim(_))
    }

    /// 后端名字，进日志与诊断包。
    pub fn backend(&self) -> &'static str {
        match self {
            Self::Sidecar(_) => "sidecar",
            Self::Sim(_) => "sim",
        }
    }

    pub fn read_card(&self) -> Result<CardSnapshot, DeviceError> {
        match self {
            Self::Sidecar(d) => d.read_card(),
            Self::Sim(d) => d.read_card(),
        }
    }

    pub fn erase_card(&self) -> Result<CardSnapshot, DeviceError> {
        match self {
            Self::Sidecar(d) => d.erase_card(),
            Self::Sim(d) => d.erase_card(),
        }
    }

    pub fn write_guest_day(&self, spec: &GuestDaySpec) -> Result<CardSnapshot, DeviceError> {
        match self {
            Self::Sidecar(d) => d.write_guest_day(spec),
            Self::Sim(d) => d.write_guest_day(spec),
        }
    }

    pub fn write_guest_hour(&self, spec: &GuestHourSpec) -> Result<CardSnapshot, DeviceError> {
        match self {
            Self::Sidecar(d) => d.write_guest_hour(spec),
            Self::Sim(d) => d.write_guest_hour(spec),
        }
    }

    pub fn write_master(&self, spec: &MasterSpec) -> Result<CardSnapshot, DeviceError> {
        match self {
            Self::Sidecar(d) => d.write_master(spec),
            Self::Sim(d) => d.write_master(spec),
        }
    }

    pub fn write_install(&self, spec: &InstallSpec) -> Result<CardSnapshot, DeviceError> {
        match self {
            Self::Sidecar(d) => d.write_install(spec),
            Self::Sim(d) => d.write_install(spec),
        }
    }

    pub fn write_reset(&self, spec: &PlainCardSpec) -> Result<CardSnapshot, DeviceError> {
        match self {
            Self::Sidecar(d) => d.write_reset(spec),
            Self::Sim(d) => d.write_reset(spec),
        }
    }

    pub fn write_clear(&self, spec: &PlainCardSpec) -> Result<CardSnapshot, DeviceError> {
        match self {
            Self::Sidecar(d) => d.write_clear(spec),
            Self::Sim(d) => d.write_clear(spec),
        }
    }

    pub fn write_checkout_hour(&self, spec: &CheckoutHourSpec) -> Result<CardSnapshot, DeviceError> {
        match self {
            Self::Sidecar(d) => d.write_checkout_hour(spec),
            Self::Sim(d) => d.write_checkout_hour(spec),
        }
    }

    pub fn write_timesync(&self, spec: &TimeSyncSpec) -> Result<CardSnapshot, DeviceError> {
        match self {
            Self::Sidecar(d) => d.write_timesync(spec),
            Self::Sim(d) => d.write_timesync(spec),
        }
    }

    /// 原始直写（破坏性，仅供逆向/排障）。
    pub fn write_raw(&self, spec: &RawSpec) -> Result<CardSnapshot, DeviceError> {
        match self {
            Self::Sidecar(d) => d.write_raw(spec),
            Self::Sim(d) => d.write_raw(spec),
        }
    }

    pub fn selftest(&self) -> Result<SelfTest, DeviceError> {
        match self {
            Self::Sidecar(d) => d.selftest(),
            Self::Sim(d) => d.selftest(),
        }
    }

    pub fn probe_ports(&self) -> Result<Vec<PortProbe>, DeviceError> {
        match self {
            Self::Sidecar(d) => d.probe_ports(),
            Self::Sim(d) => d.probe_ports(),
        }
    }

    pub fn device_info(&self) -> Result<ocw_device::DeviceInfo, DeviceError> {
        match self {
            Self::Sidecar(d) => d.device_info(),
            Self::Sim(d) => d.device_info(),
        }
    }

    pub fn set_port(&self, port: u8) -> Result<(), DeviceError> {
        match self {
            Self::Sidecar(d) => d.set_port(port),
            Self::Sim(d) => d.set_port(port),
        }
    }

    /// 退出前关掉 sidecar（模拟后端是空操作）。
    pub fn shutdown(&self) {
        if let Self::Sidecar(d) = self {
            d.shutdown();
        }
    }
}

/// 按运行时目录构造设备。
///
/// 优先级：`OCW_SIM=1` 或 `sim` 参数 → 进程内模拟；找不到 sidecar → 也退化为模拟
/// （界面会明确提示「模拟设备」，避免用户以为真的在写卡）。
pub fn build_device(runtime_dir: &Path, port: u8, sim: bool) -> Device {
    let exe = runtime_dir.join("rc-device-sidecar.exe");
    if sim || !exe.is_file() {
        if !sim {
            eprintln!(
                "[ocw] 未找到 {}，退化为模拟设备（先跑 pnpm build:device）",
                exe.display()
            );
        }
        let device = SimDevice::new();
        let _ = device.set_port(port);
        return Device::Sim(Arc::new(device));
    }

    let driver_dir = runtime_dir.join("drivers").join("LC2000");
    let device = SidecarDevice::new(exe, Some(driver_dir));
    // 端口设置失败不阻塞启动：真机上「没有读卡器」是常见状态，界面会提示去探测
    if let Err(e) = device.set_port(port) {
        eprintln!("[ocw] 设置串口 {port} 失败（可稍后在设置页重选）：{e}");
    }
    Device::Sidecar(Arc::new(device))
}

/// 当前设备 + 串口。串口可变，所以放在锁里。
pub struct DeviceSlot {
    pub device: Device,
    pub port: u8,
}

pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub db_path: PathBuf,
    pub slot: Arc<Mutex<DeviceSlot>>,
    pub runtime_dir: PathBuf,
    pub driver_sha256: Option<String>,
    /// 硬件层来源：`env` / `resource` / `dev`
    pub device_source: Option<String>,
    pub dev: bool,
    /// 身份证读卡器。与门锁卡读写器是**两个互不相关的设备**：任一故障都不影响另一个。
    ///
    /// 它走本机 `ws://127.0.0.1:9001` 服务，不需要 32 位驱动，所以留在主进程内，
    /// 不经过 sidecar。
    pub id_card: Arc<dyn rc_device::IdCardReader>,
    /// 当前会话。只存内存，重启即失效。
    pub session: Arc<Mutex<Option<Session>>>,
}

impl AppState {
    pub fn session(&self) -> Option<Session> {
        match self.session.lock() {
            Ok(s) => s.clone(),
            Err(_) => None,
        }
    }

    pub fn set_session(&self, value: Option<Session>) {
        if let Ok(mut s) = self.session.lock() {
            *s = value;
        }
    }

    /// 取一份设备副本。`Device` 内部是 `Arc`，复制成本可忽略。
    pub fn device(&self) -> Device {
        match self.slot.lock() {
            Ok(s) => s.device.clone(),
            Err(_) => Device::Sim(Arc::new(SimDevice::new())),
        }
    }

    /// 身份证读卡器。与门锁卡设备彼此独立，故障不互相传染。
    pub fn id_card(&self) -> Arc<dyn rc_device::IdCardReader> {
        Arc::clone(&self.id_card)
    }

    pub fn port(&self) -> u8 {
        match self.slot.lock() {
            Ok(s) => s.port,
            Err(_) => 0,
        }
    }

    /// 切换串口：发一条 `set_port` 给硬件层，成功后才更新本地记录。
    pub fn set_port(&self, device: &Device, port: u8) -> Result<(), String> {
        device
            .set_port(port)
            .map_err(|e| format!("{}（{}）", e, e.hint()))?;
        let mut slot = self.slot.lock().map_err(|_| "设备状态锁异常".to_string())?;
        slot.port = port;
        Ok(())
    }
}
