use std::fs::{self, Permissions, File};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use nix::unistd::sethostname;

use common::{print_step, status_fail, status_ok, status_warn};

/// Setup fonts before anything else in the terminal
pub fn setup_console_font() {
    let candidates = [
        ("/usr/share/consolefonts/Lat2-Terminus16.psfu.gz", "/usr/sbin/setfont"),
        ("/usr/share/kbd/consolefonts/Lat2-Terminus16.psfu.gz", "/usr/bin/setfont"),
    ];

    for (font, setfont_bin) in candidates {
        if fs::metadata(font).is_ok() && fs::metadata(setfont_bin).is_ok() {
            if let Err(e) = Command::new(setfont_bin)
                .arg(font)
                .status()
            {
                print_step(&format!("Failed to set font using {}: {}", font, e), &status_fail());
            }
            return;
        }
    }

    print_step("No valid console font found — skipping setfont", &status_warn());
}

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
    match fs::read_to_string("/etc/hostname") {
        Ok(contents) => {
            let hostname = contents.trim(); // strip newline if present
            match sethostname(hostname) {
                Ok(_) => {
                    print_step(&format!("Hostname set to: {}", hostname), &status_ok());
                }
                Err(e) => {
                    print_step(&format!("Failed to set hostname: {}", e), &status_fail());
                }
            }
        }
        Err(e) => {
            print_step(&format!("Failed to read /etc/hostname: {}", e), &status_fail());
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
    let dev_null = || File::open("/dev/null").unwrap_or_else(|_| {
        print_step("Failed to open /dev/null", &status_fail());
        std::process::exit(1);
    });

    let started = if Path::new("/lib/systemd/systemd-udevd").exists() {
        if Command::new("/lib/systemd/systemd-udevd")
            .arg("--daemon")
            .stdout(Stdio::from(dev_null()))
            .stderr(Stdio::from(dev_null()))
            .status()
            .is_ok()
        {
            print_step("Started udevd", &status_ok());
            true
        } else {
            print_step("Failed to start systemd-udevd", &status_fail());
            false
        }
    } else if Path::new("/sbin/udevd").exists() || Path::new("/usr/lib/udevd").exists() {
        let udev_path = if Path::new("/sbin/udevd").exists() {
            "/sbin/udevd"
        } else {
            "/usr/lib/udevd"
        };
        if Command::new(udev_path)
            .arg("--daemon")
            .stdout(Stdio::from(dev_null()))
            .stderr(Stdio::from(dev_null()))
            .status()
            .is_ok()
        {
            print_step("Started udevd", &status_ok());
            true
        } else {
            print_step("Failed to start udevd", &status_fail());
            false
        }
    } else if Path::new("/sbin/mdev").exists() {
        let _ = std::fs::write("/proc/sys/kernel/hotplug", "/sbin/mdev");
        if Command::new("/sbin/mdev")
            .arg("-s")
            .stdout(Stdio::from(dev_null()))
            .stderr(Stdio::from(dev_null()))
            .status()
            .is_ok()
        {
            print_step("Started busybox mdev", &status_ok());
            true
        } else {
            print_step("Failed to run mdev -s", &status_fail());
            false
        }
    } else {
        print_step("No supported device manager (udevd or mdev) found", &status_fail());
        false
    };

    // If udevd started, try to run udevadm trigger to process devices
    if started && Path::new("/usr/bin/udevadm").exists() {
        if Command::new("/usr/bin/udevadm")
            .arg("trigger")
            .stdout(Stdio::from(dev_null()))
            .stderr(Stdio::from(dev_null()))
            .status()
            .is_ok()
        {
            print_step("Triggered udev events", &status_ok());
        } else {
            print_step("Failed to trigger udev events with udevadm", &status_fail());
        }
    }
}

/// Run fsck on the root filesystem if supported
pub fn check_root_filesystem() {
    let status = Command::new("fsck")
        .args(&["-a", "-C", "/"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    match status {
        Ok(s) if s.success() => {
            print_step("Filesystem check on '/' completed with no errors", &status_ok());
        }
        Ok(s) if s.code() == Some(1) => {
            // 1 = filesystem errors corrected
            print_step("Filesystem check on '/' found and corrected errors", &status_ok());
        }
        Ok(s) => {
            print_step(
                &format!("Filesystem check on / returned error code {}", s),
                &status_fail(),
            );
        }
        Err(e) => {
            print_step(&format!("Failed to run fsck on /: {}", e), &status_fail());
        }
    }
}


