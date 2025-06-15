mod mount;
mod hostname;
mod process;

use common::utils::{print_info, verdant_banner};

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::exit;
use std::thread;
use std::time::Duration;
use nix::sys::reboot::{reboot, RebootMode};
use nix::sys::wait::{waitpid, WaitPidFlag};
use nix::unistd::Pid;

fn main() {
    // Uncomment to enforce PID 1
    /*
    if std::process::id() != 1 {
        eprintln!("{} Verdant init system must be run as PID 1!", status_fail());
        exit(1);
    }
    */

    verdant_banner();

    let _ = mount::mount_drives();    
    let _ = hostname::init_hostname();

    // Ensure /run/verdant exists
    if let Err(e) = fs::create_dir_all("/run/verdant") {
        eprintln!("Failed to create /run/verdant: {}", e);
        exit(1);
    }

    // Lock down permissions
    if let Err(e) = fs::set_permissions("/run/verdant", fs::Permissions::from_mode(0o700)) {
        eprintln!("Failed to set permissions on /run/verdant: {}", e);
        exit(1);
    }

    // Create allow_power_ops flag
    if let Err(e) = fs::write("/run/verdant/allow_power_ops", "1") {
        eprintln!("Failed to write allow_power_ops flag: {}", e);
        exit(1);
    }

    // Start verdantd
    process::start_verdantd();

    // Watch for shutdown or reboot
    watch_for_power_requests();

    // Manual cleanup after returning
    let _ = fs::remove_file("/run/verdant/allow_power_ops");
}

/// Watches /run/verdant for shutdown/reboot requests
fn watch_for_power_requests() {
    loop {
        // Reap any zombie processes
        loop {
            match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)) {
                Ok(nix::sys::wait::WaitStatus::StillAlive) => break,
                Ok(_status) => continue, // Reaped a zombie
                Err(_e) => break,        // Nothing to reap or error
            }
        }

        if Path::new("/run/verdant/request_shutdown").exists() {
            let _ = fs::remove_file("/run/verdant/request_shutdown");
            let _ = fs::remove_file("/run/verdant/allow_power_ops");
            print_info("Power request: shutting down...");
            let _ = reboot(RebootMode::RB_POWER_OFF);
        }

        if Path::new("/run/verdant/request_reboot").exists() {
            let _ = fs::remove_file("/run/verdant/request_reboot");
            let _ = fs::remove_file("/run/verdant/allow_power_ops");
            print_info("Power request: rebooting...");
            let _ = reboot(RebootMode::RB_AUTOBOOT);
        }

        thread::sleep(Duration::from_secs(1));
    }
}
