use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::ffi::CString;
use std::process::{Command, Stdio};

use bloom::status::LogLevel;
use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::time::ProcessTimer;

pub fn set_hostname(
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
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
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
) -> Result<Option<String>, BloomError> {
    let timer = ProcessTimer::start();
    let localtime_path = Path::new("/etc/localtime");

    let link_target = match fs::read_link(localtime_path) {
        Ok(target) => target,
        Err(e) => {
            log_error(console_logger, file_logger, &timer, LogLevel::Warn, &format!("Could not read /etc/localtime symlink: {}", e));
            return Ok(None);
        }
    };

    let zoneinfo_prefix = Path::new("/usr/share/zoneinfo/");
    if !link_target.starts_with(zoneinfo_prefix) {
        log_error(console_logger, file_logger, &timer, LogLevel::Warn, &format!("/etc/localtime symlink does not point inside zoneinfo: {:?}", link_target));
        return Ok(None);
    }

    let tz_path: PathBuf = link_target.strip_prefix(zoneinfo_prefix).unwrap().to_path_buf();

    match tz_path.to_str() {
        Some(tz_str) => {
            log_success(console_logger, file_logger, &timer, LogLevel::Ok, &format!("Detected timezone '{}'", tz_str));
            Ok(Some(tz_str.to_string()))
        }
        None => {
            log_error(console_logger, file_logger, &timer, LogLevel::Fail, "Failed to convert timezone path to string");
            Ok(None)
        }
    }
}



pub fn sync_clock_from_hardware(
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
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
            log_success(console_logger, file_logger, &timer, LogLevel::Ok, "Synchronized system clock from RTC (UTC)");
            Ok(())
        }
        Ok(s) => {
            let msg = format!("hwclock exited with non-zero status: {}", s);
            log_error(console_logger, file_logger, &timer, LogLevel::Warn, &msg);
            Err(BloomError::Custom(msg))
        }
        Err(e) => {
            let msg = format!("Failed to execute hwclock: {}", e);
            log_error(console_logger, file_logger, &timer, LogLevel::Warn, &msg);
            Err(BloomError::Io(e))
        }
    }
}

fn log_success(
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
    timer: &ProcessTimer,
    level: LogLevel,
    message: &str,
) {
    let elapsed = timer.elapsed();
    console_logger.message(level, message, elapsed);
    file_logger.log(level, message);
}

fn log_error(
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
    timer: &ProcessTimer,
    level: LogLevel,
    message: &str,
) {
    let elapsed = timer.elapsed();
    console_logger.message(level, message, elapsed);
    file_logger.log(level, message);
}

