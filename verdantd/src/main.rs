mod ipc;
mod managed_service;
mod service;
mod loader;
mod sort;
mod runtime;

use std::sync::mpsc::channel;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use nix::fcntl::{Flock, FlockArg};
use crate::runtime::{ServiceManager, SystemAction};
use crate::ipc::start_ipc_listener;
use crate::loader::load_enabled_services;
use common::{print_step, status_ok};

fn main() {
    if let Err(e) = ensure_single_instance() {
        eprintln!("Failed to start verdantd: {}", e);
        std::process::exit(1);
    }
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

fn ensure_single_instance() -> io::Result<()> {
    let lockfile_path = "/run/verdantd.pid";

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .mode(0o644)
        .open(lockfile_path)?;

    // Correct: pass the File, not the raw fd
    let _lock = Flock::lock(file.try_clone()?, FlockArg::LockExclusiveNonblock)
        .map_err(|_| io::Error::new(io::ErrorKind::AlreadyExists, "verdantd is already running"))?;

    // Write PID to the lockfile (optional)
    writeln!(&file, "{}", std::process::id())?;

    Ok(())
}
