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

    pub fn supervise_loop(&mut self, shutdown_flag: Arc<AtomicBool>) -> Result<(), BloomError> {
        if self.child.is_none() {
            self.state = ServiceState::Starting;
            if let Err(e) = self.start() {
                self.state = ServiceState::Failed;
                return Err(e);
            }
        }

        loop {
            if shutdown_flag.load(Ordering::SeqCst) {
                let _ = self.shutdown();

                let deadline = Instant::now() + Duration::from_secs(5);
                while Instant::now() < deadline {
                    if let Some(child) = &mut self.child {
                        match child.try_wait() {
                            Ok(Some(_)) => {
                                self.child = None;
                                self.state = ServiceState::Stopped;
                                break;
                            }
                            Ok(None) => thread::sleep(Duration::from_millis(100)),
                            Err(_) => break,
                        }
                    } else {
                        break;
                    }
                }

                if self.child.is_some() {
                    {
                        let mut file = self.file_logger.lock().unwrap();
                        file.log(LogLevel::Warn, &format!(
                            "Service '{}' did not stop in time. Sending SIGKILL.", self.service.name
                        ));
                    }

                    #[cfg(unix)]
                    {
                        use nix::sys::signal::{kill, Signal};
                        use nix::unistd::Pid;
                        let _ = kill(Pid::from_raw(self.child.as_ref().unwrap().id() as i32), Signal::SIGKILL);
                    }

                    #[cfg(windows)]
                    {
                        let _ = self.child.as_mut().unwrap().kill();
                    }

                    thread::sleep(Duration::from_millis(300));
                    self.child = None;
                    self.state = ServiceState::Stopped;
                }

                return Ok(());
            }

            if let Some(child) = &mut self.child {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        self.child = None;

                        {
                            let mut file = self.file_logger.lock().unwrap();
                            file.log(LogLevel::Warn, &format!(
                                "Service '{}' exited with status {}", self.service.name, status
                            ));
                        }

                        match self.service.restart {
                            RestartPolicy::Always => {
                                self.restart_count += 1;
                                if let Some(delay) = self.service.restart_delay {
                                    thread::sleep(Duration::from_secs(delay));
                                }
                                self.start()?;
                            }
                            RestartPolicy::OnFailure => {
                                if !status.success() {
                                    self.restart_count += 1;
                                    if let Some(delay) = self.service.restart_delay {
                                        thread::sleep(Duration::from_secs(delay));
                                    }
                                    self.start()?;
                                } else {
                                    self.state = ServiceState::Stopped;
                                    return Ok(());
                                }
                            }
                            RestartPolicy::Never => {
                                self.state = ServiceState::Stopped;
                                return Ok(());
                            }
                        }
                    }
                    Ok(None) => {
                        if self.state == ServiceState::Starting {
                            self.state = ServiceState::Running;
                        }
                    }
                    Err(e) => {
                        self.state = ServiceState::Failed;
                        return Err(BloomError::Custom(format!(
                            "Failed to wait on service '{}': {}", self.service.name, e
                        )));
                    }
                }
            }

            thread::sleep(Duration::from_millis(250));
        }
    }
}

