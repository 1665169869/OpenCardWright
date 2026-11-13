//! 与 32 位 sidecar 进程的常驻 NDJSON 会话。
//!
//! # 为什么是常驻进程
//!
//! 打开读卡器要「加载 DLL → 开串口 → 握手 → 选卡」，真机上耗时在数百毫秒量级；
//! 每发一张卡重来一遍，前台会明显卡顿。更重要的是串口是独占资源，
//! 频繁开关更容易撞上「上一轮的端口还没释放」。
//!
//! # 三条纪律
//!
//! 1. **写卡绝不重试**。重试会把同一张卡写两遍、卡号推进两次；失败就如实返回，
//!    让前台人工确认。
//! 2. **单请求超时就杀掉进程**，下次调用重新拉起。超时意味着驱动的工作线程可能
//!    还卡在半路，继续用同一个会话会让迟到的响应与后续请求错位。
//! 3. **stdout 只读响应行、stderr 只收日志**。sidecar 的 stdout 只有 NDJSON 响应，
//!    日志走 stderr；这里把 stderr 末尾几行留着，拼进错误详情，现场排查全靠它。

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rc_device::sidecar::{Operation, Request, Response};
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::DeviceError;
use crate::map::{self, CardSnapshot, PortProbe, SelfTest};

/// 单片操作（读卡 / 写卡 / 擦卡）的上限。
///
/// 开串口 + 握手 + 写两页 + 回读实测在 3 秒内，留足余量后仍能在读卡器掉线时
/// 及时把错误交回界面，而不是让界面一直转圈。
pub const CARD_TIMEOUT: Duration = Duration::from_secs(20);
/// 打开设备：加载驱动 + 开串口 + 握手 + 选卡。
pub const OPEN_TIMEOUT: Duration = Duration::from_secs(15);
/// 串口探测要逐个端口开合，8 个端口全试一遍需要更长时间。
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(60);
/// 离线自检是纯计算（46 项字节向量），30 秒足够。
pub const SELFTEST_TIMEOUT: Duration = Duration::from_secs(30);
/// 退出时发 shutdown 的上限：拿不到回应就直接杀。
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

/// 常驻 sidecar 的设备客户端。
pub struct SidecarDevice {
    exe: PathBuf,
    driver_dir: Option<PathBuf>,
    /// 以 `--sim` 启动：用内存模拟后端，不加载驱动。
    sim: bool,
    /// `--sim` 时的预置卡（`blank` / `guest-day` / `guest-hour` / `none`）。
    sim_card: Option<String>,
    card_timeout: Duration,
    session: Mutex<Option<Session>>,
    /// 当前会话里是否已经 open 过。会话重建后必须重新 open。
    opened: AtomicBool,
    next_id: AtomicU64,
}

/// 一次 sidecar 会话：子进程 + stdin + stdout 的接收端 + stderr 尾巴。
struct Session {
    child: Child,
    stdin: ChildStdin,
    /// stdout 由后台线程逐行转发过来；`Err` 表示读管道失败。
    lines: Receiver<std::io::Result<String>>,
    reader: Option<JoinHandle<()>>,
    /// stderr 只保留最后若干行，拼进错误详情。
    stderr: Arc<Mutex<Vec<String>>>,
    stderr_reader: Option<JoinHandle<()>>,
}

impl Drop for Session {
    fn drop(&mut self) {
        // 先杀再 join：管道关闭后两个后台线程的阻塞读会立刻返回。
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(handle) = self.reader.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.stderr_reader.take() {
            let _ = handle.join();
        }
    }
}

impl SidecarDevice {
    /// 连接真机 sidecar。
    pub fn new(exe: impl Into<PathBuf>, driver_dir: Option<PathBuf>) -> Self {
        Self {
            exe: exe.into(),
            driver_dir,
            sim: false,
            sim_card: None,
            card_timeout: CARD_TIMEOUT,
            session: Mutex::new(None),
            opened: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
        }
    }

    /// 连接以 `--sim` 启动的 sidecar（内存模拟，不碰硬件）。
    pub fn sim(exe: impl Into<PathBuf>) -> Self {
        Self {
            exe: exe.into(),
            driver_dir: None,
            sim: true,
            sim_card: None,
            card_timeout: CARD_TIMEOUT,
            session: Mutex::new(None),
            opened: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
        }
    }

    /// 以 `--sim` 启动，并指定卡槽里的预置卡。
    ///
    /// `card` 取 `blank`（默认）/ `guest-day` / `guest-hour` / `none`（没放卡）。
    /// 用于演示「读一张已发出的卡」与测试「没放卡」这类分支。
    pub fn sim_with_initial_card(exe: impl Into<PathBuf>, card: &str) -> Self {
        Self {
            exe: exe.into(),
            driver_dir: None,
            sim: true,
            sim_card: Some(card.to_string()),
            card_timeout: CARD_TIMEOUT,
            session: Mutex::new(None),
            opened: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
        }
    }

