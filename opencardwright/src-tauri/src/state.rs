//! 全局状态与设备后端抽象。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ocw_device::{parse, CardSnapshot, CliDevice, DeviceError, PortProbe, SelfTest, SimDevice};
use ocw_store::DbConnection as Connection;

use crate::auth::Session;

/// 设备后端。真实机器走 `Cli`（子进程），CI/无驱动环境走 `Sim`（回放样本）。
#[derive(Clone)]
pub enum Device {
    Cli(CliDevice),
    Sim,
}

impl Device {
    pub fn is_sim(&self) -> bool {
        matches!(self, Self::Sim)
    }

    pub fn read_card(&self) -> Result<CardSnapshot, DeviceError> {
        match self {
            Self::Cli(d) => d.read_card(),
            Self::Sim => SimDevice::new().read_card(),
        }
    }

    pub fn issue(&self, args: &[&str]) -> Result<CardSnapshot, DeviceError> {
        match self {
            Self::Cli(d) => d.issue(args),
            Self::Sim => {
                let sample = if args.contains(&"install") {
                    ocw_device::sim::INSTALL_JSON
                } else {
                    ocw_device::sim::GUEST_DAY_JSON
                };
                parse::parse_card(sample, 0)
            }
        }
    }

    pub fn selftest(&self) -> Result<SelfTest, DeviceError> {
        match self {
            Self::Cli(d) => d.selftest(),
            Self::Sim => SimDevice::new().selftest(),
        }
    }

    pub fn probe_ports(&self) -> Result<Vec<PortProbe>, DeviceError> {
        match self {
            Self::Cli(d) => d.probe_ports(),
            Self::Sim => SimDevice::new().probe_ports(),
        }
    }
}

/// 按运行时目录构造设备。`OCW_SIM=1` 或 SDK 不存在时退化为模拟设备。
pub fn build_device(runtime_dir: &Path, port: u8) -> Device {
    if std::env::var_os("OCW_SIM").is_some() {
        return Device::Sim;
    }
    let exe = runtime_dir.join("PMSLockCard.exe");
    if !exe.is_file() {
        return Device::Sim;
    }
    Device::Cli(CliDevice::new(
        exe,
        runtime_dir.join("LockCard.ini"),
        runtime_dir.join("IOServer").join("LC2000"),
        port,
    ))
}

/// 当前设备 + 串口。串口可变，所以放在锁里；切换端口时整体重建。
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
    pub sdk_source: Option<String>,
    pub dev: bool,
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

    /// 取一份设备副本。`Device` 是轻量句柄（路径 + 端口），复制成本可忽略。
    pub fn device(&self) -> Device {
        match self.slot.lock() {
            Ok(s) => s.device.clone(),
            Err(_) => Device::Sim,
        }
    }

    pub fn port(&self) -> u8 {
        match self.slot.lock() {
            Ok(s) => s.port,
            Err(_) => 0,
        }
    }

    /// 切换串口：设备层本来就是"每次调用开关一次串口"，重建实例是安全的。
    pub fn rebuild_device(&self, port: u8) -> Result<(), String> {
        let device = build_device(&self.runtime_dir, port);
        let mut slot = self.slot.lock().map_err(|_| "设备状态锁异常".to_string())?;
        slot.device = device;
        slot.port = port;
        Ok(())
    }
}
