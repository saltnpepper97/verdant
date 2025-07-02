use std::fs;
use std::io::Read;
use std::path::Path;
use std::ffi::CString;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use bloom::status::LogLevel;
use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::time::ProcessTimer;

pub fn set_hostname(
    console_logger: &Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
    file_logger: &Arc<Mutex<dyn FileLogger + Send + Sync>>,
) -> Result<(), BloomError> {
    let timer = ProcessTimer::start();
    let hostname_path = "/etc/hostname";

    match fs::File::open(hostname_path) {
        Ok(mut file) => {
            let mut hostname = String::new();
            if let Err(e) = file.read_to_string(&mut hostname) {
                log_error(console_logger, file_logger, &timer, LogLevel::Fail, &format!("Failed to read hostname file: {}", e));
                return Err(BloomError::Io(e));
            }
            let hostname = hostname.trim();

            match CString::new(hostname) {
                Ok(c_hostname) => {
                    let result = unsafe { libc::sethostname(c_hostname.as_ptr(), hostname.len()) };
                    if result != 0 {
                        let e = std::io::Error::last_os_error();
                        log_error(console_logger, file_logger, &timer, LogLevel::Fail, &format!("Failed to set hostname: {}", e));
                        return Err(BloomError::Io(e));
                    }
                    log_success(console_logger, file_logger, &timer, LogLevel::Ok, &format!("Hostname set to '{}'", hostname));
                    Ok(())
                }
                Err(_) => {
                    let msg = "Hostname contains invalid null byte";
                    log_error(console_logger, file_logger, &timer, LogLevel::Fail, msg);
                    Err(BloomError::Parse(msg.into()))
                }
            }
        }
        Err(e) => {
            log_error(console_logger, file_logger, &timer, LogLevel::Fail, &format!("Failed to open hostname file: {}", e));
            Err(BloomError::Io(e))
        }
    }
}

pub fn detect_timezone(
    console_logger: &Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
    file_logger: &Arc<Mutex<dyn FileLogger + Send + Sync>>,
) -> Result<Option<String>, BloomError> {
    let timer = ProcessTimer::start();
    let localtime_path = Path::new("/etc/localtime");

    let link_target = match fs::read_link(localtime_path) {
        Ok(target) => target,
        Err(e) => {
            log_error(console_logger, file_logger, &timer, LogLevel::Warn, &format!(
                "Could not read /etc/localtime symlink: {}", e));
            return Ok(None);
        }
    };

    let zoneinfo_roots = [
        Path::new("/usr/share/zoneinfo/"),
        Path::new("/etc/zoneinfo/"),
    ];

    for root in &zoneinfo_roots {
        if let Ok(stripped) = link_target.strip_prefix(root) {
            if let Some(tz_str) = stripped.to_str() {
                log_success(console_logger, file_logger, &timer, LogLevel::Ok, &format!(
                    "Detected timezone '{}'", tz_str));
                return Ok(Some(tz_str.to_string()));
            }
        }
    }

    log_error(console_logger, file_logger, &timer, LogLevel::Warn, &format!(
        "/etc/localtime symlink does not point inside known zoneinfo roots: {:?}", link_target));
    Ok(None)
}

/// Synchronize system clock from hardware RTC using `/sbin/hwclock --hctosys --utc`
/// Uses mutable refs because it runs synchronously and no need for locking.
pub fn sync_clock_from_hardware(
    console_logger: &mut dyn ConsoleLogger,
    file_logger: &mut dyn FileLogger,
) -> Result<(), BloomError> {
    let timer = ProcessTimer::start();
    let status = Command::new("/sbin/hwclock")
        .arg("--hctosys")
        .arg("--utc")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    match status {
        Ok(s) if s.success() => {
            console_logger.message(LogLevel::Ok, "Synchronized system clock from RTC (UTC)", timer.elapsed());
            file_logger.log(LogLevel::Ok, "Synchronized system clock from RTC (UTC)");
            Ok(())
        }
        Ok(s) => {
            let msg = format!("hwclock exited with non-zero status: {}", s);
            console_logger.message(LogLevel::Warn, &msg, timer.elapsed());
            file_logger.log(LogLevel::Warn, &msg);
            Err(BloomError::Custom(msg))
        }
        Err(e) => {
            let msg = format!("Failed to execute hwclock: {}", e);
            console_logger.message(LogLevel::Warn, &msg, timer.elapsed());
            file_logger.log(LogLevel::Warn, &msg);
            Err(BloomError::Io(e))
        }
    }
}

fn log_success(
    console_logger: &Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
    file_logger: &Arc<Mutex<dyn FileLogger + Send + Sync>>,
    timer: &ProcessTimer,
    level: LogLevel,
    message: &str,
) {
    let elapsed = timer.elapsed();
    if let Ok(mut con_log) = console_logger.lock() {
        con_log.message(level, message, elapsed);
    }
    if let Ok(mut file_log) = file_logger.lock() {
        file_log.log(level, message);
    }
}

fn log_error(
    console_logger: &Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
    file_logger: &Arc<Mutex<dyn FileLogger + Send + Sync>>,
    timer: &ProcessTimer,
    level: LogLevel,
    message: &str,
) {
    let elapsed = timer.elapsed();
    if let Ok(mut con_log) = console_logger.lock() {
        con_log.message(level, message, elapsed);
    }
    if let Ok(mut file_log) = file_logger.lock() {
        file_log.log(level, message);
    }
}

