mod mount;
mod setup;
mod modules;
mod handoff;

use common::verdant_banner;
use mount::mount_essential;
use modules::{load_modules_from_map, merge_module_configs};
use handoff::handoff_to_verdantd;
use setup::*;

fn main() {
    let os_name = get_os_name();
    verdant_banner(&os_name);
        
    println!();

    mount_essential();

    println!();

    setup_lock_dir();
    setup_runtime_dirs();

    setup_hostname();

    setup_device_manager();

    if let Ok(modules) = merge_module_configs() {
        if let Err(_) = load_modules_from_map(&modules) {
            std::process::exit(1);
        }
    } else {
        common::print_step("Failed to merge kernel module configs", &common::status_fail());
        std::process::exit(1);
    }

    handoff_to_verdantd() 
}


