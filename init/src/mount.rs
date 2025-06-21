use std::fs;
use std::process::{Command, Stdio};
use std::path::Path;

use common::{print_step, print_info_step, print_substep, print_substep_last, status_ok, status_skip, status_fail};

fn find_boot_partition() -> Option<String> {
    // First try by-label (most reliable if available)
    let candidates = ["BOOT", "boot", "EFI", "efi"];
    for label in &candidates {
        let path = format!("/dev/disk/by-label/{}", label);
        if Path::new(&path).exists() {
            if let Ok(real_path) = fs::read_link(&path) {
                return Some(format!("/dev/{}", real_path.display()));
            }
        }
    }

    // Try parsing lsblk for a partition with mountpoint "/boot" or label "BOOT"
    if let Ok(output) = Command::new("lsblk")
        .args(&["-no", "NAME,LABEL,MOUNTPOINT"])
        .output()
    {
        if let Ok(stdout) = String::from_utf8(output.stdout) {
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 1 {
                    let name = parts[0];
                    let label = parts.get(1).copied().unwrap_or("");
                    let mountpoint = parts.get(2).copied().unwrap_or("");

                    if mountpoint == "/boot" {
                        return Some(format!("/dev/{}", name));
                    }

                    if label.eq_ignore_ascii_case("boot") || label.eq_ignore_ascii_case("efi") {
                        return Some(format!("/dev/{}", name));
                    }
                }
            }
        }
    }

    // Try fallback candidates for common devices (dangerous if too broad)
    for dev in &["/dev/sda1", "/dev/vda1", "/dev/mmcblk0p1"] {
        if Path::new(dev).exists() {
            return Some(dev.to_string());
        }
    }

    None
}

pub fn mount_boot_partition() -> Result<(), String> {
    if is_mounted("/boot") {
        print_step("/boot is already mounted", &status_skip());
        return Ok(());
    }

    if let Some(device) = find_boot_partition() {
        let fs_types = ["vfat", "ext4", "ext3", "ext2"];
        for fstype in &fs_types {
            let status = Command::new("mount")
                .args(["-t", fstype])
                .arg(&device)
                .arg("/boot")
                .status()
                .map_err(|e| e.to_string())?;

            if status.success() {
                print_step("Mounted /boot partition", &status_ok());
                return Ok(());
            }
        }

        Err(format!("Found {} but could not mount with known FS types", device))
    } else {
        Err("No boot partition found using label, lsblk, or fallback paths".into())
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
