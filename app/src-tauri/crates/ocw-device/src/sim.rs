//! 进程内模拟设备：没有读卡器时也能跑完整发卡流程。
//!
//! # 模拟由硬件层提供，不是产品自己拼
//!
//! 旧实现是「产品编译一个 `OCWCardBuild.exe`，再按 CLI 参数拼包」——卡字节规则
//! 在模拟路径上又写了一遍，和真机路径迟早对不上。现在直接用 `rc_device::sim::SimCardDevice`：
//! 它实现同一个 `CardDevice` 契约，写卡走 `builder`、读卡走 `parser`，
//! 与真机**逐字节相同**，只是把「写进卡」换成「写进内存卡槽」。
//!
//! # 为什么不用 sidecar 的 `--sim`
//!
//! 两种方式都提供，用途不同：
//!
//! - **进程内**（本模块）：不需要 exe、不需要 32 位、启动零延迟，适合 `OCW_SIM=1` 的
//!   日常开发与单元测试；
//! - **sidecar `--sim`**：连协议、进程生命周期、崩溃重启一起测，适合端到端与 CI 冒烟。
//!
//! 两者返回的 DTO 与错误形状完全一致，命令层看不出差别。

use rc_device::sim::SimCardDevice;
use rc_device::CardDevice;

use crate::error::DeviceError;
use crate::map::{self, CardSnapshot, PortProbe, SelfTest};

/// 模拟设备。默认卡槽里预置一张空白卡，保证「写卡前先读卡」能通过。
pub struct SimDevice {
    inner: SimCardDevice,
}

impl Default for SimDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl SimDevice {
    pub fn new() -> Self {
        Self {
            inner: SimCardDevice::new(),
        }
    }

    /// 卡槽里放一张现成的 28 字节卡（演示「读一张已发出的卡」）。
    pub fn with_card(raw: [u8; rc_device::CARD_BYTES]) -> Self {
        Self {
            inner: SimCardDevice::with_card(raw),
        }
    }

    /// 另一个后端实例，互不共享卡槽状态。
    pub fn from(inner: SimCardDevice) -> Self {
        Self { inner }
    }

    // ------------------------------------------------------------ 卡槽控制

    /// 把卡放到读卡器上。
    pub fn insert_card(&self, raw: [u8; rc_device::CARD_BYTES]) {
        self.inner.insert_card(raw);
    }

    /// 把卡拿走：之后读卡返回 `no_card`。
    pub fn remove_card(&self) {
        self.inner.remove_card();
    }

    /// 卡槽里当前的 28 字节。
    pub fn card(&self) -> Option<[u8; rc_device::CARD_BYTES]> {
        self.inner.card()
    }

    // ------------------------------------------------------------ 设备操作

    pub fn read_card(&self) -> Result<CardSnapshot, DeviceError> {
        self.inner.read_card().map(map::to_dto).map_err(to_err)
    }

    pub fn erase_card(&self) -> Result<CardSnapshot, DeviceError> {
        self.inner.erase_card().map(map::to_dto).map_err(to_err)
    }

    pub fn write_guest_day(&self, spec: &rc_device::GuestDaySpec) -> Result<CardSnapshot, DeviceError> {
        self.inner
            .write_guest_day(spec)
            .map(map::to_dto)
            .map_err(to_err)
    }

    pub fn write_guest_hour(
        &self,
        spec: &rc_device::GuestHourSpec,
    ) -> Result<CardSnapshot, DeviceError> {
        self.inner
            .write_guest_hour(spec)
            .map(map::to_dto)
            .map_err(to_err)
    }

    pub fn write_master(&self, spec: &rc_device::MasterSpec) -> Result<CardSnapshot, DeviceError> {
        self.inner.write_master(spec).map(map::to_dto).map_err(to_err)
    }

    pub fn write_install(&self, spec: &rc_device::InstallSpec) -> Result<CardSnapshot, DeviceError> {
        self.inner
            .write_install(spec)
            .map(map::to_dto)
            .map_err(to_err)
    }

    pub fn write_reset(&self, spec: &rc_device::PlainCardSpec) -> Result<CardSnapshot, DeviceError> {
        self.inner.write_reset(spec).map(map::to_dto).map_err(to_err)
    }

    pub fn write_clear(&self, spec: &rc_device::PlainCardSpec) -> Result<CardSnapshot, DeviceError> {
        self.inner.write_clear(spec).map(map::to_dto).map_err(to_err)
    }

    pub fn write_checkout_hour(
        &self,
        spec: &rc_device::CheckoutHourSpec,
    ) -> Result<CardSnapshot, DeviceError> {
        self.inner
            .write_checkout_hour(spec)
            .map(map::to_dto)
            .map_err(to_err)
    }

    pub fn write_timesync(
        &self,
        spec: &rc_device::TimeSyncSpec,
    ) -> Result<CardSnapshot, DeviceError> {
        self.inner
            .write_timesync(spec)
            .map(map::to_dto)
            .map_err(to_err)
    }

