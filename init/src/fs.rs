use std::fs::{create_dir_all, File};
use std::io::{BufRead, BufReader};
use std::path::Path;

use nix::errno::Errno;
use nix::mount::{mount, MsFlags};

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;
use bloom::time::ProcessTimer;

/// Mounts standard Linux virtual filesystems: /proc, /sys, /dev, /run
pub fn mount_virtual_filesystems(
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
) -> Result<(), BloomError> {
    let timer = ProcessTimer::start();

    mount_fs(Some("proc"), "/proc", Some("proc"), MsFlags::empty(), None, "proc", console_logger, file_logger, &timer)?;
    mount_fs(Some("sysfs"), "/sys", Some("sysfs"), MsFlags::empty(), None, "sysfs", console_logger, file_logger, &timer)?;
    mount_fs(Some("devtmpfs"), "/dev", Some("devtmpfs"), MsFlags::empty(), None, "devtmpfs", console_logger, file_logger, &timer)?;
    mount_fs(Some("tmpfs"), "/run", Some("tmpfs"), MsFlags::empty(), Some("mode=755"), "tmpfs", console_logger, file_logger, &timer)?;

    // Ensure /run/lock exists
    ensure_dir("/run/lock", "runtime lock directory", console_logger, file_logger, &timer)?;

    // Ensure /run/verdant exists
    ensure_dir("/run/verdant", "Verdant runtime directory", console_logger, file_logger, &timer)?;

    Ok(())
}

/// Mount securityfs at /sys/kernel/security
pub fn mount_securityfs(
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
) -> Result<(), BloomError> {
    let timer = ProcessTimer::start();

    mount_fs(
        Some("securityfs"),
        "/sys/kernel/security",
        Some("securityfs"),
        MsFlags::empty(),
        None,
        "securityfs",
        console_logger,
        file_logger,
        &timer,
    )
}

/// Helper to mount a filesystem unless it's already mounted
pub fn mount_fs(
    source: Option<&str>,
    target: &str,
    fstype: Option<&str>,
    flags: MsFlags,
    data: Option<&str>,
    fs_name: &str,
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
    timer: &ProcessTimer,
) -> Result<(), BloomError> {
    if source == Some("none") || target == "none" {
        return Ok(()); // skip invalid/virtual-only
    }

    let target_path = Path::new(target);
    if !target_path.exists() {
        if let Err(e) = create_dir_all(target_path) {
            let msg = format!("Failed to create mount point {}: {}", target, e);
            log_error(console_logger, file_logger, timer, LogLevel::Fail, &msg);
            return Err(BloomError::Io(e));
        }
    }

    if is_mounted(target)? {
        log_success(console_logger, file_logger, timer, LogLevel::Info, &format!("{} already mounted at {}", fs_name, target));
        return Ok(());
    }

    // Pass mount data only for certain filesystem types (tmpfs, nfs, cifs, fuse)
    let supported_data_fs = ["tmpfs", "nfs", "cifs", "fuse"];
    let mount_data = match fstype {
        Some(fs) if supported_data_fs.contains(&fs) => data,
        _ => None,
    };

    match mount(source, target_path, fstype, flags, mount_data) {
        Ok(()) => {
            log_success(console_logger, file_logger, timer, LogLevel::Ok, &format!("Mounted {} at {}", fs_name, target));
            Ok(())
        }
        Err(e) if e == Errno::ENODEV => Ok(()), // ignore silently
        Err(e) => {
            let msg = format!("Failed to mount {} at {}: {}", fs_name, target, e);
            log_error(console_logger, file_logger, timer, LogLevel::Fail, &msg);
            Err(BloomError::Nix(e))
        }
    }
}


/// Check if the target is mounted by parsing `/proc/self/mountinfo`
fn is_mounted(target: &str) -> Result<bool, BloomError> {
    let file = File::open("/proc/self/mountinfo")?;
    for line in BufReader::new(file).lines() {
        let line = line?;
        if let Some(mount_point) = line.split_whitespace().nth(4) {
            if mount_point == target {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn ensure_dir(
    path: &str,
    desc: &str,
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
    timer: &ProcessTimer,
) -> Result<(), BloomError> {
    let p = Path::new(path);
    if !p.exists() {
        if let Err(e) = create_dir_all(p) {
            let msg = format!("Failed to create {} at {}: {}", desc, path, e);
            log_error(console_logger, file_logger, timer, LogLevel::Fail, &msg);
            return Err(BloomError::Io(e));
        }
        let msg = format!("Created {}", path);
        log_success(console_logger, file_logger, timer, LogLevel::Info, &msg);
    } else {
        let msg = format!("{} already exists", path);
        log_success(console_logger, file_logger, timer, LogLevel::Info, &msg);
    }
    Ok(())
}

fn log_success(
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
    timer: &ProcessTimer,
    level: LogLevel,
    msg: &str,
) {
    let elapsed = timer.elapsed();
    console_logger.message(level, msg, elapsed);
    file_logger.log(level, msg);
}

fn log_error(
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
    timer: &ProcessTimer,
    level: LogLevel,
    msg: &str,
) {
    let elapsed = timer.elapsed();
    console_logger.message(level, msg, elapsed);
    file_logger.log(level, msg);
}

