//! 设备层排障小工具：直接调 CLI 并打印原始输出。
//!
//! ```text
//! cargo run -p ocw-device --example probe -- <exe> <ini> <driver_dir> <port> <verb> [args..]
//! ```

use ocw_device::CliDevice;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 5 {
        eprintln!("用法: probe <PMSLockCard.exe> <LockCard.ini> <驱动目录> <port> <verb> [args..]");
        std::process::exit(2);
    }

    let port: u8 = args[3].parse().unwrap_or(6);
    let verb = args[4].clone();
    let rest: Vec<&str> = args[5..].iter().map(|s| s.as_str()).collect();

    let dev = CliDevice::new(args[0].clone(), args[1].clone(), args[2].clone(), port);

    match dev.run_raw(&verb, &rest) {
        Ok((code, stdout, stderr)) => {
            println!("exit = {code}");
            println!("stdout ({} bytes) = <<<{stdout}>>>", stdout.len());
            println!("stderr ({} bytes) = <<<{stderr}>>>", stderr.len());
            let parsed = match verb.as_str() {
                "ports" => format!("{:?}", ocw_device::parse_ports(&stdout)),
                "selftest" => format!("{:?}", ocw_device::parse_selftest(&stdout)),
                _ => format!("{:?}", ocw_device::parse_card(&stdout, code)),
            };
            println!("parsed = {parsed}");
        }
        Err(e) => {
            println!("ERR = {e:?}");
            println!("message = {e}");
            println!("hint    = {}", e.hint());
        }
    }
}