    /// 调整单片操作的超时（测试用；生产用默认值）。
    pub fn with_card_timeout(mut self, timeout: Duration) -> Self {
        self.card_timeout = timeout;
        self
    }

    /// sidecar 可执行文件路径。
    pub fn exe_path(&self) -> &std::path::Path {
        &self.exe
    }

    // ------------------------------------------------------------ 设备操作

    /// 打开设备。幂等：真实机重复调用不会重复握手（sidecar 内部判断）。
    pub fn open(&self) -> Result<(), DeviceError> {
        let value: Value = self.call(Operation::Open, OPEN_TIMEOUT, false)?;
        let _ = value;
        self.opened.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// 读一张卡。
    pub fn read_card(&self) -> Result<CardSnapshot, DeviceError> {
        let raw: rc_device::CardSnapshot = self.call(Operation::ReadCard, self.card_timeout, true)?;
        Ok(map::to_dto(raw))
    }

    /// 擦卡：写零，锁不再认这张卡。
    pub fn erase_card(&self) -> Result<CardSnapshot, DeviceError> {
        let raw: rc_device::CardSnapshot = self.call(Operation::EraseCard, self.card_timeout, true)?;
        Ok(map::to_dto(raw))
    }

    /// 写日租客卡。
    pub fn write_guest_day(&self, spec: &rc_device::GuestDaySpec) -> Result<CardSnapshot, DeviceError> {
        let op = Operation::WriteGuestDay { spec: *spec };
        let raw: rc_device::CardSnapshot = self.call(op, self.card_timeout, true)?;
        Ok(map::to_dto(raw))
    }

    /// 写钟点客卡。
    pub fn write_guest_hour(
        &self,
        spec: &rc_device::GuestHourSpec,
    ) -> Result<CardSnapshot, DeviceError> {
        let op = Operation::WriteGuestHour { spec: *spec };
        let raw: rc_device::CardSnapshot = self.call(op, self.card_timeout, true)?;
        Ok(map::to_dto(raw))
    }

    /// 写总控卡。
    pub fn write_master(&self, spec: &rc_device::MasterSpec) -> Result<CardSnapshot, DeviceError> {
        let op = Operation::WriteMaster { spec: *spec };
        let raw: rc_device::CardSnapshot = self.call(op, self.card_timeout, true)?;
        Ok(map::to_dto(raw))
    }

    /// 写安装卡。
    pub fn write_install(&self, spec: &rc_device::InstallSpec) -> Result<CardSnapshot, DeviceError> {
        let op = Operation::WriteInstall { spec: *spec };
        let raw: rc_device::CardSnapshot = self.call(op, self.card_timeout, true)?;
        Ok(map::to_dto(raw))
    }

    /// 写复位卡。
    pub fn write_reset(&self, spec: &rc_device::PlainCardSpec) -> Result<CardSnapshot, DeviceError> {
        let op = Operation::WriteReset { spec: *spec };
        let raw: rc_device::CardSnapshot = self.call(op, self.card_timeout, true)?;
        Ok(map::to_dto(raw))
    }

    /// 写清除卡。
    pub fn write_clear(&self, spec: &rc_device::PlainCardSpec) -> Result<CardSnapshot, DeviceError> {
        let op = Operation::WriteClear { spec: *spec };
        let raw: rc_device::CardSnapshot = self.call(op, self.card_timeout, true)?;
        Ok(map::to_dto(raw))
    }

    /// 写退房钟点设置卡。
    pub fn write_checkout_hour(
        &self,
        spec: &rc_device::CheckoutHourSpec,
    ) -> Result<CardSnapshot, DeviceError> {
        let op = Operation::WriteCheckoutHour { spec: *spec };
        let raw: rc_device::CardSnapshot = self.call(op, self.card_timeout, true)?;
        Ok(map::to_dto(raw))
    }

    /// 写校时卡。
    pub fn write_timesync(&self, spec: &rc_device::TimeSyncSpec) -> Result<CardSnapshot, DeviceError> {
        let op = Operation::WriteTimesync { spec: *spec };
        let raw: rc_device::CardSnapshot = self.call(op, self.card_timeout, true)?;
        Ok(map::to_dto(raw))
    }

    /// 原始直写：把 8 字节原样写进卡。破坏性接口，仅供逆向/排障。
    pub fn write_raw(&self, spec: &rc_device::RawSpec) -> Result<CardSnapshot, DeviceError> {
        let op = Operation::WriteRaw { spec: *spec };
        let raw: rc_device::CardSnapshot = self.call(op, self.card_timeout, true)?;
        Ok(map::to_dto(raw))
    }

    // ------------------------------------------------------------ 环境

    /// 探测串口 0–7。
    pub fn probe_ports(&self) -> Result<Vec<PortProbe>, DeviceError> {
        let probes: Vec<rc_device::PortProbe> = self.call(Operation::ProbePorts, PROBE_TIMEOUT, false)?;
        Ok(probes.into_iter().map(PortProbe::from).collect())
    }

    /// 切换串口。sidecar 内部会关闭旧端口再用新端口打开。
    pub fn set_port(&self, port: u8) -> Result<(), DeviceError> {
        if port > 7 {
            return Err(DeviceError::Device {
                code: 1,
                message: format!("串口索引必须在 0–7，实际 {port}"),
                hint: "用「探测端口」看看读卡器挂在哪一个口".to_string(),
                device_fault: false,
                kind: "param".to_string(),
            });
        }
        let value: Value = self.call(Operation::SetPort { port }, OPEN_TIMEOUT, false)?;
        let _ = value;
        // 换口等于重开会话：下一次业务调用前重新 open（幂等，代价很小）
        self.opened.store(false, Ordering::SeqCst);
        Ok(())
    }

    /// 跑一遍离线自检（46 项）。
    pub fn selftest(&self) -> Result<SelfTest, DeviceError> {
        let report: rc_device::SelfTestReport = self.call(Operation::Selftest, SELFTEST_TIMEOUT, false)?;
        Ok(SelfTest::from(report))
    }

    /// 设备与环境信息（驱动路径/哈希/架构/端口）。
    pub fn device_info(&self) -> Result<rc_device::DeviceInfo, DeviceError> {
        self.call(Operation::DeviceInfo, OPEN_TIMEOUT, false)
    }

    /// 关闭会话：先礼后兵——发 shutdown，超时就由 `Session` 的 Drop 杀掉。
    pub fn shutdown(&self) {
        if let Ok(mut guard) = self.session.lock() {
            if guard.is_some() {
                let _ = self.round_trip::<Value>(&mut guard, Operation::Shutdown, SHUTDOWN_TIMEOUT);
            }
            *guard = None;
        }
        self.opened.store(false, Ordering::SeqCst);
    }

    // ------------------------------------------------------------ 内部

    fn lock_session(&self) -> Result<MutexGuard<'_, Option<Session>>, DeviceError> {
        self.session
            .lock()
            .map_err(|_| DeviceError::Io("设备会话锁异常，请重启程序".to_string()))
    }

