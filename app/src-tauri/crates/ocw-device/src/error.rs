//! 设备错误：把硬件层的结构化错误翻译成产品口径。
//!
//! # 为什么不再自己定一套错误码
//!
//! 旧实现里有两套码在打架：SDK 的 `LockResult`（0–11）与 CLI 的进程退出码（0–7），
//! 在 2/3 两个值上错位，解析时要额外做一次映射，谁都记不住。
//! 现在硬件层直接给出 [`rc_device::DeviceErrorPayload`]——`kind` 是稳定分类、
//! `sdkCode` 对齐原 SDK 语义、`hint` 是给现场看的中文建议——产品侧只做搬运。
//!
//! 保留 `exit_code()` 这个名字是**历史原因**：前端 `CommandError.code` 与读卡页的
//! 错误码展示都用它，语义已改成「硬件层给的 SDK 码」，不再是谁的进程退出码。

use rc_device::DeviceErrorPayload;

/// 设备层的统一错误。
#[derive(Debug, Clone, thiserror::Error)]
pub enum DeviceError {
    /// 硬件层返回的结构化错误（真机走 sidecar，模拟走进程内后端，形状一致）。
    #[error("{message}")]
    Device {
        code: i32,
        message: String,
        hint: String,
        /// 是否为设备本身故障（而非操作失误，比如「没放卡」）。
        device_fault: bool,
        /// 硬件层的稳定分类（`no_card` / `port_open_failed` / …），进日志与诊断包。
        kind: String,
    },

    /// 找不到 sidecar 可执行文件，或它起不来。
    #[error("找不到设备程序：{0}")]
    Spawn(String),

    /// 单次请求超过上限：读卡器没响应（或卡没放稳）。
    #[error("设备操作超时（{0} 秒）")]
    Timeout(u64),

    /// 与 sidecar 进程的管道断了。
    #[error("与设备进程通信失败：{0}")]
    Io(String),

    /// 响应不是约定的 NDJSON 形状。
    #[error("设备响应无法解析：{0}")]
    Protocol(String),
}

impl DeviceError {
    /// 把硬件层的错误负载搬过来，一个字段都不丢。
    pub fn from_payload(payload: DeviceErrorPayload) -> Self {
        Self::Device {
            code: payload.sdk_code,
            message: payload.message,
            hint: payload.hint,
            device_fault: payload.device_fault,
            kind: payload.kind,
        }
    }

    /// 硬件层 SDK 语义码（0–11；-1 = 本层自有故障，如找不到 exe）。
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::Device { code, .. } => *code,
            _ => -1,
        }
    }

    /// 给人看的处理建议。前端直接展示这段文本。
    pub fn hint(&self) -> &str {
        match self {
            Self::Device { hint, .. } => hint,
            Self::Spawn(_) => "确认安装目录里有 rc-device-sidecar.exe；被安全软件隔离时先加白名单",
            Self::Timeout(_) => "确认读卡器插好、串口没被原门锁软件占用，然后重试",
            Self::Io(_) => "设备进程意外退出。重启程序后再试；反复出现请导出诊断包",
            Self::Protocol(_) => "硬件层返回了无法识别的内容，请导出诊断包反馈",
        }
    }

    /// 是否属于设备故障（用于诊断包与告警分级）。
    pub fn is_device_fault(&self) -> bool {
        match self {
            Self::Device { device_fault, .. } => *device_fault,
            // 起不来 / 管道断 / 超时都是设备侧问题
            Self::Spawn(_) | Self::Timeout(_) | Self::Io(_) => true,
            // 协议不符更可能是版本错配，不算"设备坏了"
            Self::Protocol(_) => false,
        }
    }

    /// 硬件层的稳定分类；本层自有的错误给一个近似值。
    pub fn kind(&self) -> &str {
        match self {
            Self::Device { kind, .. } => kind,
            Self::Spawn(_) => "spawn_failed",
            Self::Timeout(_) => "timeout",
            Self::Io(_) => "io",
            Self::Protocol(_) => "protocol",
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use rc_device::DeviceError as HwError;

    /// 硬件层错误负载的每个字段都要原样保留，尤其是 `kind` 与 `deviceFault`。
    #[test]
    fn payload_fields_survive_the_mapping() {
        let payload = HwError::NoCard.payload();
        let err = DeviceError::from_payload(payload);

        assert_eq!(err.exit_code(), 5, "SDK 码必须原样透传");
        assert_eq!(err.kind(), "no_card");
        assert!(!err.is_device_fault(), "没放卡是操作问题，不是设备故障");
        assert!(!err.hint().trim().is_empty(), "必须给出现场建议");
        assert!(!err.to_string().trim().is_empty());
    }

    /// 设备故障（串口打不开）要标出来，前端据此区分「重试」还是「报修」。
    #[test]
    fn device_fault_is_preserved() {
        let err = DeviceError::from_payload(HwError::PortOpenFailed { port: 6 }.payload());
        assert_eq!(err.kind(), "port_open_failed");
        assert!(err.is_device_fault());
        assert_eq!(err.exit_code(), 3);
    }

    /// 本层自有错误也要有码、有建议、有分类，不能是空壳。
    #[test]
    fn local_errors_carry_hints() {
        let cases = [
            DeviceError::Spawn("x".into()),
            DeviceError::Timeout(20),
            DeviceError::Io("x".into()),
            DeviceError::Protocol("x".into()),
        ];
        for e in cases {
            assert_eq!(e.exit_code(), -1);
            assert!(!e.hint().trim().is_empty(), "{e:?} 缺少建议");
            assert!(!e.kind().trim().is_empty(), "{e:?} 缺少分类");
        }
    }
}