    /// 原始直写：模拟环境下同样会写出一张「坏卡」，不会替你回滚。
    pub fn write_raw(&self, spec: &rc_device::RawSpec) -> Result<CardSnapshot, DeviceError> {
        self.inner.write_raw(spec).map(map::to_dto).map_err(to_err)
    }

    pub fn probe_ports(&self) -> Result<Vec<PortProbe>, DeviceError> {
        self.inner
            .probe_ports()
            .map(|list| list.into_iter().map(PortProbe::from).collect())
            .map_err(to_err)
    }

    pub fn set_port(&self, port: u8) -> Result<(), DeviceError> {
        self.inner.set_port(port).map_err(to_err)
    }

    pub fn selftest(&self) -> Result<SelfTest, DeviceError> {
        self.inner.selftest().map(SelfTest::from).map_err(to_err)
    }

    pub fn device_info(&self) -> Result<rc_device::DeviceInfo, DeviceError> {
        self.inner.device_info().map_err(to_err)
    }
}

/// 硬件层错误 → 产品错误。模拟与真机走同一条映射，前端不需要区分后端。
fn to_err(e: rc_device::DeviceError) -> DeviceError {
    DeviceError::from_payload(e.payload())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use rc_device::{timestamp_card_no, CardIssuedAt, GuestDaySpec, RawSpec};

    fn issued_at() -> CardIssuedAt {
        CardIssuedAt {
            year: 2026,
            month: 11,
            day: 8,
            hour: 14,
        }
    }

    fn guest_day(index: u16) -> GuestDaySpec {
        GuestDaySpec {
            room_index: index,
            days: 1,
            card_no: timestamp_card_no(&issued_at(), 0).unwrap(),
            lost: false,
            issued_at: issued_at(),
        }
    }

    /// 默认有一张空白卡：否则模拟环境里任何发卡都会在「写卡前预读」失败。
    #[test]
    fn new_sim_holds_a_blank_card() {
        let dev = SimDevice::new();
        assert_eq!(dev.read_card().unwrap().kind, crate::CardKind::Blank);
    }

    /// 发卡 → 读回：DTO 字段与字节都要对得上。
    #[test]
    fn written_card_reads_back_with_same_bytes() {
        let dev = SimDevice::new();
        let written = dev.write_guest_day(&guest_day(15)).unwrap();
        let back = dev.read_card().unwrap();

        assert_eq!(written.kind, crate::CardKind::GuestDay);
        assert_eq!(written.room_index, 15);
        assert_eq!(written.days, 1);
        assert_eq!(back.raw_hex, written.raw_hex);
        assert!(dev.card().is_some(), "写卡后卡槽里必须留有这张卡");
    }

    /// 没放卡时是 `no_card`（码 5），前端据此提示「把卡放上去」。
    #[test]
    fn missing_card_reports_no_card_error() {
        let dev = SimDevice::new();
        dev.remove_card();
        let err = dev.read_card().unwrap_err();
        assert_eq!(err.kind(), "no_card");
        assert_eq!(err.exit_code(), 5);
        assert!(!err.is_device_fault());
    }

    /// 擦卡把卡写零：持卡退房后要用 `kind=blank` 判定擦卡成功。
    #[test]
    fn erase_produces_a_blank_card() {
        let dev = SimDevice::new();
        dev.write_guest_day(&guest_day(3)).unwrap();
        let erased = dev.erase_card().unwrap();
        assert_eq!(erased.kind, crate::CardKind::Blank);
        assert!(erased.raw_hex.chars().all(|c| c == '0'));
    }

    /// 原始直写在模拟环境里同样能写出「读不懂的卡」，且不回滚。
    #[test]
    fn raw_write_keeps_garbage() {
        let dev = SimDevice::new();
        let spec = RawSpec {
            p1: [0x11, 0x22, 0x33, 0x44],
            p2: [0x55, 0x66, 0x77, 0x88],
        };
        let snap = dev.write_raw(&spec).unwrap();
        assert_eq!(snap.kind, crate::CardKind::Unknown);
        assert_eq!(dev.card().unwrap()[0..4], spec.p1);
    }

    /// 自检 46 项必须全绿——模拟可信的唯一依据。
    #[test]
    fn selftest_is_green() {
        let report = SimDevice::new().selftest().unwrap();
        assert!(report.is_ok(), "{} 项失败", report.failed);
        assert_eq!(report.passed, 46);
    }

    /// 端口探测：当前端口可用，其余不可用（前端的端口选择器据此排序）。
    #[test]
    fn probe_ports_only_current_is_available() {
        let dev = SimDevice::new();
        dev.set_port(2).unwrap();
        let ports = dev.probe_ports().unwrap();
        assert_eq!(ports.len(), 8);
        assert!(ports[2].available);
        assert_eq!(ports[2].com, "COM3");
        assert!(!ports[0].available);
    }

    /// 模拟后端要能被识别出来，诊断包才有「当前在模拟」这一条。
    #[test]
    fn device_info_reports_mock_backend() {
        let info = SimDevice::new().device_info().unwrap();
        assert_eq!(info.backend, rc_device::DeviceBackend::Mock);
    }
}
