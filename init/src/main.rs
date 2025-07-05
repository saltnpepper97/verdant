mod actions;
mod device_manager;
mod env;
mod filesystem;
mod hardware_drivers;
mod ipc_server;
mod kernel;
mod mount;
mod network;
mod run;
mod seed;
mod service_manager;
mod signal;
mod tty;
mod unmount;
mod utils;

use std::{
    env::args,
    fs,
    path::Path,
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
use bloom::ipc::INIT_SOCKET_PATH;
use bloom::config::Config;
use tty::TtyManager;

use crate::service_manager::launch_verdant_service_manager;

fn main() {
    let is_test = args().any(|arg| arg == "test");

    if !is_test && std::process::id() != 1 {
        eprintln!("Verdant: Must be run as PID 1 (init).");
        std::process::exit(1);
    }

    let config = Config::from_file("/etc/verdant/config.toml")
        .expect("Failed to load config file");
    let tty_list = Arc::new(config.init.tty_sessions.clone());

    let result = std::panic::catch_unwind(|| inner_main(tty_list.clone()));

    if result.is_err() {
        eprintln!("Fatal error in init process. Dropping to emergency shell.");
        spawn_recovery_shell();
    }

    loop {
        thread::sleep(Duration::from_secs(60));
    }
}

fn inner_main(tty_list: Arc<Vec<String>>) {
    let (console_logger_impl, file_logger, start_time) = run::boot();

    let console_logger: Arc<Mutex<dyn ConsoleLogger + Send + Sync>> = console_logger_impl;
    let file_logger: Arc<Mutex<dyn FileLogger + Send + Sync>> = file_logger;

    let shutdown_flag = Arc::new(AtomicBool::new(false));
    let reboot_flag = Arc::new(AtomicBool::new(false));
    let boot_complete_flag = Arc::new(AtomicBool::new(false));
    let tty_launched_flag = Arc::new(AtomicBool::new(false));

    let tty_manager = Arc::new(Mutex::new(
        TtyManager::new().expect("Failed to create TTY manager"),
    ));

    wait_for_init_ipc_socket(&console_logger);

    {
        use bloom::colour::color::{RESET, YELLOW};
        use bloom::time::format_duration;
        let elapsed = start_time.elapsed();
        println!("\nTook: {} {} {}", YELLOW, format_duration(elapsed), RESET);
    }

    {
        let ipc_shutdown_flag = Arc::clone(&shutdown_flag);
        let ipc_reboot_flag = Arc::clone(&reboot_flag);
        let ipc_boot_complete_flag = Arc::clone(&boot_complete_flag);
        let ipc_console_logger = Arc::clone(&console_logger);
        let ipc_file_logger = Arc::clone(&file_logger);
        let ipc_main_thread = thread::current();

        thread::spawn(move || {
            if let Err(e) = ipc_server::run_ipc_server(
                ipc_shutdown_flag,
                ipc_reboot_flag,
                ipc_boot_complete_flag,
                ipc_console_logger,
                ipc_file_logger,
                ipc_main_thread,
            ) {
                eprintln!("Init IPC server failed: {e}");
            }
        });
    }

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

    signal::install_signal_handlers(
        Arc::clone(&shutdown_flag),
        Arc::clone(&reboot_flag),
        Arc::clone(&file_logger),
        Arc::clone(&console_logger),
        thread::current(),
    )
    .expect("Failed to install signal handlers");

    loop {
        if reboot_flag.load(Ordering::SeqCst) {
            log_shutdown(&console_logger, &file_logger, "Reboot");
            if let (Ok(mut con), Ok(mut file)) = (console_logger.lock(), file_logger.lock()) {
                let _ = unmount::unmount_fstab_filesystems(&mut *con, &mut *file);
            }
            remove_init_socket(&file_logger);
            let _ = actions::reboot();
            loop {
                thread::park();
            }
        }

        if shutdown_flag.load(Ordering::SeqCst) {
            log_shutdown(&console_logger, &file_logger, "Shutdown");
            if let (Ok(mut con), Ok(mut file)) = (console_logger.lock(), file_logger.lock()) {
                let _ = unmount::unmount_fstab_filesystems(&mut *con, &mut *file);
            }
            remove_init_socket(&file_logger);
            let _ = actions::shutdown();
            loop {
                thread::park();
            }
        }

        if boot_complete_flag.load(Ordering::SeqCst)
            && !tty_launched_flag.swap(true, Ordering::SeqCst)
        {
            let mut tty_mgr = tty_manager.lock().unwrap();
            if let Err(e) = tty_mgr.launch_tty_sessions(&tty_list) {
                if let Ok(mut con) = console_logger.lock() {
                    con.message(
                        LogLevel::Fail,
                        &format!("Failed to launch getty sessions: {}", e),
                        Duration::ZERO,
                    );
                }
            } else {
                let count = tty_list.len();
                let summary = format!("{} TTY sessions launched successfully.", count);

                if let Ok(mut con) = console_logger.lock() {
                    con.message(LogLevel::Info, &summary, Duration::ZERO);
                }
                if let Ok(mut file) = file_logger.lock() {
                    file.log(LogLevel::Info, &summary);
                }

                let tty_mgr_clone = Arc::clone(&tty_manager);
                thread::spawn(move || {
                    let mut mgr = tty_mgr_clone.lock().unwrap();
                    mgr.supervise();
                });
            }
        }

        thread::park_timeout(Duration::from_millis(500));
    }
}

fn wait_for_init_ipc_socket(console_logger: &Arc<Mutex<dyn ConsoleLogger + Send + Sync>>) {
    const WAIT_TIMEOUT_MS: u64 = 5000;
    const WAIT_INTERVAL_MS: u64 = 50;
    let mut waited_ms = 0;

    while waited_ms < WAIT_TIMEOUT_MS {
        if Path::new(INIT_SOCKET_PATH).exists() {
            if let Ok(mut con) = console_logger.lock() {
                con.message(
                    LogLevel::Info,
                    &format!("Init IPC socket ready at {}", INIT_SOCKET_PATH),
                    Duration::ZERO,
                );
            }
            return;
        }
        thread::sleep(Duration::from_millis(WAIT_INTERVAL_MS));
        waited_ms += WAIT_INTERVAL_MS;
    }

    if let Ok(mut con) = console_logger.lock() {
        con.message(
            LogLevel::Fail,
            &format!("Timeout waiting for init IPC socket: {}", INIT_SOCKET_PATH),
            Duration::ZERO,
        );
    }
    panic!("Init IPC socket did not appear in time");
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

fn remove_init_socket(file_logger: &Arc<Mutex<dyn FileLogger + Send + Sync>>) {
    let path = Path::new(INIT_SOCKET_PATH);
    if path.exists() {
        if let Err(e) = fs::remove_file(path) {
            if let Ok(mut file) = file_logger.lock() {
                file.log(LogLevel::Warn, &format!("Failed to remove init socket: {}", e));
            }
        }
    }
}

