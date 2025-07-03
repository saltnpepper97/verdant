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

        let mut file = self.file_logger.lock().unwrap();
        let launch_result = start_service(&self.service, &mut *file)?;
        drop(file);

        self.child = Some(launch_result.child);
        self.last_start = Some(launch_result.start_time);
        self.restart_count = 0;

        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), BloomError> {
        if let Some(child) = &mut self.child {
            let mut con = self.console_logger.lock().unwrap();
            let mut file = self.file_logger.lock().unwrap();

            self.state = ServiceState::Stopping;
            stop_service(&self.service, child.id(), &mut *con, &mut *file)?;
            drop(file);
            drop(con);

            self.child = None;
            self.state = ServiceState::Stopped;

            Ok(())
        } else {
            self.state = ServiceState::Stopped;
            Ok(())
        }
    }

    pub fn shutdown(&mut self) -> Result<(), BloomError> {
        match self.stop() {
            Ok(()) => {
                let mut file = self.file_logger.lock().unwrap();
                file.log(LogLevel::Info, &format!("Supervisor shutdown: '{}'", self.service.name));
                Ok(())
            }
            Err(e) => {
                let mut file = self.file_logger.lock().unwrap();
                file.log(LogLevel::Fail, &format!("Supervisor shutdown error: '{}': {}", self.service.name, e));
                Err(e)
            }
        }
    }

    pub fn supervise_loop(&mut self, shutdown_flag: Arc<AtomicBool>) -> Result<(), BloomError> {
        if self.child.is_none() {
            self.state = ServiceState::Starting;
            if let Err(e) = self.start() {
                self.state = ServiceState::Failed;
                return Err(e);
            }
        }

        let mut last_state = self.state;

        loop {
            if shutdown_flag.load(Ordering::SeqCst) {
                break;
            }

            let current_state = match self.child.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(Some(status)) => {
                        {
                            let mut file = self.file_logger.lock().unwrap();
                            file.log(
                                LogLevel::Warn,
                                &format!("Service '{}' exited with status {}", self.service.name, status),
                            );
                        }

                        self.child = None;

                        if shutdown_flag.load(Ordering::SeqCst) {
                            return Ok(());
                        }

                        match self.service.restart {
                            RestartPolicy::Always => {
                                self.restart_count += 1;

                                {
                                    let mut file = self.file_logger.lock().unwrap();
                                    file.log(
                                        LogLevel::Info,
                                        &format!(
                                            "Restarting service '{}' (attempt #{})",
                                            self.service.name, self.restart_count
                                        ),
                                    );
                                }

                                if let Some(delay) = self.service.restart_delay {
                                    thread::sleep(Duration::from_secs(delay));
                                }

                                if shutdown_flag.load(Ordering::SeqCst) {
                                    let _ = self.shutdown();
                                    return Ok(());
                                }

                                self.state = ServiceState::Starting;
                                if let Err(e) = self.start() {
                                    self.state = ServiceState::Failed;
                                    return Err(e);
                                }
                                ServiceState::Starting
                            }
                            RestartPolicy::OnFailure => {
                                if !status.success() {
                                    self.restart_count += 1;

                                    {
                                        let mut file = self.file_logger.lock().unwrap();
                                        file.log(
                                            LogLevel::Info,
                                            &format!(
                                                "Restarting service '{}' (attempt #{}) due to failure",
                                                self.service.name, self.restart_count
                                            ),
                                        );
                                    }

                                    if let Some(delay) = self.service.restart_delay {
                                        thread::sleep(Duration::from_secs(delay));
                                    }

                                    if shutdown_flag.load(Ordering::SeqCst) {
                                        let _ = self.shutdown();
                                        return Ok(());
                                    }

                                    self.state = ServiceState::Starting;
                                    if let Err(e) = self.start() {
                                        self.state = ServiceState::Failed;
                                        return Err(e);
                                    }
                                    ServiceState::Starting
                                } else {
                                    self.state = ServiceState::Stopped;
                                    break;
                                }
                            }
                            RestartPolicy::Never => {
                                self.state = ServiceState::Stopped;
                                break;
                            }
                        }
                    }
                    Ok(None) => match self.state {
                        ServiceState::Starting => {
                            self.state = ServiceState::Running;
                            ServiceState::Running
                        }
                        ServiceState::Running => ServiceState::Running,
                        other => other,
                    },
                    Err(e) => {
                        self.state = ServiceState::Failed;
                        return Err(BloomError::Custom(format!(
                            "Failed to wait for service {}: {}", self.service.name, e
                        )));
                    }
                },
                None => break,
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
            thread::sleep(Duration::from_millis(500));
        }

        Ok(())
    }
}

