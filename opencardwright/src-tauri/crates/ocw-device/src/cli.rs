//! 启动 `PMSLockCard.exe` 并取回输出。
//!
//! 三条纪律：
//! 1. 参数**全量显式**传（`--ini` / `--dir` / `--port` / `--json`），不依赖工作目录；
//! 2. 有超时，超时就杀进程——宁可让用户重试，也不让界面永久卡住；
//! 3. **不自动重试**。写卡重试可能把一张卡写两遍、把卡号推进两次。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::parse::{self, CardSnapshot, DeviceError, PortProbe, SelfTest};

/// 单片操作的默认上限。开串口 + 握手 + 写两页 + 回读，实测在 3s 内，留足余量。
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub struct CliDevice {
    exe: PathBuf,
    ini: PathBuf,
    driver_dir: PathBuf,
    port: u8,
    timeout: Duration,
}

struct RunOutput {
    code: i32,
    stdout: String,
    stderr: String,
}

impl CliDevice {
    pub fn new(exe: impl Into<PathBuf>, ini: impl Into<PathBuf>, driver_dir: impl Into<PathBuf>, port: u8) -> Self {
        Self {
            exe: exe.into(),
            ini: ini.into(),
            driver_dir: driver_dir.into(),
            port,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn exe_path(&self) -> &Path {
        &self.exe
    }

    pub fn port(&self) -> u8 {
        self.port
    }

    pub fn read_card(&self) -> Result<CardSnapshot, DeviceError> {
        let out = self.run("read", &["--json"])?;
        parse::parse_card(&out.stdout, out.code).map_err(|e| enrich(e, &out.stderr))
    }

    /// 发卡。`args` 为除全局参数外该子命令自己的参数，例如
    /// `["--room", "220", "--days", "1", "--json"]`。
    pub fn issue(&self, args: &[&str]) -> Result<CardSnapshot, DeviceError> {
        let mut full: Vec<&str> = args.to_vec();
        full.push("--json");
        let out = self.run("issue", &full)?;
        parse::parse_card(&out.stdout, out.code).map_err(|e| enrich(e, &out.stderr))
    }

    pub fn selftest(&self) -> Result<SelfTest, DeviceError> {
        let out = self.run("selftest", &[])?;
        parse::parse_selftest(&out.stdout).map_err(|e| enrich(e, &out.stderr))
    }

    pub fn probe_ports(&self) -> Result<Vec<PortProbe>, DeviceError> {
        let out = self.run("ports", &[])?;
        if out.stdout.trim().is_empty() {
            // 把退出码和 stderr 带出来，否则现场只能看到一句"没有输出"
            let detail = if out.stderr.trim().is_empty() {
                format!("退出码 {}，stdout 与 stderr 都是空的", out.code)
            } else {
                out.stderr.trim().to_string()
            };
            return Err(DeviceError::Protocol(format!("ports 无输出：{detail}")));
        }
        Ok(parse::parse_ports(&out.stdout))
    }

    /// 原始调用：返回 (退出码, stdout, stderr)。给诊断包和排障用，不参与业务逻辑。
    pub fn run_raw(&self, verb: &str, extra: &[&str]) -> Result<(i32, String, String), DeviceError> {
        let out = self.run(verb, extra)?;
        Ok((out.code, out.stdout, out.stderr))
    }

    /// 执行一次 CLI 调用。`verb` 必须是 `args[0]`——CLI 是按位置读子命令的。
    fn run(&self, verb: &str, extra: &[&str]) -> Result<RunOutput, DeviceError> {
        if !self.exe.is_file() {
            return Err(DeviceError::Spawn(format!(
                "找不到设备程序: {}",
                self.exe.display()
            )));
        }

        let mut cmd = Command::new(&self.exe);
        cmd.arg(verb)
            .arg("--ini")
            .arg(&self.ini)
            .arg("--dir")
            .arg(&self.driver_dir)
            .arg("--port")
            .arg(self.port.to_string());
        for a in extra {
            cmd.arg(a);
        }

        if let Some(dir) = self.exe.parent() {
            cmd.current_dir(dir);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // 不弹黑框；CI 与无桌面环境也安全。
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd.spawn().map_err(|e| DeviceError::Spawn(e.to_string()))?;
        let mut stdout_pipe = child
            .stdout
            .take()
            .ok_or_else(|| DeviceError::Io("stdout 未捕获".into()))?;
        let mut stderr_pipe = child
            .stderr
            .take()
            .ok_or_else(|| DeviceError::Io("stderr 未捕获".into()))?;

        let stdout_handle = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout_pipe.read_to_end(&mut buf);
            decode(&buf)
        });
        let stderr_handle = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stderr_pipe.read_to_end(&mut buf);
            decode(&buf)
        });

        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => {
                    if started.elapsed() > self.timeout {
                        // 只杀直接子进程；CLI 本身不会再派生子进程
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(DeviceError::Timeout(self.timeout.as_secs()));
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(e) => return Err(DeviceError::Io(e.to_string())),
            }
        };

        let stdout = stdout_handle.join().unwrap_or_default();
        let stderr = stderr_handle.join().unwrap_or_default();

        Ok(RunOutput {
            code: status.code().unwrap_or(parse::EXIT_CODE_UNKNOWN),
            stdout,
            stderr,
        })
    }
}

/// 解析失败时把 stderr 附上，便于用户导出诊断包时定位。
fn enrich(err: DeviceError, stderr: &str) -> DeviceError {    let tail: String = stderr
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(5)
        .collect::<Vec<_>>()
        .join(" / ");
    if tail.is_empty() {
        return err;
    }
    match err {
        DeviceError::Protocol(m) => DeviceError::Protocol(format!("{m}（stderr: {tail}）")),
        other => other,
    }
}

/// 把子进程输出解码成字符串。
///
/// CLI 是 .NET Framework 程序，写管道时用的是**控制台代码页**（中文 Windows 上是 GBK 936），
/// 不是 UTF-8。如果直接 `read_to_string`，遇到中文会整体失败并返回空串 ——
/// 现场表现就是"设备没有任何输出"。所以先试 UTF-8，失败再按 GBK 解。
fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::GBK.decode(bytes).0.into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_exe_is_spawn_error_not_panic() {
        let dev = CliDevice::new(
            r"C:\does\not\exist\PMSLockCard.exe",
            r"C:\does\not\exist\LockCard.ini",
            r"C:\does\not\exist",
            6,
        );
        let err = dev.read_card().unwrap_err();
        assert!(matches!(err, DeviceError::Spawn(_)));
    }

    #[test]
    fn utf8_output_passes_through() {
        assert_eq!(decode("自检结果: 27 通过".as_bytes()), "自检结果: 27 通过");
    }

    #[test]
    fn gbk_output_is_decoded_not_lost() {
        // 模拟 .NET 在中文 Windows 上写出的 GBK 字节
        let (gbk, _, _) = encoding_rs::GBK.encode("COM3  可用   读卡器=4E57");
        assert!(std::str::from_utf8(&gbk).is_err(), "前提：GBK 字节不是合法 UTF-8");
        let text = decode(&gbk);
        assert_eq!(text, "COM3  可用   读卡器=4E57");
        assert!(!text.is_empty());
    }
}
