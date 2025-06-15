use nix::mount::{mount, MsFlags};
use nix::Error;
use std::io::{self, Write};
use std::process::exit;
use common::utils::*;

fn mount_proc() -> Result<(), Error> {
    mount(Some("proc"), "/proc", Some("proc"), MsFlags::empty(), None::<&str>)
}

fn mount_sys() -> Result<(), Error> {
    mount(Some("sysfs"), "/sys", Some("sysfs"), MsFlags::empty(), None::<&str>)
}

fn mount_dev() -> Result<(), Error> {
    mount(Some("devtmpfs"), "/dev", Some("devtmpfs"), MsFlags::empty(), None::<&str>)
}

fn mount_run() -> Result<(), Error> {
    mount(Some("tmpfs"), "/run", Some("tmpfs"), MsFlags::empty(), None::<&str>)
}

pub fn mount_drives() {
    println!("{} Mounting /proc...", tag_boot());
    io::stdout().flush().unwrap();
    if let Err(e) = mount_proc() {
        print_error(&format!("Failed to mount /proc: {}", e));
        exit(1);
    }
    println!("{} /proc mounted successfully", status_ok());

    println!("{} Mounting /sys...", tag_boot());
    io::stdout().flush().unwrap();
    if let Err(e) = mount_sys() {
        print_error(&format!("Failed to mount /sys: {}", e));
        exit(1);
    }
    println!("{} /sys mounted successfully", status_ok());

    println!("{} Mounting /dev...", tag_boot());
    io::stdout().flush().unwrap();
    if let Err(e) = mount_dev() {
        print_error(&format!("Failed to mount /dev: {}", e));
        exit(1);
    }
    println!("{} /dev mounted successfully", status_ok());

    println!("{} Mounting /run...", tag_boot());
    io::stdout().flush().unwrap();
    if let Err(e) = mount_run() {
        print_error(&format!("Failed to mount /run: {}", e));
        exit(1);
    }
    println!("{} /run mounted successfully", status_ok());
}
