use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
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

    // Run depmod early
    if Path::new("/sbin/depmod").exists() {
        let _ = Command::new("/sbin/depmod")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    // Use BTreeSet to keep aliases sorted (predictable load order)
    let mut aliases = BTreeSet::new();

    // Only look in /sys/devices, filter broken paths
    for entry in WalkDir::new("/sys/devices")
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_name() == "modalias")
    {
        let path: PathBuf = entry.path().to_path_buf();

        // Avoid hanging on special files or sockets
        if let Ok(metadata) = fs::metadata(&path) {
            if !metadata.is_file() {
                continue;
            }
        }

        if let Ok(file) = File::open(&path) {
            let reader = BufReader::new(file);
            for line in reader.lines().flatten() {
                let alias = line.trim();
                if !alias.is_empty() && alias.len() < 256 {
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

    for alias in aliases {
        let mut cmd = Command::new("/sbin/modprobe");
        cmd.arg("-b").arg(&alias);
        cmd.stdout(Stdio::null()).stderr(Stdio::null());

        match cmd.spawn() {
            Ok(mut child) => {
                match child.wait_timeout(timeout).unwrap_or(None) {
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

