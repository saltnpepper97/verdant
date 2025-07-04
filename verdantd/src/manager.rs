use std::thread;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

use bloom::errors::BloomError;
use bloom::log::{FileLogger, ConsoleLogger};

use crate::loader::load_services;
use crate::supervisor::Supervisor;
use crate::shutdown;

pub struct Manager {
    supervisors: Vec<Arc<Mutex<Supervisor>>>,
    running: Arc<AtomicBool>,
}

impl Manager {
    /// Takes both file logger and console logger.
    pub fn new(logger: &mut dyn FileLogger) -> Self {
        let (services, _loaded_count, _failed_count) = load_services(logger);

        let supervisors = services
            .into_iter()
            .map(|service| Arc::new(Mutex::new(Supervisor::new(service))))
            .collect();

        Self {
            supervisors,
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    /// Starts supervising all services concurrently.
    pub fn start_all(&self) {
        let running = self.running.clone();

        for supervisor in &self.supervisors {
            let sup = supervisor.clone();
            let running = running.clone();

            thread::spawn(move || {
                let mut sup = sup.lock().unwrap();

                // Run the supervise loop until manager is stopped
                while running.load(Ordering::Relaxed) {
                    if let Err(e) = sup.supervise_loop(running.clone()) {
                        eprintln!("Supervisor error for {}: {:?}", sup.service.name, e);
                    }
                }

                // On exit, ensure service is stopped cleanly
                let _ = sup.stop();
            });
        }
    }

    /// Starts only services whose startup package matches one in `allowed_startups`.
    /// Logs to both file and console loggers.
    pub fn start_startup_services(
        &self,
        allowed_startups: &[&str],
        file_logger: &mut dyn FileLogger,
        console_logger: &mut dyn ConsoleLogger,
    ) {
        let running = self.running.clone();

        let mut matched_count = 0;

        for supervisor in &self.supervisors {
            let sup = supervisor.clone();
            let startup_str = sup.lock().unwrap().service.startup.as_str();

            if allowed_startups.contains(&startup_str) {
                matched_count += 1;

                // Log the matched service startup package to both loggers
                let msg = format!("Starting service '{}' in startup package '{}'", sup.lock().unwrap().service.name, startup_str);
                file_logger.log(bloom::status::LogLevel::Info, &msg);
                console_logger.message(bloom::status::LogLevel::Info, &msg, std::time::Duration::from_secs(0));

                let running = running.clone();
                thread::spawn(move || {
                    let mut sup = sup.lock().unwrap();

                    while running.load(Ordering::Relaxed) {
                        if let Err(e) = sup.supervise_loop(running.clone()) {
                            eprintln!("Supervisor error for {}: {:?}", sup.service.name, e);
                        }
                    }

                    let _ = sup.stop();
                });
            }
        }

        if matched_count == 0 {
            for startup in allowed_startups {
                let msg = format!("No services found for startup package '{}'", startup);
                file_logger.log(bloom::status::LogLevel::Warn, &msg);
                console_logger.message(bloom::status::LogLevel::Warn, &msg, std::time::Duration::from_secs(0));
            }
        }
    }

    /// Stops all supervisors and services cleanly.
    pub fn stop_all(&self) {
        self.running.store(false, Ordering::Relaxed);

        for supervisor in &self.supervisors {
            if let Ok(mut sup) = supervisor.lock() {
                let _ = sup.stop();
            }
        }
    }

    /// Clean shutdown, waits for supervisors to stop and returns errors if any.
    pub fn shutdown_all_services(&self) -> Result<(), BloomError> {
        self.running.store(false, Ordering::Relaxed);

        shutdown::shutdown_all(&self.supervisors)
    }
}

