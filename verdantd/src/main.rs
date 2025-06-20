mod ipc;
mod managed_service;
mod service;
mod loader;
mod sort;
mod runtime;

use single_instance::SingleInstance;
use std::env;
use std::fs;
use std::process;
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use signal_hook::consts::signal::*;
use signal_hook::flag;

use crate::runtime::{ServiceManager, SystemAction};
use crate::ipc::{start_ipc_listener};
use ipc_protocol::SOCKET_PATH;
use crate::loader::load_enabled_services;
use common::{print_step, status_ok};

fn main() {
    let is_test_mode = env::args().any(|arg| arg == "--test");

    // Set up signal handling only in test mode
    let term_now = Arc::new(AtomicBool::new(false));
    if is_test_mode {
        flag::register(SIGINT, Arc::clone(&term_now)).expect("Failed to register SIGINT handler");
    }

    let instance = SingleInstance::new("verdantd_single_instance_lock").unwrap_or_else(|e| {
        eprintln!("Failed to create single instance lock: {}", e);
        process::exit(1);
    });

    if !instance.is_single() {
        eprintln!("Another instance of verdantd is already running.");
        process::exit(1);
    }

    print_step("verdantd started successfully", &status_ok());

    // Start IPC listener
    let (tx, rx) = channel::<SystemAction>();
    if let Err(e) = start_ipc_listener(tx) {
        eprintln!("Failed to start IPC listener: {}", e);
        process::exit(1);
    }

    let configs = load_enabled_services();
    let mut manager = ServiceManager::new(configs);

    // If in test mode, spawn a thread to watch for SIGINT and trigger graceful exit
    if is_test_mode {
        let term_now = Arc::clone(&term_now);
        std::thread::spawn(move || {
            while !term_now.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            println!("\n[TEST MODE] Received SIGINT, cleaning up verdantd.sock and exiting...");
            let _ = fs::remove_file(SOCKET_PATH);
            process::exit(0);
        });
    }

    if let Err(e) = manager.run_with_ipc(rx) {
        eprintln!("Service manager error: {}", e);
        let _ = fs::remove_file(SOCKET_PATH); // Also clean up on normal failure
        process::exit(1);
    }

    let _ = fs::remove_file(SOCKET_PATH); // Clean up on graceful exit
}

