use std::thread;
use std::time::Duration;

use common::*;

mod service;
mod loader;
mod runtime;

use crate::loader::load_enabled_services;
use crate::runtime::ServiceManager;

fn main() {
    print_info_step("verdantd is starting up...");

    // Initial service load
    let configs = load_enabled_services();
    let mut manager = ServiceManager::new(configs);

    if let Err(e) = manager.run() {
        eprintln!("Error running service manager: {}", e);
    }

    // Main loop
    loop {
        // Eventually, here you'll supervise services, restart if needed, etc.
        thread::sleep(Duration::from_secs(1));
    }
}