    /// 一次完整请求：必要时拉起进程、必要时 open、然后收发。
    fn call<T: DeserializeOwned>(
        &self,
        op: Operation,
        timeout: Duration,
        ensure_open: bool,
    ) -> Result<T, DeviceError> {
        let mut guard = self.lock_session()?;
        if guard.is_none() {
            *guard = Some(self.spawn()?);
            self.opened.store(false, Ordering::SeqCst);
        }

        let result = if ensure_open && !self.opened.load(Ordering::SeqCst) {
            // 打开失败（没插读卡器 / 串口被占）时把错误原样交回，不要继续发业务命令
            match self.round_trip::<Value>(&mut guard, Operation::Open, OPEN_TIMEOUT) {
                Ok(_) => {
                    self.opened.store(true, Ordering::SeqCst);
                    self.round_trip::<T>(&mut guard, op, timeout)
                }
                Err(e) => Err(e),
            }
        } else {
            self.round_trip::<T>(&mut guard, op, timeout)
        };

        if guard.is_none() {
            // 会话已被丢弃（崩溃 / 超时 / 协议错）：下一次调用重新拉起并重新 open
            self.opened.store(false, Ordering::SeqCst);
        }
        result
    }

    /// 写一行请求、等一行配对响应。
    ///
    /// 返回 `Err` 且把 `slot` 置空，表示这个会话已经不可信。
    fn round_trip<T: DeserializeOwned>(
        &self,
        slot: &mut Option<Session>,
        op: Operation,
        timeout: Duration,
    ) -> Result<T, DeviceError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let name = op.name();
        let line = Request::new(id, op)
            .to_line()
            .map_err(|e| DeviceError::Protocol(format!("请求无法编码：{e}")))?;

        let Some(session) = slot.as_mut() else {
            return Err(DeviceError::Io("设备会话已失效，请重试".to_string()));
        };

