use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;
use bloom::time::ProcessTimer;

use walkdir::WalkDir;

pub fn load_hardware_drivers(
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
) -> Result<(), BloomError> {
    let timer = ProcessTimer::start();

    let uevent_helper = "/bin/echo";
    let uevent_path = "/proc/sys/kernel/hotplug";

    if Path::new(uevent_path).exists() {
        let _ = fs::write(uevent_path, uevent_helper);
    }

    if Path::new("/sbin/depmod").exists() {
        let _ = Command::new("/sbin/depmod")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    let mut triggered = 0;
    for entry in WalkDir::new("/sys/devices")
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_name() == "uevent")
    {
        let path = entry.path();
        if let Ok(mut file) = OpenOptions::new().write(true).open(path) {
            if file.write_all(b"add\n").is_ok() {
                triggered += 1;
            }
        }
    }

    if triggered > 0 {
        let msg = format!("Triggered {} kernel uevents to load drivers", triggered);
        file_logger.log(LogLevel::Info, &msg);
        console_logger.message(LogLevel::Ok, &msg, timer.elapsed());
        Ok(())
    } else {
        let msg = "No uevents could be triggered (nothing written to /sys/.../uevent)";
        file_logger.log(LogLevel::Warn, msg);
        console_logger.message(LogLevel::Warn, msg, timer.elapsed());
        Err(BloomError::Custom(msg.into()))
    }
}

