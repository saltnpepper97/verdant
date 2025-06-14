use std::process::{Command, exit};
use std::fs;
use std::ffi::CString;
use libc;
use nix::mount::{mount, MsFlags};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::{fork, ForkResult};
use nix::Error;
use common::colour::*;

fn mount_proc() -> Result<(), Error> {
    mount(
        Some("proc"),
        "/proc",
        Some("proc"),
        MsFlags::empty(),
        None::<&str>,
    )
}

fn mount_sys() -> Result<(), Error> {
    mount(
        Some("sysfs"),
        "/sys",
        Some("sysfs"),
        MsFlags::empty(),
        None::<&str>,
    )
}

fn mount_dev() -> Result<(), Error> {
    mount(
        Some("devtmpfs"),
        "/dev",
        Some("devtmpfs"),
        MsFlags::empty(),
        None::<&str>,
    )
}

fn mount_run() -> Result<(), Error> {
    mount(
        Some("tmpfs"),
        "/run",
        Some("tmpfs"),
        MsFlags::empty(),
        None::<&str>,
    )
}

fn start_drives() {
    println!("{} Mounting /proc...", tag_boot());

    if let Err(e) = mount_proc() {
        eprintln!("{} Failed to mount /proc: {}", status_fail(), e);
        exit(1);
    }
    println!("{}", status_ok());

    println!("{} Mounting /sys...", tag_boot());

    if let Err(e) = mount_sys() {
        eprintln!("{} Failed to mount /sys: {}", status_fail(), e);
        exit(1);
    }
    println!("    {}", status_ok());
    
    println!("{} Mounting /dev...", tag_boot());

    if let Err(e) = mount_dev() {
        eprintln!("{} Failed to mount /dev: {}", status_fail(), e);
        exit(1);
    }
    println!("    {}", status_ok());

    println!("{} Mounting /run...", tag_boot());

    if let Err(e) = mount_run() {
        eprintln!("{} Failed to mount /run: {}", status_fail(), e);
        exit(1);
    }
    println!("    {}", status_ok());
}

pub fn init_hostname() -> Result<(), Box<dyn std::error::Error>> {
    let hostname = fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| {
            eprintln!("{} /etc/hostname not found, defaulting to 'verdant'", status_fail());
            "verdant".to_string()
        });

    let c_hostname = CString::new(hostname.clone())?;
    let result = unsafe { libc::sethostname(c_hostname.as_ptr(), hostname.len()) };

    if result == 0 {
        println!("{} Hostname set to: {}", status_ok(), hostname);
        Ok(())
    } else {
        Err(std::io::Error::last_os_error().into())
    }
}

fn main() {

    // Check we are running as PID 1
    //if std::process::id() != 1 {
    //    eprintln!("This program must be run as PID 1.");
    //    std::process::exit(1);
    //}

    // Startup banner
    verdant_banner();

    // Start the necessary drives
    start_drives();

    // Set hostname
    let _ = init_hostname();

    println!("{} Starting verdantd service supervisor...", tag_boot());

    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            // exec verdantd
            let _err = Command::new("/usr/bin/verdantd")
                .spawn()
                .expect("failed to exec verdantd");
            exit(0);
        }
        Ok(ForkResult::Parent { child }) => {
            println!("    {} started with PID {}", status_ok(), child);
            // Wait for children and reap zombies
            loop {
                match waitpid(None, Some(WaitPidFlag::WNOHANG)) {
                    Ok(WaitStatus::StillAlive) => {
                        // Sleep or do minimal work
                        std::thread::sleep(std::time::Duration::from_secs(1));
                    }
                    Ok(status) => {
                        println!("Child exited: {:?}", status);
                    }
                    Err(e) => {
                        eprintln!("waitpid error: {:?}", e);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("fork failed: {:?}", e);
            exit(1);
        }
    }
}
