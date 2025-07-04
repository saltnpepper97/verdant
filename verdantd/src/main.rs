use std::sync::mpsc::channel;
use std::thread;
use std::time::Duration;

mod control;
mod ipc_server;
mod loader;
mod manager;
mod parser;
mod service;
mod shutdown;
mod supervisor;

use crate::manager::Manager;
use crate::loader::load_services;
use crate::ipc_server::run_ipc_server;

use bloom::ipc::{IpcCommand, IpcRequest, IpcTarget, send_ipc_request, INIT_SOCKET_PATH};
use bloom::log::{ConsoleLogger, ConsoleLoggerImpl, FileLogger, FileLoggerImpl};
use bloom::status::LogLevel;

fn main() {
    let mut console_logger = ConsoleLoggerImpl::new(LogLevel::Info);
    let mut file_logger = FileLoggerImpl::new(LogLevel::Info, "/var/log/verdant/verdantd.log");
    file_logger.initialize(&mut console_logger).expect("Failed to init file logger");
    console_logger.banner("Starting Verdant Service Manager");

    // Create the manager
    let (services, loaded_count, failed_count) = load_services(&mut file_logger);
    
    // Log to console
    console_logger.message(
        LogLevel::Info,
        &format!("Service loading complete: {} loaded, {} failed.", loaded_count, failed_count),
        std::time::Duration::ZERO,
    );
   
    let manager = Manager::new(&mut file_logger);
    // Start only the "base" and "network" startup package services
    manager.start_startup_services(&["base", "network"], &mut file_logger, &mut console_logger);

    // Set up channel to receive shutdown/reboot commands from IPC server
    let (shutdown_tx, shutdown_rx) = channel::<IpcCommand>();

    // Spawn IPC server in a background thread, passing sender side of channel
    let ipc_shutdown_tx = shutdown_tx.clone();
    thread::spawn(move || {
        if let Err(e) = run_ipc_server(ipc_shutdown_tx) {
            eprintln!("IPC server failed: {}", e);
        }
    });

    // Main event loop, waits for shutdown or reboot commands
    loop {
        if let Ok(command) = shutdown_rx.recv() {
            match command {
                IpcCommand::Shutdown | IpcCommand::Reboot => {
                    console_logger.message(LogLevel::Info, "Shutting down all services...", Duration::ZERO);

                    // Use references directly instead of trying to extract owned supervisors
                    match manager.shutdown_all_services() {
                        Ok(_) => {
                            console_logger.message(LogLevel::Ok, "All services stopped cleanly.", Duration::ZERO);
                        }
                        Err(e) => {
                            console_logger.message(LogLevel::Fail, &format!("Shutdown error: {e}"), Duration::ZERO);
                        }
                    }

                    // Notify init process that shutdown or reboot was requested
                    let notify = IpcRequest {
                        target: IpcTarget::Init,
                        command,
                    };

                    if let Err(e) = send_ipc_request(INIT_SOCKET_PATH, &notify) {
                        console_logger.message(LogLevel::Fail, &format!("Failed to notify init: {e}"), Duration::ZERO);
                    }

                    std::process::exit(0);
                }
                _ => {
                    // Ignore other commands
                }
            }
        }
        // Sleep briefly to avoid busy loop if channel is empty
        thread::sleep(Duration::from_millis(100));
    }
}

