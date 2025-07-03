use std::process::Child;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::{LogLevel, ServiceState};

use crate::process::{start_service, stop_service};
use crate::service_file::{RestartPolicy, ServiceFile};

pub struct Supervisor {
    pub service: ServiceFile,
    pub child: Option<Child>,
    pub restart_count: u32,
    pub last_start: Option<Instant>,
    pub state: ServiceState,

    console_logger: Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
    file_logger: Arc<Mutex<dyn FileLogger + Send + Sync>>,
}

impl Supervisor {
    pub fn new(
        service: ServiceFile,
        console_logger: Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
        file_logger: Arc<Mutex<dyn FileLogger + Send + Sync>>,
    ) -> Self {
        Self {
            service,
            child: None,
            restart_count: 0,
            last_start: None,
            state: ServiceState::Stopped,
            console_logger,
            file_logger,
        }
    }

    pub fn start(&mut self) -> Result<(), BloomError> {
        if self.child.is_some() {
            return Err(BloomError::Custom(format!(
                "Service '{}' already running",
                self.service.name
            )));
        }

        self.state = ServiceState::Starting;
        {
            let mut file = self.file_logger.lock().unwrap();
            let launch = start_service(&self.service, &mut *file)?;
            self.child = Some(launch.child);
            self.last_start = Some(launch.start_time);
            self.restart_count = 0;
        }
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), BloomError> {
        if let Some(child) = &mut self.child {
            self.state = ServiceState::Stopping;
            {
                let mut con = self.console_logger.lock().unwrap();
                let mut file = self.file_logger.lock().unwrap();
                stop_service(&self.service, child.id(), &mut *con, &mut *file)?;
            }

            // Wait up to 2 seconds for process exit (check every 100ms)
            for _ in 0..20 {
                match child.try_wait() {
                    Ok(Some(_)) => {
                        self.child = None;
                        self.state = ServiceState::Stopped;
                        return Ok(());
                    }
                    Ok(None) => thread::sleep(Duration::from_millis(100)),
                    Err(e) => {
                        return Err(BloomError::Custom(format!(
                            "Error waiting for service '{}': {}",
                            self.service.name, e
                        )));
                    }
                }
            }

            // Still running? Force kill
            nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(child.id() as i32),
                nix::sys::signal::Signal::SIGKILL,
            ).map_err(|e| BloomError::Custom(format!("Failed to kill service '{}': {}", self.service.name, e)))?;

            // Wait again after kill, max 1 second
            for _ in 0..10 {
                match child.try_wait() {
                    Ok(Some(_)) => {
                        self.child = None;
                        self.state = ServiceState::Stopped;
                        return Ok(());
                    }
                    Ok(None) => thread::sleep(Duration::from_millis(100)),
                    Err(e) => {
                        return Err(BloomError::Custom(format!(
                            "Error waiting after kill for service '{}': {}",
                            self.service.name, e
                        )));
                    }
                }
            }

            Err(BloomError::Custom(format!(
                "Service '{}' did not stop in time",
                self.service.name
            )))
        } else {
            self.state = ServiceState::Stopped;
            Ok(())
        }
    }

    pub fn shutdown(&mut self) -> Result<(), BloomError> {
        let res = self.stop();
        {
            let mut file = self.file_logger.lock().unwrap();
            match &res {
                Ok(()) => file.log(LogLevel::Info, &format!("Supervisor shutdown: '{}'", self.service.name)),
                Err(e) => file.log(LogLevel::Fail, &format!("Supervisor shutdown error: '{}': {}", self.service.name, e)),
            }
        }
        res
    }

    pub fn supervise_loop(&mut self, shutdown_flag: Arc<AtomicBool>) -> Result<(), BloomError> {
        if self.child.is_none() {
            self.start().map_err(|e| {
                self.state = ServiceState::Failed;
                e
            })?;
        }

        let mut last_state = self.state;

        while !shutdown_flag.load(Ordering::SeqCst) {
            let current_state = match self.child.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(Some(status)) => {
                        {
                            let mut file = self.file_logger.lock().unwrap();
                            file.log(
                                LogLevel::Warn,
                                &format!("Service '{}' exited with status {:?}", self.service.name, status),
                            );
                        }
                        self.child = None;

                        if shutdown_flag.load(Ordering::SeqCst) {
                            break;
                        }

                        match self.service.restart {
                            RestartPolicy::Always => {
                                self.restart_count = self.restart_count.saturating_add(1);
                                if let Some(delay) = self.service.restart_delay {
                                    thread::sleep(Duration::from_secs(delay));
                                }
                                self.start().map_err(|e| {
                                    self.state = ServiceState::Failed;
                                    e
                                })?;
                                ServiceState::Starting
                            }
                            RestartPolicy::OnFailure => {
                                if !status.success() {
                                    self.restart_count = self.restart_count.saturating_add(1);
                                    if let Some(delay) = self.service.restart_delay {
                                        thread::sleep(Duration::from_secs(delay));
                                    }
                                    self.start().map_err(|e| {
                                        self.state = ServiceState::Failed;
                                        e
                                    })?;
                                    ServiceState::Starting
                                } else {
                                    ServiceState::Stopped
                                }
                            }
                            RestartPolicy::Never => ServiceState::Stopped,
                        }
                    }
                    Ok(None) => {
                        if self.state == ServiceState::Starting {
                            ServiceState::Running
                        } else {
                            self.state
                        }
                    }
                    Err(e) => {
                        self.state = ServiceState::Failed;
                        return Err(BloomError::Custom(format!(
                            "Failed to wait for service '{}': {}", self.service.name, e
                        )));
                    }
                },
                None => ServiceState::Stopped,
            };

            if current_state != last_state {
                let mut file = self.file_logger.lock().unwrap();
                file.log(
                    LogLevel::Info,
                    &format!(
                        "Service '{}' state changed: {:?} → {:?}",
                        self.service.name, last_state, current_state
                    ),
                );
                last_state = current_state;
            }

            self.state = current_state;

            if current_state == ServiceState::Stopped {
                break;
            }

            thread::sleep(Duration::from_millis(500));
        }

        // On shutdown signal, ensure service is stopped cleanly
        if shutdown_flag.load(Ordering::SeqCst) {
            let _ = self.stop();
        }

        Ok(())
    }
}

