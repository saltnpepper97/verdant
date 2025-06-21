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
use std::sync::{Arc, Mutex, mpsc::channel};
use std::sync::atomic::{AtomicBool, Ordering};
use signal_hook::consts::signal::*;
use signal_hook::flag;

use crate::runtime::{ServiceManager, SystemAction};
use crate::ipc::start_ipc_listener;
use ipc_protocol::SOCKET_PATH;
use crate::loader::load_enabled_services;
use common::{print_step, status_ok};

fn main() {
    let is_test_mode = env::args().any(|arg| arg == "--test");
    let term_now = Arc::new(AtomicBool::new(false));

    if is_test_mode {
        flag::register(SIGINT, Arc::clone(&term_now)).expect("Failed to register SIGINT handler");
    }

    {
        // SCOPED INSTANCE
        let instance = SingleInstance::new("verdantd_single_instance_lock").unwrap_or_else(|e| {
            eprintln!("Failed to create single instance lock: {}", e);
            process::exit(1);
        });

        if !instance.is_single() {
            eprintln!("Another instance of verdantd is already running.");
            process::exit(1);
        }

        print_step("verdantd started successfully", &status_ok());

        let configs = load_enabled_services();
        let svc_manager = Arc::new(Mutex::new(ServiceManager::new(configs)));

        let (tx, rx) = channel::<SystemAction>();

        if let Err(e) = start_ipc_listener(tx.clone(), Arc::clone(&svc_manager)) {
            eprintln!("Failed to start IPC listener: {}", e);
            process::exit(1);
        }

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

        if let Err(e) = ServiceManager::run_with_ipc(Arc::clone(&svc_manager), rx) {
            eprintln!("Service manager error: {}", e);
            let _ = fs::remove_file(SOCKET_PATH);
            process::exit(1);
        }
    } // 👈 instance gets dropped here

    // Cleanup after manager exits
    let _ = fs::remove_file(SOCKET_PATH);
}

