use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;
use bloom::time::ProcessTimer;

use libc::SIGPWR;
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use signal_hook::{consts::signal::*, iterator::Signals};

pub fn install_signal_handlers(
    shutdown_flag: Arc<AtomicBool>,
    file_logger: Arc<Mutex<dyn FileLogger + Send + Sync>>,
    console_logger: Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
    main_thread: std::thread::Thread,
) -> Result<(), BloomError> {
    let handled_signals = &[
        SIGCHLD,
        SIGTERM,
        SIGINT,
        SIGPWR,
        SIGUSR1, // reboot
        SIGUSR2, // halt
    ];

    let mut signals = Signals::new(handled_signals)
        .map_err(|e| BloomError::Custom(format!("Failed to register signals: {e}")))?;

    thread::spawn(move || {
        let timer = ProcessTimer::start();

        for signal in signals.forever() {
            match signal {
                SIGCHLD => {
                    loop {
                        match waitpid(None, Some(WaitPidFlag::WNOHANG)) {
                            Ok(WaitStatus::Exited(pid, code)) => {
                                let msg = format!("Reaped child PID {} (exit code {})", pid, code);
                                if let Ok(mut log) = file_logger.lock() {
                                    log.log(LogLevel::Info, &msg);
                                }
                            }
                            Ok(WaitStatus::Signaled(pid, sig, _)) => {
                                let msg = format!("Reaped child PID {} (signal {})", pid, sig);
                                if let Ok(mut log) = file_logger.lock() {
                                    log.log(LogLevel::Info, &msg);
                                }
                            }
                            Ok(WaitStatus::StillAlive) => break,
                            Ok(_) => continue,
                            Err(_) => break,
                        }
                    }
                }

                SIGTERM | SIGINT | SIGPWR | SIGUSR2 => {
                    let msg = match signal {
                        SIGTERM => "Received SIGTERM",
                        SIGINT => "Received SIGINT",
                        SIGPWR => "Received SIGPWR (ACPI power event)",
                        SIGUSR2 => "Received SIGUSR2 (halt request)",
                        _ => "Received shutdown signal",
                    };

                    if let Ok(mut log) = file_logger.lock() {
                        log.log(LogLevel::Warn, &format!("{}, scheduling shutdown", msg));
                    }
                    if let Ok(mut con) = console_logger.lock() {
                        con.message(LogLevel::Warn, &format!("{}, scheduling shutdown", msg), timer.elapsed());
                    }

                    shutdown_flag.store(true, Ordering::SeqCst);
                    main_thread.unpark();
                    break;
                }

                SIGUSR1 => {
                    let msg = "Received SIGUSR1 (reboot request)";
                    if let Ok(mut log) = file_logger.lock() {
                        log.log(LogLevel::Warn, msg);
                    }
                    if let Ok(mut con) = console_logger.lock() {
                        con.message(LogLevel::Warn, msg, timer.elapsed());
                    }

                    // If you have a reboot flag instead, you'd use that here:
                    shutdown_flag.store(true, Ordering::SeqCst);
                    main_thread.unpark();
                    break;
                }

                _ => {}
            }
        }
    });

    Ok(())
}

