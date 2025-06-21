mod ipc;
mod managed_service;
mod service;
mod loader;
mod sort;
mod runtime;
mod pid_lock;

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
    if let Err(e) = pid_lock::acquire_pid_lock() {
        eprintln!("{}", e);
        std::process::exit(1);
    }

    let is_test_mode = env::args().any(|arg| arg == "--test");
    let term_now = Arc::new(AtomicBool::new(false));

    if is_test_mode {
        flag::register(SIGINT, Arc::clone(&term_now)).expect("Failed to register SIGINT handler");
    }

    print_step("verdantd started successfully", &status_ok());

    let configs = load_enabled_services();
    let svc_manager = Arc::new(Mutex::new(ServiceManager::new(configs)));

    let (tx, rx) = channel::<SystemAction>();

    if let Err(e) = start_ipc_listener(tx.clone(), Arc::clone(&svc_manager)) {
        eprintln!("Failed to start IPC listener: {}", e);
        let _ = fs::remove_file(SOCKET_PATH);
        process::exit(1);
    }

    if is_test_mode {
        let term_now = Arc::clone(&term_now);
        std::thread::spawn(move || {
            while !term_now.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            println!("\n[TEST MODE] Received SIGINT, cleaning up...");
            let _ = fs::remove_file(SOCKET_PATH);
            process::exit(0);
        });
    }

    let result = ServiceManager::run_with_ipc(Arc::clone(&svc_manager), rx);

    let _ = fs::remove_file(SOCKET_PATH);

    if let Err(e) = result {
        eprintln!("Service manager error: {}", e);
        process::exit(1);
    }
}