        let written = session
            .stdin
            .write_all(line.as_bytes())
            .and_then(|()| session.stdin.write_all(b"\n"))
            .and_then(|()| session.stdin.flush());
        if let Err(e) = written {
            let detail = stderr_tail(session);
            *slot = None;
            return Err(DeviceError::Io(format!("写入设备进程失败：{e}{detail}")));
        }

        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match session.lines.recv_timeout(remaining) {
                Ok(Ok(raw)) => {
                    let text = raw.trim();
                    if text.is_empty() {
                        continue;
                    }
                    match Response::from_line(text) {
                        Ok(resp) if resp.id == id => {
                            return resp.into_result::<T>().map_err(DeviceError::from_payload);
                        }
                        Ok(resp) => {
                            // 上次超时后残留下来的响应：丢弃并继续等自己那条
                            eprintln!("[device] 丢弃错位响应 #{}（期望 #{id}，{name}）", resp.id);
                        }
                        Err(e) => {
                            let detail = stderr_tail(session);
                            *slot = None;
                            return Err(DeviceError::Protocol(format!(
                                "响应行无法解析：{e}（{text}）{detail}"
                            )));
                        }
                    }
                }
                Ok(Err(e)) => {
                    let detail = stderr_tail(session);
                    *slot = None;
                    return Err(DeviceError::Io(format!("读取设备进程输出失败：{e}{detail}")));
                }
                Err(RecvTimeoutError::Timeout) => {
                    let detail = stderr_tail(session);
                    if !detail.is_empty() {
                        eprintln!("[device] {name} 超时{detail}");
                    }
                    // 杀掉进程：工作线程可能还卡在驱动里，重启会话才是干净状态
                    *slot = None;
                    return Err(DeviceError::Timeout(timeout.as_secs()));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let detail = stderr_tail(session);
                    *slot = None;
                    return Err(DeviceError::Io(format!("设备进程已退出{detail}")));
                }
            }
        }
    }

    /// 拉起 sidecar 进程，返回一个可用的会话。
    fn spawn(&self) -> Result<Session, DeviceError> {
        if !self.exe.is_file() {
            return Err(DeviceError::Spawn(format!(
                "{}（先跑 pnpm build:device）",
                self.exe.display()
            )));
        }

        let mut cmd = Command::new(&self.exe);
        if let Some(dir) = &self.driver_dir {
            cmd.arg("--driver-dir").arg(dir);
        }
        if self.sim {
            cmd.arg("--sim");
            if let Some(card) = &self.sim_card {
                cmd.arg("--sim-card").arg(card);
            }
        }
        if let Some(dir) = self.exe.parent() {
            cmd.current_dir(dir);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // 不弹黑框；CI 与无桌面环境也安全。
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| DeviceError::Spawn(format!("{}：{e}", self.exe.display())))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| DeviceError::Io("stdout/stdin 未捕获".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| DeviceError::Io("stdout 未捕获".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| DeviceError::Io("stderr 未捕获".to_string()))?;

        let (tx, rx) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut buf = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match buf.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if tx.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        break;
                    }
                }
            }
        });

        let stderr_buf: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let stderr_for_thread = Arc::clone(&stderr_buf);
        let stderr_reader = std::thread::spawn(move || {
            let buf = BufReader::new(stderr);
            for line in buf.lines().map_while(Result::ok) {
                if let Ok(mut tail) = stderr_for_thread.lock() {
                    if tail.len() >= STDERR_TAIL_LINES {
                        tail.remove(0);
                    }
                    tail.push(line);
                }
            }
        });

        Ok(Session {
            child,
            stdin,
            lines: rx,
            reader: Some(reader),
            stderr: stderr_buf,
            stderr_reader: Some(stderr_reader),
        })
    }
}

impl Drop for SidecarDevice {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// stderr 尾巴保留多少行。3 行足够看出「串口被占 / 无卡 / 驱动缺失」。
const STDERR_TAIL_LINES: usize = 20;

/// 取 stderr 最后 3 行，拼成错误详情。
fn stderr_tail(session: &Session) -> String {
    let lines = session
        .stderr
        .lock()
        .map(|v| v.clone())
        .unwrap_or_default();
    if lines.is_empty() {
        return String::new();
    }
    let tail: Vec<String> = lines.iter().rev().take(3).rev().cloned().collect();
    format!("（sidecar: {}）", tail.join(" / "))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// 找不到 exe 必须是 `Spawn` 错误，而不是 panic 或含糊的 IO 错误。
    #[test]
    fn missing_exe_is_spawn_error() {
        let dev = SidecarDevice::new(r"C:\does\not\exist\rc-device-sidecar.exe", None);
        let err = dev.read_card().unwrap_err();
        assert!(matches!(err, DeviceError::Spawn(_)), "{err:?}");
        assert!(err.hint().contains("rc-device-sidecar"));
    }

    /// 越界端口在本地就拦住，不必惊动 sidecar。
    #[test]
    fn out_of_range_port_is_param_error() {
        let dev = SidecarDevice::new(r"C:\does\not\exist\rc-device-sidecar.exe", None);
        let err = dev.set_port(8).unwrap_err();
        assert_eq!(err.exit_code(), 1);
        assert_eq!(err.kind(), "param");
        assert!(!err.is_device_fault());
    }
}
