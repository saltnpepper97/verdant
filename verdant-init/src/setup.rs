use std::fs::{self, Permissions, File};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

use common::{print_step, status_fail, status_ok};

/// Create /run/verdant with appropriate permissions
pub fn setup_runtime_dirs() {
    let path = "/run/verdant";
    if let Err(e) = fs::create_dir_all(path) {
        print_step(&format!("Failed to create directory {}: {}", path, e), &status_fail());
    } else if let Err(e) = fs::set_permissions(path, Permissions::from_mode(0o755)) {
        print_step(&format!("Failed to set permissions on {}: {}", path, e), &status_fail());
    } else {
        print_step(&format!("Created directory {}", path), &status_ok());
    }
}

/// Create /run/lock with appropriate permissions
pub fn setup_lock_dir() {
    let path = "/run/lock";
    if let Err(e) = fs::create_dir_all(path) {
        print_step(&format!("Failed to create directory {}: {}", path, e), &status_fail());
    } else if let Err(e) = fs::set_permissions(path, Permissions::from_mode(0o755)) {
        print_step(&format!("Failed to set permissions on {}: {}", path, e), &status_fail());
    } else {
        print_step(&format!("Created directory {}", path), &status_ok());
    }
}

pub fn setup_hostname() {
    match fs::read_to_string("/proc/sys/kernel/hostname") {
        Ok(hostname) => {
            let hostname = hostname.trim();
            print_step(&format!("Hostname set to: {}", hostname), &status_ok());
        }
        Err(e) => {
            print_step(&format!("Failed to read hostname: {}", e), &status_fail());
        }
    }
}

pub fn get_os_name() -> String {
    let contents = fs::read_to_string("/etc/os-release").unwrap_or_default();

    for line in contents.lines() {
        if let Some(name) = line.strip_prefix("PRETTY_NAME=") {
            return name.trim_matches('"').to_string();
        }
    }

    "Unknown Linux".to_string()
}

pub fn setup_device_manager() {
    // Open /dev/null for output redirection
    let dev_null = || File::open("/dev/null").unwrap_or_else(|_| {
        print_step("Failed to open /dev/null", &status_fail());
        std::process::exit(1);
    });

    if Path::new("/lib/systemd/systemd-udevd").exists() {
        // Arch-like system with systemd-udevd
        if let Err(_) = Command::new("/lib/systemd/systemd-udevd")
            .arg("--daemon")
            .stdout(Stdio::from(dev_null()))
            .stderr(Stdio::from(dev_null()))
            .status()
        {
            print_step("Failed to start systemd-udevd", &status_fail());
        } else {
            print_step("Started udevd", &status_ok());
        }

    } else if Path::new("/sbin/udevd").exists() || Path::new("/usr/lib/udevd").exists() {
        // Generic udevd systems (e.g. Debian)
        let udev_path = if Path::new("/sbin/udevd").exists() {
            "/sbin/udevd"
        } else {
            "/usr/lib/udevd"
        };

        if let Err(_) = Command::new(udev_path)
            .arg("--daemon")
            .stdout(Stdio::from(dev_null()))
            .stderr(Stdio::from(dev_null()))
            .status()
        {
            print_step("Failed to start udevd", &status_fail());
        } else {
            print_step("Started udevd", &status_ok());
        }

    } else if Path::new("/sbin/mdev").exists() {
        // BusyBox mdev setup
        let _ = fs::write("/proc/sys/kernel/hotplug", "/sbin/mdev");

        if let Err(_) = Command::new("/sbin/mdev")
            .arg("-s")
            .stdout(Stdio::from(dev_null()))
            .stderr(Stdio::from(dev_null()))
            .status()
        {
            print_step("Failed to run mdev -s", &status_fail());
        } else {
            print_step("Initialized /dev with mdev -s", &status_ok());
        }

    } else {
        print_step("No supported device manager (udevd or mdev) found", &status_fail());
    }
}

