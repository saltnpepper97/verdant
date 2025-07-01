use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use wait_timeout::ChildExt;
use walkdir::WalkDir;

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;
use bloom::time::ProcessTimer;

pub fn load_hardware_drivers(
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
) -> Result<(), BloomError> {
    let timer = ProcessTimer::start();

    // depmod first so modaliases resolve
    if Path::new("/sbin/depmod").exists() {
        let _ = Command::new("/sbin/depmod")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    // collect modaliases from sysfs
    let mut aliases = HashSet::new();
    for entry in WalkDir::new("/sys/devices")
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_name() == "modalias")
    {
        if let Ok(file) = File::open(entry.path()) {
            let reader = BufReader::new(file);
            for line in reader.lines().flatten() {
                let alias = line.trim();
                if !alias.is_empty() {
                    aliases.insert(alias.to_string());
                }
            }
        }
    }

    if aliases.is_empty() {
        let msg = "No modalias entries found to load hardware drivers.";
        file_logger.log(LogLevel::Info, msg);
        console_logger.message(LogLevel::Warn, msg, timer.elapsed());
        return Ok(());
    }

    let timeout = Duration::from_secs(2);
    let mut loaded = 0;
    let mut failed = 0;

    for alias in &aliases {
        let mut cmd = Command::new("/sbin/modprobe");
        cmd.arg("-b").arg(alias);
        cmd.stdout(Stdio::null()).stderr(Stdio::null());

        match cmd.spawn() {
            Ok(mut child) => {
                match child.wait_timeout(timeout).unwrap() {
                    Some(status) if status.success() => loaded += 1,
                    _ => {
                        let _ = child.kill();
                        failed += 1;
                        let _ = file_logger.log(
                            LogLevel::Info,
                            &format!("modprobe timed out or failed for alias: {}", alias),
                        );
                    }
                }
            }
            Err(e) => {
                failed += 1;
                let _ = file_logger.log(
                    LogLevel::Info,
                    &format!("Failed to spawn modprobe for {}: {}", alias, e),
                );
            }
        }
    }

    let msg = format!("Loaded {} hardware modules ({} failed)", loaded, failed);
    file_logger.log(LogLevel::Info, &msg);
    console_logger.message(
        if loaded > 0 { LogLevel::Ok } else { LogLevel::Warn },
        &msg,
        timer.elapsed(),
    );

    Ok(())
}

