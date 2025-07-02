mod ipc_server;
mod loader;
mod manager;
mod ordering;
mod parser;
mod process;
mod service_file;
mod supervisor;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use bloom::log::{ConsoleLogger, ConsoleLoggerImpl, FileLogger, FileLoggerImpl};
use bloom::status::LogLevel;

use crate::manager::ServiceManager;
use crate::loader::load_services;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut console_logger_impl = ConsoleLoggerImpl::new(LogLevel::Info);
    let mut file_logger_impl = FileLoggerImpl::new(LogLevel::Info, "/var/log/verdantd.log");

    let version = env!("CARGO_PKG_VERSION");
    console_logger_impl.banner(&format!(
        "Verdant Service Manager v{} - Cultivating System Harmony",
        version
    ));
    file_logger_impl.initialize(&mut console_logger_impl);

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

        let vars = HashMap::new();
        let services = load_services(&vars, &mut *con, &mut *file)?;

        let mut mgr = manager.lock().unwrap();
        for service in services {
            mgr.add_service(service)?;
        }
    }

    {
        let mut mgr = manager.lock().unwrap();
        mgr.start_startup_services()?;
        mgr.supervise_all(Arc::clone(&shutdown_flag))?; // <-- pass shutdown_flag here
    }

    // Clone for IPC server
    let ipc_manager = Arc::clone(&manager);
    let ipc_shutdown_flag = Arc::clone(&shutdown_flag);

    thread::spawn(move || {
        if let Err(e) = ipc_server::run_ipc_server(ipc_manager, ipc_shutdown_flag) {
            eprintln!("IPC server error: {}", e);
        }
    });

    // Main thread watches shutdown flag
    while !shutdown_flag.load(Ordering::SeqCst) {
        thread::sleep(std::time::Duration::from_secs(1));
    }
    
    // Use console logger to emit clean shutdown log
    {
        let mut file = file_logger.lock().unwrap();
        file.log(LogLevel::Info, "verdantd exiting cleanly.");
    }

    {
        let mut mgr = manager.lock().unwrap();
        mgr.shutdown(Arc::clone(&shutdown_flag))?; // <- graceful cleanup
    }

    Ok(())
}

