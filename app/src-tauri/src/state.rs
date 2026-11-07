//! 全局状态与设备后端抽象。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ocw_device::{parse, CardSnapshot, CliDevice, DeviceError, PortProbe, SelfTest, SimDevice};
use ocw_store::DbConnection as Connection;

use crate::auth::Session;

/// 设备后端。真实机器走 `Cli`（子进程），CI/无驱动环境走 `Sim`。
///
/// `Sim` 也带运行时目录：发客卡时会调用 `OCWCardBuild.exe` 离线构包并推进 `CardNo.state`，
/// 所以模拟环境下的卡号与水位也会像真机一样自增。
#[derive(Clone)]
pub enum Device {
    Cli(CliDevice),
    Sim(SimDevice),
}

impl Device {
    pub fn is_sim(&self) -> bool {
        matches!(self, Self::Sim(_))
    }

    /// 模拟设备专用：离线构一张客卡。真机返回 `None`，调用方继续走 CLI。
    /// `serial` = 复用卡号（TD-127），Some 时构包器不推进 `CardNo.state`。
    pub fn sim_issue_guest(
        &self,
        room_index: u8,
        room_no: &str,
        days: u16,
        hours: u16,
        serial: Option<u16>,
    ) -> Option<Result<CardSnapshot, DeviceError>> {
        match self {
            Self::Sim(d) => d.issue_guest(room_index, room_no, days, hours, serial),
            Self::Cli(_) => None,
        }
    }

    pub fn read_card(&self) -> Result<CardSnapshot, DeviceError> {
        match self {
            Self::Cli(d) => d.read_card(),
            Self::Sim(d) => d.read_card(),
        }
    }

    /// 擦卡（TD-124）：把页1/页2 写零 → 28 字节全零 = 空白卡（锁不再认这张卡）。
    /// 真实设备走 CLI `issue --type raw`（页3–7 原厂本就全零）；模拟设备没有真实写入
    /// 闭环，直接回放空白卡快照。擦卡不消耗卡号（raw 卡不含卡号）。
    pub fn erase_card(&self) -> Result<CardSnapshot, DeviceError> {
        match self {
            Self::Cli(d) => d.issue(&["--type", "raw", "--p1", "00000000", "--p2", "00000000"]),
            Self::Sim(_) => parse::parse_card(ocw_device::sim::BLANK_JSON, 0),
        }
    }

    /// 写一张卡。`room_index` 只有模拟构包器用得上（卡里存的是序号不是房号）。
    pub fn issue(&self, args: &[&str], room_index: Option<u8>) -> Result<CardSnapshot, DeviceError> {
        match self {
            Self::Cli(d) => d.issue(args),
            Self::Sim(d) => {
                // 有构包器就按卡型真实构包（字节与真机一致），没有才回退
                if let Some(res) = d.issue_admin(args, room_index) {
                    return res;
                }
                let kind = args
                    .iter()
                    .position(|a| *a == "--type")
                    .and_then(|i| args.get(i + 1))
                    .copied();
                match kind {
                    // 安装卡与客卡有固定样本可回放
                    Some("install") => parse::parse_card(ocw_device::sim::INSTALL_JSON, 0),
                    // 其它管理卡没有样本：宁可报错，也不要静默回放客卡让人以为发对了
                    Some(other) => Err(DeviceError::Protocol(format!(
                        "模拟设备缺少 OCWCardBuild.exe，无法构 {other} 卡（先跑 npm run build:sdk）"
                    ))),
                    None => parse::parse_card(ocw_device::sim::GUEST_DAY_JSON, 0),
                }
            }
        }
    }

    pub fn selftest(&self) -> Result<SelfTest, DeviceError> {
        match self {
            Self::Cli(d) => d.selftest(),
            Self::Sim(d) => d.selftest(),
        }
    }

    pub fn probe_ports(&self) -> Result<Vec<PortProbe>, DeviceError> {
        match self {
            Self::Cli(d) => d.probe_ports(),
            Self::Sim(d) => d.probe_ports(),
        }
    }
}

/// 按运行时目录构造设备。`OCW_SIM=1` 或 SDK 不存在时退化为模拟设备。
pub fn build_device(runtime_dir: &Path, port: u8) -> Device {
    if std::env::var_os("OCW_SIM").is_some() {
        return Device::Sim(SimDevice::at(runtime_dir));
    }
    let exe = runtime_dir.join("PMSLockCard.exe");
    if !exe.is_file() {
        return Device::Sim(SimDevice::at(runtime_dir));
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
            Err(_) => Device::Sim(SimDevice::default()),
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
