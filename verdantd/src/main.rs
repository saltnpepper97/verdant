mod ipc;
mod managed_service;
mod service;
mod loader;
mod sort;
mod runtime;

use single_instance::SingleInstance;
use std::process;
use std::sync::mpsc::channel;
use crate::runtime::{ServiceManager, SystemAction};
use crate::ipc::start_ipc_listener;
use crate::loader::load_enabled_services;
use common::{print_step, status_ok};

fn main() {
    // Create a single instance lock with a unique name
    let instance = SingleInstance::new("verdantd_single_instance_lock").unwrap_or_else(|e| {
        eprintln!("Failed to create single instance lock: {}", e);
        process::exit(1);
    });

    if !instance.is_single() {
        eprintln!("Another instance of verdantd is already running.");
        process::exit(1);
    }

    print_step("verdantd started successfully", &status_ok());

    // Start IPC listener thread and get the receiver channel
    let (tx, rx) = channel::<SystemAction>();
    if let Err(e) = start_ipc_listener(tx) {
        eprintln!("Failed to start IPC listener: {}", e);
        process::exit(1);
    }

    let configs = load_enabled_services();
    let mut manager = ServiceManager::new(configs);

    if let Err(e) = manager.run_with_ipc(rx) {
        eprintln!("Service manager error: {}", e);
        process::exit(1);
    }
}

