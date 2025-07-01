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

use std::{
    env::args,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;

use crate::service_manager::launch_verdant_service_manager;

fn main() {
    // Check for "test" argument to skip full init (useful for running under debugger/test harness)
    if args().any(|arg| arg == "test") {
        eprintln!("Test mode detected, skipping full init.");
        return;
    }

    let result = std::panic::catch_unwind(inner_main);

    if result.is_err() {
        eprintln!("Fatal error in init process. Dropping to emergency shell.");
        spawn_recovery_shell();
    }

    // Minimal fallback loop to keep PID 1 alive without spamming output
    loop {
        thread::sleep(Duration::from_secs(60));
    }
}

fn inner_main() {
    let (raw_console_logger, file_logger, start_time) = run::boot();

    let console_logger: Arc<Mutex<dyn ConsoleLogger + Send + Sync>> =
        Arc::new(Mutex::new(raw_console_logger));
    let file_logger: Arc<Mutex<dyn FileLogger + Send + Sync>> = file_logger;

    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let reboot_flag = Arc::new(AtomicBool::new(false));

    // Start IPC server thread (comment out if suspected to cause issues)
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

    thread::sleep(Duration::from_millis(500));

    // Show boot timing
    {
        use bloom::colour::color::{RESET, YELLOW};
        use bloom::time::format_duration;

        let elapsed = start_time.elapsed();
        println!("\nTook: {} {} {}", YELLOW, format_duration(elapsed), RESET);
    }

    // Launch VerdantD service manager
    if let Ok(mut guard) = console_logger.lock() {
        let logger: &mut dyn ConsoleLogger = &mut *guard;
        if launch_verdant_service_manager(logger).is_none() {
            logger.message(
                LogLevel::Fail,
                "Critical: Could not launch Verdant Service Manager. Dropping to recovery shell.",
                Duration::ZERO,
            );
            drop(guard);
            spawn_recovery_shell();
        }
    }

    // Install signal handlers (simplified, no global blocking)
    signal::install_signal_handlers(
        Arc::clone(&shutdown_flag),
        Arc::clone(&file_logger),
        Arc::clone(&console_logger),
        thread::current(),
    )
    .expect("Failed to install signal handlers");

    // Main control loop
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

        thread::sleep(Duration::from_millis(500));
    }
}

fn spawn_recovery_shell() {
    match Command::new("/bin/sh")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .and_then(|mut child| child.wait())
    {
        Ok(status) => {
            eprintln!("Recovery shell exited with status: {status}");
        }
        Err(e) => {
            eprintln!("Failed to launch recovery shell: {e}");
        }
    }
}

fn log_shutdown(
    console_logger: &Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
    file_logger: &Arc<Mutex<dyn FileLogger + Send + Sync>>,
    action: &str,
) {
    let msg = format!("Init {} requested, shutting down cleanly.", action);

    if let Ok(mut con) = console_logger.lock() {
        con.message(LogLevel::Info, &msg, Duration::ZERO);
    }

    if let Ok(mut file) = file_logger.lock() {
        file.log(LogLevel::Info, &msg);
    }
}

