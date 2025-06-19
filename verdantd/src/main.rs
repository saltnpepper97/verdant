mod runtime;
mod ipc;
mod service;
mod loader;

use std::sync::mpsc::channel;
use crate::runtime::{ServiceManager, SystemAction};
use crate::ipc::start_ipc_listener;
use crate::loader::load_enabled_services;
use common::{print_step, status_ok};
use ipc_protocol;

fn main() {
    print_step("verdantd started successfully", &status_ok());


    // Start IPC listener thread and get the receiver channel
    let (tx, rx) = channel::<SystemAction>();
    if let Err(e) = start_ipc_listener(tx) {
        eprintln!("Failed to start IPC listener: {}", e);
        std::process::exit(1);
    }

    let configs = load_enabled_services();
    let mut manager = ServiceManager::new(configs);
    
    // Run the manager supervising services and listening for IPC commands
    if let Err(e) = manager.run_with_ipc(rx) {
        eprintln!("Service manager error: {}", e);
        std::process::exit(1);
    }
}

