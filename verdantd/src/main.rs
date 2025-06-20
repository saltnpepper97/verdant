mod ipc;
mod managed_service;
mod service;
mod loader;
mod sort;
mod runtime;

use std::fs::OpenOptions;
use std::path::Path;
use std::sync::Arc;
use file_guard::{lock, FileGuard, Lock};
use std::sync::mpsc::channel;
use crate::runtime::{ServiceManager, SystemAction};
use crate::ipc::start_ipc_listener;
use crate::loader::load_enabled_services;
use common::{print_step, status_ok};

const LOCKFILE_PATH: &str = "/run/verdantd.lock";

fn main() {
    let skip_lock = std::env::args().any(|a| a == "--no-lock");

    let _lock_guard = if skip_lock {
        None
    } else {
        Some(check_single_instance().unwrap_or_else(|e| {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }))
    };
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

pub fn check_single_instance() -> Result<FileGuard<Arc<std::fs::File>>, String> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(Path::new(LOCKFILE_PATH))
        .map_err(|e| format!("Failed to open lockfile: {}", e))?;

    let arc_file = Arc::new(file);

    let guard = lock(arc_file, Lock::Exclusive, 0, 1)
        .map_err(|e| format!("verdantd already running ({}): {}", LOCKFILE_PATH, e))?;

    Ok(guard)
}

