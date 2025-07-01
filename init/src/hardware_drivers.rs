use std::process::{Command, Stdio};
use std::path::Path;
use std::time::Duration;

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;
use bloom::time::ProcessTimer;

pub fn load_hardware_drivers(
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
) -> Result<(), BloomError> {
    let timer = ProcessTimer::start();

    let uevent_helper = "/bin/echo"; // neutralize blocking helper if needed
    let uevent_path = "/proc/sys/kernel/hotplug";

    // Prevent legacy hotplug helpers from interfering
    if Path::new(uevent_path).exists() {
        let _ = std::fs::write(uevent_path, uevent_helper);
    }

    // Best-effort depmod
    if Path::new("/sbin/depmod").exists() {
        let _ = Command::new("/sbin/depmod")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    // Walk /sys/devices and re-trigger uevents
    let result = Command::new("find")
        .arg("/sys/devices/")
        .arg("-name")
        .arg("uevent")
        .stdout(Stdio::piped())
        .spawn()
        .and_then(|find| {
            let xargs = Command::new("xargs")
                .arg("-n1")
                .arg("-I{}")
                .arg("sh")
                .arg("-c")
                .arg("echo add > {}")
                .stdin(find.stdout.unwrap())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            xargs
        });

    match result {
        Ok(status) if status.success() => {
            let msg = "Re-triggered uevents to load hardware modules.";
            file_logger.log(LogLevel::Info, msg);
            console_logger.message(LogLevel::Ok, msg, timer.elapsed());
            Ok(())
        }
        _ => {
            let msg = "Failed to trigger kernel uevents for hardware driver loading.";
            file_logger.log(LogLevel::Warn, msg);
            console_logger.message(LogLevel::Warn, msg, timer.elapsed());
            Err(BloomError::Custom(msg.into()))
        }
    }
}

