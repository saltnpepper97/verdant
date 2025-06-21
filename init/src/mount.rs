use std::fs;
use std::process::{Command, Stdio};
use std::path::Path;

use common::{print_step, print_info_step, print_substep, print_substep_last, status_ok, status_skip, status_fail};

fn find_boot_by_label() -> Option<String> {
    let candidates = ["BOOT", "boot", "EFI", "efi"];

    for label in &candidates {
        let path_str = format!("/dev/disk/by-label/{}", label);
        let path = Path::new(&path_str);
        if path.exists() {
            if let Ok(real_path) = fs::read_link(path) {
                // real_path is relative, so join with /dev/disk/by-label
                let full_path = path.parent().unwrap().join(real_path);
                if let Some(dev_path) = full_path.to_str() {
                    return Some(dev_path.to_string());
                }
            }
        }
    }
    None
}

pub fn mount_boot_by_label() -> Result<(), String> {
    if let Some(device) = find_boot_by_label() {
        // Try common boot filesystems
        let fstype_attempts = ["vfat", "ext4", "ext3", "ext2"];
        for fstype in &fstype_attempts {
            let status = Command::new("mount")
                .args(["-t", fstype])
                .arg(&device)
                .arg("/boot")
                .status()
                .map_err(|e| e.to_string())?;

            if status.success() {
                print_step("Boot Partition found mounting", &status_ok());
                return Ok(());
            }
        }
        Err(format!("Failed to mount {} as known boot fs types", device))
    } else {
        Err("No /boot partition found by label".into())
    }
}

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
