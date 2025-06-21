mod mount;
mod setup;
mod modules;
mod handoff;

use std::thread::sleep;
use std::time::Duration;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;

use common::{print_step, status_ok, verdant_banner};
use mount::{remount_root_rw, mount_essential};
use modules::{load_modules_from_map, merge_module_configs};
use handoff::handoff_to_verdantd;
use setup::*;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let skip_pid_check = args.iter().any(|arg| arg == "--test");

    if !skip_pid_check {
        if nix::unistd::getpid().as_raw() != 1 {
            eprintln!("Error: Verdant must be run as PID 1.");
            std::process::exit(1);
        }
    }

    wait_for_framebuffer(3);

    let os_name = get_os_name();
    verdant_banner(&os_name, VERSION);

    sleep(Duration::from_secs(1));

    println!();

    mount_essential();

    println!();

    setup_lock_dir();
    setup_runtime_dirs();
    setup_device_manager();

    if let Ok(modules) = merge_module_configs() {
        if let Err(_) = load_modules_from_map(&modules) {
            common::print_step("Warning: Failed to load some kernel modules, continuing boot", &common::status_fail());
        }
    } else {
        common::print_step("Warning: Failed to merge kernel module configs, continuing boot", &common::status_fail());
    }

    println!();

    if reap_zombies() {
        print_step("Reaped zombie processes", &status_ok());
    }

    check_root_filesystem();

    remount_root_rw();
    setup_hostname();
    
    handoff_to_verdantd() 
}

fn reap_zombies() -> bool {
    let mut reaped_any = false;

    loop {
        match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::StillAlive) | Err(_) => break,
            Ok(_) => reaped_any = true,
        }
    }
    reaped_any
}

