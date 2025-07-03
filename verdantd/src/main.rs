mod ipc_server;
mod loader;
mod manager;
mod ordering;
mod parser;
mod process;
mod service_file;
mod supervisor;

use std::sync::{Arc, Mutex, mpsc};
use std::sync::atomic::AtomicBool;
use std::thread;

use bloom::log::{ConsoleLogger, ConsoleLoggerImpl, FileLogger, FileLoggerImpl};
use bloom::status::LogLevel;

use crate::manager::ServiceManager;
use crate::loader::load_services;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut console_logger_impl = ConsoleLoggerImpl::new(LogLevel::Info);
    let mut file_logger_impl = FileLoggerImpl::new(LogLevel::Info, "/var/log/verdant/verdantd.log");

    let version = env!("CARGO_PKG_VERSION");
    console_logger_impl.banner(&format!(
        "Verdant Service Manager v{} - Cultivating System Harmony",
        version
    ));
    let _ = file_logger_impl.initialize(&mut console_logger_impl);

    let console_logger: Arc<Mutex<dyn ConsoleLogger + Send + Sync>> =
        Arc::new(Mutex::new(console_logger_impl));
    let file_logger: Arc<Mutex<dyn FileLogger + Send + Sync>> =
        Arc::new(Mutex::new(file_logger_impl));

    let manager = Arc::new(Mutex::new(ServiceManager::new(
        console_logger.clone(),
        file_logger.clone(),
    )));

    let shutdown_flag = Arc::new(AtomicBool::new(false));

    {
        let mut con = console_logger.lock().unwrap();
        let mut file = file_logger.lock().unwrap();

        let services = load_services(&mut *con, &mut *file)?;

        let mut mgr = manager.lock().unwrap();
        for service in services {
            mgr.add_service(service)?;
        }
    }

    {
        let mut mgr = manager.lock().unwrap();
        mgr.start_startup_services()?;
        mgr.supervise_all(Arc::clone(&shutdown_flag))?;
    }


    // Clone for IPC server
    let ipc_manager = Arc::clone(&manager);
    let ipc_shutdown_flag = Arc::clone(&shutdown_flag);


    let (ipc_ready_tx, ipc_ready_rx) = mpsc::channel();
    let (shutdown_done_tx, shutdown_done_rx) = mpsc::channel();
    let shutdown_done_tx = Arc::new(Mutex::new(Some(shutdown_done_tx)));

    thread::spawn(move || {
        if let Err(e) = ipc_server::run_ipc_server(ipc_manager, ipc_shutdown_flag, Some(ipc_ready_tx), Some(shutdown_done_tx)) {
            eprintln!("IPC server error: {}", e);
        }
    });

    // wait for ipc server to signal readiness before printing banner
    ipc_ready_rx.recv().expect("Failed to receive IPC ready signal");

    {
        let banner = "\nBoot process complete. Breathe in. Log in.";
        let mut con = console_logger.lock().unwrap();
        con.banner(banner);
    }


    // Wait until shutdown actually completes
    shutdown_done_rx.recv().expect("Failed to receive shutdown completion signal");
    
    // Use console logger to emit clean shutdown log
    {
        let mut file = file_logger.lock().unwrap();
        file.log(LogLevel::Info, "verdantd exiting cleanly.");
    }

    let socket_path = bloom::ipc::VERDANTD_SOCKET_PATH;
    if std::path::Path::new(socket_path).exists() {
        if let Err(e) = std::fs::remove_file(socket_path) {
            let mut file = file_logger.lock().unwrap();
            let msg = format!("Failed to remove verdantd socket: {}", e);
            file.log(LogLevel::Warn, &msg);
        }
    }

    Ok(())
}

