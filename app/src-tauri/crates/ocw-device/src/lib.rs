//! 设备门面：把硬件层（`rc-device`）的能力包装成产品要的形状。
//!
//! # 分层
//!
//! ```text
//!   命令层（commands.rs）──▶ ocw-device ──▶ rc-device（硬件兼容层）
//!                              │                ├─ 真机：32 位 rc-device-sidecar 子进程（NDJSON）
//!                              │                └─ 模拟：进程内 SimCardDevice
//!                              ├─ map.rs  硬件快照 → 界面 DTO（含总控卡纠正）
//!                              └─ error.rs 硬件错误 → 产品错误（码 / 建议 / 分类）
//! ```
//!
//! 本 crate **不含任何卡字节知识**：卡格式、卡号编码、时长码、回读校验全部在
//! `rc-device`。这里只解决三件产品侧的事：
//!
//! 1. 64 位主程序无法加载 32 位驱动，所以真机走常驻 sidecar 子进程（[`SidecarDevice`]）；
//! 2. 硬件快照要变成界面 DTO（[`map`]），并保留产品对总控卡的历史口径；
//! 3. 硬件错误要变成「码 + 中文建议」（[`error`]），前端才显示得出来。
//!
//! # 为什么没有自己的 `CardBuilder`
//!
//! 曾经有过。后果是产品与 SDK 各有一份字节规则，回归时两边的黄金向量对不上。
//! 现在写卡参数是 `rc_device::*Spec`，产品侧拼不出字节，也就不可能拼错。

pub mod error;
pub mod map;
pub mod sidecar;
pub mod sim;
pub mod workno;

pub use error::DeviceError;
pub use map::{CardKind, CardSnapshot, PortProbe, SelfTest};
pub use workno::work_no;
pub use rc_device::DeviceInfo;
pub use sidecar::{
    SidecarDevice, CARD_TIMEOUT, OPEN_TIMEOUT, PROBE_TIMEOUT, SELFTEST_TIMEOUT,
};
pub use sim::SimDevice;

// 写卡入参直接复用硬件层的类型：产品侧不再定义第二套 `*Spec`。
pub use rc_device::{
    CardDuration, CardIssuedAt, CardStamp, CheckoutHourSpec, GuestDaySpec, GuestHourSpec,
    InstallSpec, MasterSpec, PlainCardSpec, RawSpec, TimeSyncSpec,
};
