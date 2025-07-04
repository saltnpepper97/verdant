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

// Get the Cargo package version set at compile time
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let mut console_logger = ConsoleLoggerImpl::new(LogLevel::Info);
    let mut file_logger = FileLoggerImpl::new(LogLevel::Info, "/var/log/verdant/verdantd.log");

    console_logger.banner(&format!(
        "Verdantd Service Manager v{} - Cultivating System Harmony",
        VERSION
    ));

    file_logger
        .initialize(&mut console_logger)
        .expect("Failed to init file logger");

    let (_services, loaded_count, failed_count) = load_services(&mut file_logger);

    console_logger.message(
        LogLevel::Info,
        &format!("Service loading complete: {} loaded, {} failed.", loaded_count, failed_count),
        Duration::ZERO,
    );

    let manager = Manager::new(&mut file_logger);
    manager.start_startup_services(&["base", "network"], &mut file_logger, &mut console_logger);

    let (shutdown_tx, shutdown_rx) = channel::<IpcCommand>();

    let ipc_shutdown_tx = shutdown_tx.clone();
    thread::spawn(move || {
        if let Err(e) = run_ipc_server(ipc_shutdown_tx) {
            eprintln!("IPC server failed: {}", e);
        }
    });

    loop {
        if let Ok(command) = shutdown_rx.recv() {
            match command {
                IpcCommand::Shutdown | IpcCommand::Reboot => {
                    let msg = "Shutting down all services...";
                    console_logger.message(LogLevel::Info, msg, Duration::ZERO);
                    file_logger.log(LogLevel::Info, msg);

                    match manager.shutdown_all_services() {
                        Ok(_) => {
                            let msg = "All services stopped cleanly.";
                            console_logger.message(LogLevel::Ok, msg, Duration::ZERO);
                            file_logger.log(LogLevel::Ok, msg);
                        }
                        Err(e) => {
                            let msg = format!("Shutdown error: {e}");
                            console_logger.message(LogLevel::Fail, &msg, Duration::ZERO);
                            file_logger.log(LogLevel::Fail, &msg);
                        }
                    }

                    let notify = IpcRequest {
                        target: IpcTarget::Init,
                        command,
                    };

                    if let Err(e) = send_ipc_request(INIT_SOCKET_PATH, &notify) {
                        let msg = format!("Failed to notify init: {e}");
                        console_logger.message(LogLevel::Fail, &msg, Duration::ZERO);
                        file_logger.log(LogLevel::Fail, &msg);
                    }

                    std::process::exit(0);
                }
                _ => {
                    // Ignore other commands
                }
            }
        }

        thread::sleep(Duration::from_millis(100));
    }
}

