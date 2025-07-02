use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use nix::mount::umount;

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;
use bloom::time::ProcessTimer;

/// Unmount all filesystems listed in /etc/fstab, except the root `/`
pub fn unmount_fstab_filesystems(
    console_logger: &mut dyn ConsoleLogger,
    file_logger: &mut dyn FileLogger,
) -> Result<(), BloomError> {
    let timer = ProcessTimer::start();

    let file = File::open("/etc/fstab").map_err(BloomError::Io)?;
    let mut mount_points = Vec::new();

    for line_result in BufReader::new(file).lines() {
        let line = line_result.map_err(BloomError::Io)?.trim().to_string();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            continue;
        }

        let target = fields[1];
        let options = fields[3];

        if target == "/" || target == "none" || !Path::new(target).is_absolute() {
            continue;
        }

        if options.split(',').any(|opt| opt == "noauto") {
            continue;
        }

        mount_points.push(target.to_string());
    }

    // Sort by descending path length to unmount deeper mounts first
    mount_points.sort_by(|a, b| b.len().cmp(&a.len()));

    for mount_point in mount_points {
        let path = Path::new(&mount_point);
        match umount(path) {
            Ok(()) => {
                let msg = format!("Unmounted {}", mount_point);
                log_success(console_logger, file_logger, &timer, LogLevel::Ok, &msg);
            }
            Err(e) => {
                let msg = format!("Failed to unmount {}: {}", mount_point, e);
                log_error(console_logger, file_logger, &timer, LogLevel::Warn, &msg);
            }
        }
    }

    Ok(())
}

fn log_success(
    console_logger: &mut dyn ConsoleLogger,
    file_logger: &mut dyn FileLogger,
    timer: &ProcessTimer,
    level: LogLevel,
    msg: &str,
) {
    let elapsed = timer.elapsed();
    console_logger.message(level, msg, elapsed);
    file_logger.log(level, msg);
}

fn log_error(
    console_logger: &mut dyn ConsoleLogger,
    file_logger: &mut dyn FileLogger,
    timer: &ProcessTimer,
    level: LogLevel,
    msg: &str,
) {
    let elapsed = timer.elapsed();
    console_logger.message(level, msg, elapsed);
    file_logger.log(level, msg);
}
