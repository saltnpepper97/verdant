use std::{fs, io};
use std::path::Path;
use std::process::{Command, Stdio};

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
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
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
                console_logger.message(LogLevel::Info, &msg, timer.elapsed());
                file_logger.log(LogLevel::Info, &msg);
                return Ok(());
            }
            Ok(false) => {
                // continue to spawn below
            }
            Err(e) => {
                let msg = format!("Failed to check existing processes: {}", e);
                console_logger.message(LogLevel::Warn, &msg, timer.elapsed());
                file_logger.log(LogLevel::Warn, &msg);
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
                console_logger.message(LogLevel::Ok, "Device manager started", timer.elapsed());
                file_logger.log(LogLevel::Info, &msg);
                Ok(())
            }
            Err(e) => {
                let msg = format!("Failed to start device manager daemon '{}': {:?}", dm_path, e);
                console_logger.message(LogLevel::Fail, "Device manager start failed", timer.elapsed());
                file_logger.log(LogLevel::Fail, &msg);
                Err(BloomError::Custom(msg))
            }
        }
    } else {
        let msg = "No device manager daemon found on system";
        console_logger.message(LogLevel::Warn, msg, timer.elapsed());
        file_logger.log(LogLevel::Warn, msg);
        Err(BloomError::Custom(msg.to_string()))
    }
}

pub fn monitor_udev_events(
    file_logger: &mut impl FileLogger,
) -> Result<(), BloomError> {

    let monitor = MonitorBuilder::new()
        .map_err(BloomError::from)?
        .listen()
        .map_err(BloomError::from)?;

    file_logger.log(LogLevel::Info, "Started udev event monitor");

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
        file_logger.log(LogLevel::Info, &msg);
    }

    Ok(())
}

fn is_process_running(name: &str) -> io::Result<bool> {
    // Scan /proc for any process whose cmdline contains `name`
    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        let path = entry.path();

        // Only check numeric dirs (process IDs)
        if let Some(pid_str) = path.file_name().and_then(|s| s.to_str()) {
            if pid_str.chars().all(|c| c.is_ascii_digit()) {
                // Read cmdline file
                let cmdline_path = path.join("cmdline");
                if let Ok(cmdline) = fs::read(cmdline_path) {
                    // cmdline is null-separated, so check if name is contained
                    if cmdline.windows(name.len()).any(|window| window == name.as_bytes()) {
                        return Ok(true);
                    }
                }
            }
        }
    }
    Ok(false)
}

