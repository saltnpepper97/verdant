use std::fs;
use std::process::{Command, Stdio};
use std::path::Path;

use common::{print_info_step, print_step, print_substep, print_substep_last, status_fail, status_ok, status_skip, status_warn};

fn is_mounted(target: &str) -> bool {
    if let Ok(mounts) = fs::read_to_string("/proc/mounts") {
        mounts.lines().any(|line| line.contains(&format!(" {}", target)))
    } else {
        false
    }
}

fn find_all_block_devices() -> Vec<String> {
    let mut devices = Vec::new();

    if let Ok(entries) = fs::read_dir("/dev") {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();

            // Match known block device patterns (skip loop, ram, etc.)
            if name.starts_with("sd") || name.starts_with("vd") || name.starts_with("nvme") {
                // Skip whole disks (like sda or nvme0n1) — only try partitions
                if name.chars().any(|c| c.is_digit(10)) {
                    let path = format!("/dev/{}", name);
                    devices.push(path);
                }
            }
        }
    }

    devices
}

fn try_mount_boot_partition(dev: &str) -> bool {
    // Try mounting the partition
    let status = Command::new("mount")
        .args(["-t", "auto", dev, "/boot"])
        .stdout(Stdio::null())
        .stderr(Stdio::null()) 
        .status();

    if let Ok(s) = status {
        if s.success() {
            // Check if it looks like a /boot partition (has extlinux.conf or grub, etc.)
            let looks_valid = Path::new("/boot/extlinux.conf").exists()
                || Path::new("/boot/grub").exists()
                || Path::new("/boot/vmlinuz").exists();

            if looks_valid {
                return true;
            } else {
                // Not the right partition; unmount it
                let _ = Command::new("umount").arg("/boot").status();
            }
        }
    }

    false
}

pub fn mount_boot_partition() {
    let devices = find_all_block_devices();

    for dev in devices {
        if try_mount_boot_partition(&dev) {
            return; // Successfully mounted
        }
    }

    print_step("No suitable /boot partition found.", &status_warn());
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
