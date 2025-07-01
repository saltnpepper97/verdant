mod actions;
mod device_manager;
mod env;
mod fs;
mod hardware_drivers;
mod ipc_server;
mod kernel;
mod mount;
mod network;
mod run;
mod seed;
mod service_manager;
mod signal;
mod utils;

use std::time::Duration;
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;

use nix::sys::signal::{SigSet, Signal};

use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;

use crate::service_manager::launch_verdant_service_manager;

fn main() {
    let (raw_console_logger, file_logger, start_time) = run::boot();

    let console_logger: Arc<Mutex<dyn ConsoleLogger + Send + Sync>> =
        Arc::new(Mutex::new(raw_console_logger));
    let file_logger: Arc<Mutex<dyn FileLogger + Send + Sync>> = file_logger;


    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let reboot_flag = Arc::new(AtomicBool::new(false));

    // Block signals globally
    let mut sigset = SigSet::empty();
    sigset.add(Signal::SIGCHLD);
    sigset.add(Signal::SIGTERM);
    sigset.thread_block().expect("Failed to block signals");

    // Start IPC server
    {
        let ipc_shutdown_flag = Arc::clone(&shutdown_flag);
        let ipc_reboot_flag = Arc::clone(&reboot_flag);
        let ipc_console_logger = Arc::clone(&console_logger);
        let ipc_file_logger = Arc::clone(&file_logger);
        let ipc_main_thread = thread::current();

        thread::spawn(move || {
            if let Err(e) = ipc_server::run_ipc_server(
                ipc_shutdown_flag,
                ipc_reboot_flag,
                ipc_console_logger,
                ipc_file_logger,
                ipc_main_thread,
            ) {
                eprintln!("Init IPC server failed: {e}");
            }
        });
    }

    std::thread::sleep(std::time::Duration::from_millis(500));

    {
        use bloom::colour::color::{YELLOW, RESET};
        use bloom::time::format_duration;
        let elapsed = start_time.elapsed();
        println!();
        println!("Took: {} {} {}", YELLOW, format_duration(elapsed), RESET);
    }

    // Launch verdantd without waiting for it to exit
    let mut con = console_logger.lock().unwrap();
    if let Some(_child) = launch_verdant_service_manager(&mut *con) {
        // success
    } else {
        con.message(
            LogLevel::Fail,
            "Critical: Could not launch Verdant Service Manager. Dropping to recovery shell.",
            Duration::ZERO,
        );
        drop(con); // release lock before shell

        match Command::new("/bin/sh")
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .and_then(|mut child| child.wait())
        {
            Ok(status) => {
                println!("Recovery shell exited with status: {status}");
            }
            Err(e) => {
                eprintln!("Failed to launch recovery shell: {e}");
            }
        }

        // Optionally reboot or halt afterwards
        shutdown_flag.store(true, Ordering::SeqCst);
    }

    // Install signal handlers
    signal::install_signal_handlers(
        Arc::clone(&shutdown_flag),
        Arc::clone(&file_logger),
        Arc::clone(&console_logger),
        thread::current(),
    )
    .expect("Failed to install signal handlers");

    loop {
        if reboot_flag.load(Ordering::SeqCst) {
            log_shutdown(&console_logger, &file_logger, "Reboot");
            let _ = actions::reboot();
            break;
        }
        if shutdown_flag.load(Ordering::SeqCst) {
            log_shutdown(&console_logger, &file_logger, "Shutdown");
            let _ = actions::shutdown();
            break;
        }
        thread::park_timeout(std::time::Duration::from_secs(1));
    }
}

fn log_shutdown(
    console_logger: &Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
    file_logger: &Arc<Mutex<dyn FileLogger + Send + Sync>>,
    action: &str,
) {
    let msg = format!("Init {} requested, shutting down cleanly.", action);
    if let Ok(mut con) = console_logger.lock() {
        con.message(bloom::status::LogLevel::Info, &msg, std::time::Duration::ZERO);
    }
    if let Ok(mut file) = file_logger.lock() {
        file.log(bloom::status::LogLevel::Info, &msg);
    }
}

