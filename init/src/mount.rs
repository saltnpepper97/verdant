use std::fs;
use std::process::{Command, Stdio};
use std::path::Path;

use common::{print_step, print_info_step, print_substep, print_substep_last, status_ok, status_skip, status_fail};

fn is_mounted(target: &str) -> bool {
    if let Ok(mounts) = fs::read_to_string("/proc/mounts") {
        mounts.lines().any(|line| line.contains(&format!(" {}", target)))
    } else {
        false
    }
}

fn mount_fs(source: &str, target: &str, fstype: &str, flags: &[&str], is_last: bool) -> Result<(), String> {
    if is_mounted(target) {
        if is_last {
            print_substep_last(&format!("{} is already mounted", target), &status_skip());
        } else {
            print_substep(&format!("{} is already mounted", target), &status_skip());
        }
        return Ok(());
    }

    let status = Command::new("mount")
        .args(["-t", fstype])
        .args(flags)
        .arg(source)
        .arg(target)
        .status()
        .map_err(|e| e.to_string())?;

    if status.success() {
        if is_last {
            print_substep_last(&format!("Mounted {}", target), &status_ok());
        } else {
            print_substep(&format!("Mounted {}", target), &status_ok());
        }
        Ok(())
    } else {
        if is_last {
            print_substep_last(&format!("Failed to mount {}", target), &status_fail());
        } else {
            print_substep(&format!("Failed to mount {}", target), &status_fail());
        }
        Err(format!("Failed to mount {}", target))
    }
}

pub fn mount_essential() {
    print_info_step("Mounting essential filesystems ...");

    let mounts = [
        ("proc", "/proc", "proc", vec![]),
        ("sysfs", "/sys", "sysfs", vec![]),
        ("devtmpfs", "/dev", "devtmpfs", vec![]),
        ("devpts", "/dev/pts", "devpts", vec![]),
        ("tmpfs", "/run", "tmpfs", vec!["-o", "mode=0755"]),
        ("tmpfs", "/dev/shm", "tmpfs", vec!["-o", "mode=1777"]),
        ("tmpfs", "/tmp", "tmpfs", vec!["-o", "mode=1777"]),
    ];

    let last_index = mounts.len() - 1;

    for (i, (source, target, fstype, flags)) in mounts.iter().enumerate() {
        let is_last = i == last_index;

        let _ = mount_fs(source, target, fstype, flags, is_last);
    }
}

/// Remounts the root filesystem as read/write
pub fn remount_root_rw() {
    let status = Command::new("mount")
        .args(&["-o", "remount,rw", "/"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    match status {
        Ok(s) if s.success() => {
            print_step("Remounted root filesystem as read/write", &status_ok());
        }
        Ok(s) => {
            print_step(
                &format!("mount returned non-zero status: {}", s),
                &status_fail(),
            );
        }
        Err(e) => {
            print_step(&format!("Failed to remount root filesystem: {}", e), &status_fail());
        }
    }
}

/// Mounts local filesystems listed in /etc/fstab (like /boot)
pub fn mount_local_filesystems() {
    use std::fs::read_to_string;

    print_info_step("Mounting local filesystems ...");

    let fstab = match read_to_string("/etc/fstab") {
        Ok(s) => s,
        Err(e) => {
            print_step(&format!("Failed to read /etc/fstab: {}", e), &status_fail());
            return;
        }
    };

    let mut entries = vec![];

    for line in fstab.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }

        let source = parts[0];
        let target = parts[1];
        let fstype = parts[2];
        let options = parts[3];

        // Skip ignored types
        if fstype == "swap" || options.contains("noauto") {
            continue;
        }

        // Skip pseudo-filesystems already mounted in mount_essential
        if ["/", "/proc", "/sys", "/dev", "/run", "/dev/pts", "/dev/shm", "/tmp"].contains(&target) {
            continue;
        }

        let flags: Vec<&str> = vec!["-o", options];

        entries.push((source.to_string(), target.to_string(), fstype.to_string(), flags));
    }

    let last_index = entries.len().saturating_sub(1);

    for (i, (source, target, fstype, flags)) in entries.into_iter().enumerate() {
        let is_last = i == last_index;
        let _ = mount_fs(&source, &target, &fstype, &flags, is_last);
    }
}
