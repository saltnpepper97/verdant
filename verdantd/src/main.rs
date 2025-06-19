use std::process;
use common::{print_step, status_ok};

mod service;
mod loader;
mod runtime;

use crate::loader::load_enabled_services;
use crate::runtime::ServiceManager;

fn main() {
    print_step("verdantd started successfully", &status_ok());

    // Load and start services
    let configs = load_enabled_services();
    let mut manager = ServiceManager::new(configs);

    if let Err(e) = manager.run() {
        eprintln!("Error running service manager: {}", e);
        process::exit(1);
    }
}

