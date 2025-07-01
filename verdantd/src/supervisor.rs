use std::process::Child;
use std::sync::{Arc, Mutex};
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

        let mut con = self.console_logger.lock().unwrap();
        let mut file = self.file_logger.lock().unwrap();

        self.state = ServiceState::Starting;
        let launch_result = start_service(&self.service, &mut *con, &mut *file)?;
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
            self.child = None;
            self.state = ServiceState::Stopped;
            Ok(())
        } else {
            Err(BloomError::Custom(format!(
                "Service '{}' not running",
                self.service.name
            )))
        }
    }

    pub fn shutdown(&mut self) -> Result<(), BloomError> {
        let result = self.stop();

        let mut file = self.file_logger.lock().unwrap();
        let msg = if result.is_ok() {
            format!("Supervisor shutdown: '{}'", self.service.name)
        } else {
            format!("Supervisor shutdown failed: '{}'", self.service.name)
        };
        file.log(LogLevel::Info, &msg);

        result
    }

    pub fn supervise_loop(&mut self) -> Result<(), BloomError> {
        let mut last_state = self.state;

        if self.child.is_none() {
            self.state = ServiceState::Starting;
            if let Err(e) = self.start() {
                self.state = ServiceState::Failed;
                return Err(e);
            }
        }

        loop {
            let current_state = match self.child.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(Some(status)) => {
                        {
                            let mut file = self.file_logger.lock().unwrap();
                            let msg = format!(
                                "Service '{}' exited with status {}",
                                self.service.name, status
                            );
                            file.log(LogLevel::Warn, &msg);
                        }

                        self.child = None;

                        match self.service.restart {
                            RestartPolicy::Always => {
                                self.restart_count += 1;
                                if let Some(delay) = self.service.restart_delay {
                                    if delay > 0 {
                                        thread::sleep(Duration::from_secs(delay));
                                    }
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
                                    if let Some(delay) = self.service.restart_delay {
                                        if delay > 0 {
                                            thread::sleep(Duration::from_secs(delay));
                                        }
                                    }
                                    self.state = ServiceState::Starting;
                                    if let Err(e) = self.start() {
                                        self.state = ServiceState::Failed;
                                        return Err(e);
                                    }
                                    ServiceState::Starting
                                } else {
                                    break; // exit supervise loop on clean exit
                                }
                            }
                            RestartPolicy::Never => break, // exit supervise loop
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
                            "Failed to wait for service {}: {}",
                            self.service.name, e
                        )));
                    }
                },
                None => break, // no child, nothing to supervise
            };

            if current_state != last_state {
                let mut file = self.file_logger.lock().unwrap();
                let msg = format!(
                    "Service '{}' state changed: {:?} → {:?}",
                    self.service.name, last_state, current_state
                );
                file.log(LogLevel::Info, &msg);
                last_state = current_state;
            }

            self.state = current_state;
            thread::sleep(Duration::from_millis(500));
        }

        Ok(())
    }
}

