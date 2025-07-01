use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::Path;

use nix::mount::{mount, MsFlags};
use nix::sys::statvfs::statvfs;

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;
use bloom::time::ProcessTimer;

/// Check if root `/` is read-only and remount as read-write if needed.
pub fn remount_root(
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
) -> Result<(), BloomError> {
    let timer = ProcessTimer::start();

    if is_root_readonly()? {
        mount(
            Some(Path::new("/")),
            Path::new("/"),
            None::<&Path>,
            MsFlags::MS_REMOUNT,
            None::<&str>,
        )
        .map_err(BloomError::Nix)?;

        log_success(console_logger, file_logger, &timer, LogLevel::Ok, "Remounted root as read-write");
    } else {
        log_success(console_logger, file_logger, &timer, LogLevel::Info, "root already mounted read-write");
    }

    Ok(())
}

/// Parse `/proc/mounts` to check if `/` is mounted read-only.
fn is_root_readonly() -> Result<bool, BloomError> {
    let file = File::open("/proc/mounts")?;
    for line in BufReader::new(file).lines() {
        let line = line?;
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 4 && fields[1] == "/" {
            return Ok(fields[3].split(',').any(|opt| opt == "ro"));
        }
    }
    Ok(false)
}

/// Mount entries in /etc/fstab except the root `/`.
pub fn mount_fstab_filesystems(
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
) -> Result<(), BloomError> {
    let timer = ProcessTimer::start();
    let fstab = File::open("/etc/fstab").map_err(BloomError::Io)?;

    for line in BufReader::new(fstab).lines() {
        let line = line.map_err(BloomError::Io)?.trim().to_string();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            log_success(console_logger, file_logger, &timer, LogLevel::Warn, &format!("Skipping invalid fstab line: {}", line));
            continue;
        }

        let source = fields[0];
        let target = fields[1];
        let fstype = fields[2];
        let options = fields[3];

        // Skip mounting if 'noauto' is present
        if options.split(',').any(|opt| opt == "noauto") {
            log_success(console_logger, file_logger, &timer, LogLevel::Info, &format!("Skipping noauto mount: {}", target));
            continue;
        }

        // Skip root (already mounted)
        if target == "/" {
            continue;
        }

        // Skip bogus mount point
        if target == "none" || !Path::new(target).is_absolute() {
            continue;
        }

        // Ensure mount point exists
        let target_path = Path::new(target);
        if !target_path.exists() {
            if let Err(e) = fs::create_dir_all(target_path) {
                log_error(console_logger, file_logger, &timer, LogLevel::Warn, &format!("Failed to create mount point {}: {}", target, e));
                continue;
            }
        }

        // Skip if source device does not exist (for media devices)
        if source.starts_with("/dev/") && !Path::new(source).exists() {
            log_error(console_logger, file_logger, &timer, LogLevel::Warn, &format!(
                "Device {} not found, skipping mount of {}", source, target));
            continue;
        }

        let flags = parse_mount_flags(options);

        if let Err(e) = crate::fs::mount_fs(
            Some(source),
            target,
            Some(fstype),
            flags,
            Some(options),
            &format!("fstab entry {}", target),
            console_logger,
            file_logger,
            &timer,
        ) {
            let kind = e.to_string();
            if kind.contains("EINVAL") || kind.contains("ENOENT") {
                log_error(console_logger, file_logger, &timer, LogLevel::Warn, &format!("Skipped mount {}: {}", target, e));
            } else {
                log_error(console_logger, file_logger, &timer, LogLevel::Fail, &format!("Failed to mount {}: {}", target, e));
            }
        }
    }

    Ok(())
}

fn parse_mount_flags(options: &str) -> MsFlags {
    let mut flags = MsFlags::empty();
    for opt in options.split(',') {
        match opt {
            "ro" => flags |= MsFlags::MS_RDONLY,
            "rw" => flags &= !MsFlags::MS_RDONLY,
            "noexec" => flags |= MsFlags::MS_NOEXEC,
            "nosuid" => flags |= MsFlags::MS_NOSUID,
            "nodev" => flags |= MsFlags::MS_NODEV,
            "relatime" => flags |= MsFlags::MS_RELATIME,
            "nodiratime" => flags |= MsFlags::MS_NODIRATIME,
            "sync" => flags |= MsFlags::MS_SYNCHRONOUS,
            _ => (),
        }
    }
    flags
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

pub fn check_filesystem_health(
    console_logger: &mut impl ConsoleLogger,
    file_logger: &mut impl FileLogger,
) -> Result<(), BloomError> {
    let timer = ProcessTimer::start();
    let fstab_path = "/etc/fstab";
    let file = fs::File::open(fstab_path).map_err(|e| {
        BloomError::Custom(format!("Failed to open /etc/fstab: {}", e))
    })?;
    let reader = std::io::BufReader::new(file);

    let ignore_fs_types = [
        "proc", "sysfs", "tmpfs", "devtmpfs", "devpts", "cgroup", "cgroup2", "debugfs", "securityfs",
        "pstore", "efivarfs", "mqueue", "hugetlbfs", "configfs", "fusectl", "tracefs", "bpf", "ramfs",
        "overlay", "aufs", "squashfs", "autofs", "none",
    ];

    for (line_num, line_result) in reader.lines().enumerate() {
        let line = line_result.map_err(|e| {
            BloomError::Custom(format!("Error reading /etc/fstab line {}: {}", line_num + 1, e))
        })?;

        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }

        let source = parts[0];
        let mount_point = parts[1];
        let fs_type = parts[2];

        if ignore_fs_types.contains(&fs_type)
            || ignore_fs_types.contains(&source)
            || mount_point == "none"
        {
            continue;
        }

        let path = Path::new(mount_point);

        if !path.exists() {
            let msg = format!("Mount point '{}' does not exist", mount_point);
            log_error(console_logger, file_logger, &timer, LogLevel::Fail, &msg);
            return Err(BloomError::Custom(msg));
        }

        if !path.is_dir() {
            let msg = format!("Mount point '{}' is not a directory", mount_point);
            log_error(console_logger, file_logger, &timer, LogLevel::Fail, &msg);
            return Err(BloomError::Custom(msg));
        }

        if let Err(e) = fs::read_dir(path) {
            let msg = format!("Cannot read mount point '{}': {}", mount_point, e);
            log_error(console_logger, file_logger, &timer, LogLevel::Fail, &msg);
            return Err(BloomError::Custom(msg));
        }

        if let Err(e) = statvfs(path) {
            let msg = format!("statvfs failed for '{}': {}", mount_point, e);
            log_error(console_logger, file_logger, &timer, LogLevel::Fail, &msg);
            return Err(BloomError::Custom(msg));
        }

        let msg = format!("Filesystem '{}' is healthy", mount_point);
        log_success(console_logger, file_logger, &timer, LogLevel::Ok, &msg);
    }

    Ok(())
}

