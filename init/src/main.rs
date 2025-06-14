mod mount;
mod hostname;
mod process;
mod modules;
mod utils;
use crate::modules::load_all_modules;
use common::colour::*;
use std::process::exit;

fn main() {
    // Uncomment this to ensure the program is running as PID 1 (the init process)
    /*
    if std::process::id() != 1 {
        eprintln!("{} Verdant init system must be run as PID 1!", status_fail());
        exit(1);
    }
    */

    verdant_banner();

    mount::mount_drives();

    let _ = hostname::init_hostname();

    load_all_modules();

    process::start_verdantd();
}

