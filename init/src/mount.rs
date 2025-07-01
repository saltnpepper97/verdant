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

    for line_result in BufReader::new(fstab).lines() {
        let line = line_result.map_err(BloomError::Io)?.trim().to_string();

        // Skip empty or comment lines
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 4 {
            log_success(console_logger, file_logger, &timer, LogLevel::Warn, &format!("Skipping invalid fstab line: {}", line));
            continue;
        }

        let raw_source = fields[0];
        let target = fields[1];
        let fstype = fields[2];
        let options = fields[3];

        // Skip root "/", already mounted
        if target == "/" {
            continue;
        }

        // Skip "none" or invalid mount points (non-absolute)
        if target == "none" || !Path::new(target).is_absolute() {
            continue;
        }

        // Skip if "noauto" present in options
        if options.split(',').any(|opt| opt == "noauto") {
            log_success(console_logger, file_logger, &timer, LogLevel::Info, &format!("Skipping noauto mount: {}", target));
            continue;
        }

        // Ensure mount point exists
        let target_path = Path::new(target);
        if !target_path.exists() {
            match fs::create_dir_all(target_path) {
                Ok(_) => log_success(console_logger, file_logger, &timer, LogLevel::Info, &format!("Created mount point {}", target)),
                Err(e) => {
                    log_error(console_logger, file_logger, &timer, LogLevel::Warn, &format!("Failed to create mount point {}: {}", target, e));
                    continue;
                }
            }
        }

        // Resolve the device source path (UUID=, LABEL=, or absolute)
        let source = match resolve_source(raw_source) {
            Ok(dev) => dev,
            Err(e) => {
                log_error(console_logger, file_logger, &timer, LogLevel::Warn, &format!("Could not resolve source '{}': {}", raw_source, e));
                continue;
            }
        };

        // Log resolved source for debug
        log_success(console_logger, file_logger, &timer, LogLevel::Info, &format!("Mounting {} -> {} as {}", source, target, fstype));

        // Split mount options into flags and data string
        let (flags, data) = split_mount_options(options);

        // Attempt the mount syscall with correct types
        if let Err(e) = crate::fs::mount_fs(
            Some(&source),
            target,
            Some(fstype),
            flags,
            data.as_deref(),
            &format!("fstab entry {}", target),
            console_logger,
            file_logger,
            &timer,
        ) {
            let err_str = e.to_string();
            if err_str.contains("EINVAL") || err_str.contains("ENOENT") {
                log_error(console_logger, file_logger, &timer, LogLevel::Warn, &format!("Skipped mount {}: {}", target, e));
            } else {
                log_error(console_logger, file_logger, &timer, LogLevel::Fail, &format!("Failed to mount {}: {}", target, e));
            }
        }
    }

    Ok(())
}


/// Resolve UUID= or LABEL= sources to device paths
/// For pseudo-filesystems like tmpfs, proc, etc., return as-is.
fn resolve_source(source: &str) -> Result<String, BloomError> {
    // Pseudo-filesystems should be passed through untouched
    let pseudo_fs = ["tmpfs", "proc", "sysfs", "devpts", "devtmpfs", "cgroup", "cgroup2"];
    if pseudo_fs.contains(&source) {
        return Ok(source.to_string());
    }

    if let Some(uuid) = source.strip_prefix("UUID=") {
        resolve_symlink_target("/dev/disk/by-uuid", uuid)
    } else if let Some(label) = source.strip_prefix("LABEL=") {
        resolve_symlink_target("/dev/disk/by-label", label)
    } else {
        // Normal device path (e.g. /dev/sda1)
        let path = Path::new(source);
        if path.exists() {
            Ok(source.to_string())
        } else {
            Err(BloomError::Custom(format!("Device {} does not exist", source)))
        }
    }
}


fn resolve_symlink_target(base_dir: &str, name: &str) -> Result<String, BloomError> {
    let path = Path::new(base_dir).join(name);
    if !path.exists() {
        return Err(BloomError::Custom(format!("{} does not exist", path.display())));
    }

    let target = fs::read_link(&path)
        .map_err(|e| BloomError::Custom(format!("Failed to read symlink {}: {}", path.display(), e)))?;

    let full_path = if target.is_absolute() {
        target
    } else {
        path.parent().unwrap_or(Path::new("/")).join(target)
    };

    let canonical = fs::canonicalize(&full_path)
        .map_err(|e| BloomError::Custom(format!("Failed to canonicalize {}: {}", full_path.display(), e)))?;

    if canonical.exists() {
        Ok(canonical.to_string_lossy().to_string())
    } else {
        Err(BloomError::Custom(format!("Resolved device {} does not exist", canonical.display())))
    }
}

/// Helper: split mount options into MsFlags and data string for mount syscall
fn split_mount_options(options: &str) -> (MsFlags, Option<String>) {
    let mut flags = MsFlags::empty();
    let mut data_opts = Vec::new();

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
            other => data_opts.push(other),
        }
    }

    let data = if data_opts.is_empty() {
        None
    } else {
        Some(data_opts.join(","))
    };

    (flags, data)
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

