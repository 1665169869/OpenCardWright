//! 设备通道。
//!
//! 边界很硬：本 crate 只负责**启动 `PMSLockCard.exe` 并解析它的输出**，
//! 不实现、也不复制任何卡内字节格式。卡格式的实现留在 `sdk/`（.NET），
//! 那里有 27 条黄金向量做回归。
//!
//! 这样做的理由：驱动是 32 位且版本极敏感，把「字节怎么写」与「流程怎么编排」
//! 分开，崩溃可隔离、超时可杀进程、将来换设备只改这一层。

pub mod cli;
pub mod parse;
pub mod sim;

pub use cli::CliDevice;
pub use parse::{
    parse_card, parse_ports, parse_selftest, CardKind, CardSnapshot, DeviceError, PortProbe,
    SelfTest, EXIT_CODE_UNKNOWN,
};
pub use sim::SimDevice;
