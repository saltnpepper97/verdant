use std::fs;
use std::process::Command;

use common::{print_info_step, print_substep, print_substep_last, status_ok, status_skip, status_fail};

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
    print_info_step("Mounting essential filesystems...");

    let mounts = [
        ("proc", "/proc", "proc", vec![]),
        ("sysfs", "/sys", "sysfs", vec![]),
        ("devtmpfs", "/dev", "devtmpfs", vec![]),
        ("tmpfs", "/run", "tmpfs", vec!["-o", "mode=0755"]),
    ];

    let last_index = mounts.len() - 1;

    for (i, (source, target, fstype, flags)) in mounts.iter().enumerate() {
        let is_last = i == last_index;

        let _ = mount_fs(source, target, fstype, flags, is_last);
    }
}


