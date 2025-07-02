use std::{fs, io};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;
use bloom::time::ProcessTimer;

use udev::{MonitorBuilder, EventType};

fn detect_device_manager() -> Option<&'static str> {
    let candidates = [
        "/usr/lib/systemd/systemd-udevd",
        "/sbin/udevd",
        "/bin/udevd",
        "/usr/bin/udevd",
        "/sbin/mdev",
        "/bin/mdev",
        "/usr/bin/mdev",
    ];

    for &path in &candidates {
        if Path::new(path).exists() {
            return Some(path);
        }
    }
    None
}

pub fn start_device_manager(
    console_logger: &Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
    file_logger: &Arc<Mutex<dyn FileLogger + Send + Sync>>,
) -> Result<(), BloomError> {
    let timer = ProcessTimer::start();

    if let Some(dm_path) = detect_device_manager() {
        let dm_name = Path::new(dm_path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        match is_process_running(dm_name) {
            Ok(true) => {
                let msg = format!("Device manager '{}' already running.", dm_name);
                if let Ok(mut con_log) = console_logger.lock() {
                    con_log.message(LogLevel::Info, &msg, timer.elapsed());
                }
                if let Ok(mut file_log) = file_logger.lock() {
                    file_log.log(LogLevel::Info, &msg);
                }
                return Ok(());
            }
            Ok(false) => {
                // continue to spawn below
            }
            Err(e) => {
                let msg = format!("Failed to check existing processes: {}", e);
                if let Ok(mut con_log) = console_logger.lock() {
                    con_log.message(LogLevel::Warn, &msg, timer.elapsed());
                }
                if let Ok(mut file_log) = file_logger.lock() {
                    file_log.log(LogLevel::Warn, &msg);
                }
            }
        }

        let mut cmd = Command::new(dm_path);
        if dm_path.ends_with("mdev") {
            cmd.arg("-s");
        } else {
            cmd.arg("--daemon");
        }

        let child_res = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();

        match child_res {
            Ok(_) => {
                let msg = format!("Started device manager daemon: {}", dm_path);
                if let Ok(mut con_log) = console_logger.lock() {
                    con_log.message(LogLevel::Ok, "Device manager started", timer.elapsed());
                }
                if let Ok(mut file_log) = file_logger.lock() {
                    file_log.log(LogLevel::Info, &msg);
                }
                Ok(())
            }
            Err(e) => {
                let msg = format!("Failed to start device manager daemon '{}': {:?}", dm_path, e);
                if let Ok(mut con_log) = console_logger.lock() {
                    con_log.message(LogLevel::Fail, "Device manager start failed", timer.elapsed());
                }
                if let Ok(mut file_log) = file_logger.lock() {
                    file_log.log(LogLevel::Fail, &msg);
                }
                Err(BloomError::Custom(msg))
            }
        }
    } else {
        let msg = "No device manager daemon found on system";
        if let Ok(mut con_log) = console_logger.lock() {
            con_log.message(LogLevel::Warn, msg, timer.elapsed());
        }
        if let Ok(mut file_log) = file_logger.lock() {
            file_log.log(LogLevel::Warn, msg);
        }
        Err(BloomError::Custom(msg.to_string()))
    }
}

pub fn monitor_udev_events(
    file_logger: &Arc<Mutex<dyn FileLogger + Send + Sync>>,
) -> Result<(), BloomError> {

    let monitor = MonitorBuilder::new()
        .map_err(BloomError::from)?
        .listen()
        .map_err(BloomError::from)?;

    if let Ok(mut file_log) = file_logger.lock() {
        file_log.log(LogLevel::Info, "Started udev event monitor");
    }

    for event in monitor.iter() {
        let evtype = match event.event_type() {
            EventType::Add => "add",
            EventType::Remove => "remove",
            EventType::Change => "change",
            _ => "unknown",
        };

        let devnode = event.devnode()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<no devnode>".to_string());

        let msg = format!("udev event: {} on device {}", evtype, devnode);
        if let Ok(mut file_log) = file_logger.lock() {
            file_log.log(LogLevel::Info, &msg);
        }
    }

    Ok(())
}

fn is_process_running(name: &str) -> io::Result<bool> {
    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        let path = entry.path();

        if let Some(pid_str) = path.file_name().and_then(|s| s.to_str()) {
            if pid_str.chars().all(|c| c.is_ascii_digit()) {
                let cmdline_path = path.join("cmdline");
                if let Ok(cmdline) = fs::read(cmdline_path) {
                    if cmdline.windows(name.len()).any(|window| window == name.as_bytes()) {
                        return Ok(true);
                    }
                }
            }
        }
    }
    Ok(false)
}

