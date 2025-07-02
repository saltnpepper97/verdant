use std::fs::OpenOptions;
use std::process::{Command, Child, Stdio};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::{LogLevel, ServiceState};

use crate::process::stop_service;
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

        let cmd = &self.service.cmd;
        let args: Vec<&str> = match &self.service.args {
            Some(vec) => vec.iter().map(|s| s.as_str()).collect(),
            None => vec![],
        };

        // Special handling for tty@ services: bind /dev/ttyX to stdin/stdout/stderr
        if self.service.name.starts_with("tty@") {
            let tty_id = self.service.name.strip_prefix("tty@").ok_or_else(|| {
                BloomError::Custom("Invalid tty@ service name".to_string())
            })?;
            let tty_path = format!("/dev/{}", tty_id);

            let tty = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&tty_path)
                .map_err(|e| BloomError::Custom(format!("Failed to open {}: {}", tty_path, e)))?;

            let tty_clone1 = tty.try_clone().map_err(|e| {
                BloomError::Custom(format!("Failed to clone {}: {}", tty_path, e))
            })?;
            let tty_clone2 = tty.try_clone().map_err(|e| {
                BloomError::Custom(format!("Failed to clone {}: {}", tty_path, e))
            })?;

            let mut command = Command::new(cmd);
            command.args(&args);
            command.stdin(Stdio::from(tty));
            command.stdout(Stdio::from(tty_clone1));
            command.stderr(Stdio::from(tty_clone2));

            let child = command.spawn().map_err(|e| {
                BloomError::Custom(format!("Failed to start service '{}': {}", self.service.name, e))
            })?;

            self.child = Some(child);
            self.last_start = Some(Instant::now());
            self.restart_count = 0;

            return Ok(());
        }

        // Non-tty services: use start_service helper as before
        let launch_result = crate::process::start_service(&self.service, &mut *file)?;
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

    pub fn supervise_loop(
        &mut self,
        shutdown_flag: Arc<AtomicBool>,
    ) -> Result<(), BloomError> {
        let mut last_state = self.state;

        if self.child.is_none() {
            self.state = ServiceState::Starting;
            if let Err(e) = self.start() {
                self.state = ServiceState::Failed;
                return Err(e);
            }
        }

        loop {
            if shutdown_flag.load(Ordering::SeqCst) {
                {
                    let mut file = self.file_logger.lock().unwrap();
                    file.log(LogLevel::Info, &format!(
                        "Shutdown flag detected. Attempting graceful stop of '{}'", self.service.name
                    ));
                }

                // Signal stop to service
                let _ = self.shutdown();

                // Actively wait for child to exit with timeout, forcibly kill if needed
                let timeout = Duration::from_secs(5);
                let start = Instant::now();

                loop {
                    match self.child.as_mut() {
                        Some(child) => match child.try_wait() {
                            Ok(Some(_status)) => {
                                self.child = None;
                                self.state = ServiceState::Stopped;
                                let mut file = self.file_logger.lock().unwrap();
                                file.log(LogLevel::Info, &format!(
                                    "Service '{}' stopped cleanly on shutdown.", self.service.name
                                ));
                                break;
                            }
                            Ok(None) => {
                                if start.elapsed() > timeout {
                                    let mut file = self.file_logger.lock().unwrap();
                                    file.log(LogLevel::Warn, &format!(
                                        "Timeout waiting for service '{}' to stop; sending SIGKILL", self.service.name
                                    ));

                                    // Force kill the child process
                                    #[cfg(unix)]
                                    {
                                        use nix::sys::signal::{kill, Signal};
                                        use nix::unistd::Pid;

                                        let _ = kill(Pid::from_raw(child.id() as i32), Signal::SIGKILL);
                                    }

                                    thread::sleep(Duration::from_millis(200));
                                } else {
                                    thread::sleep(Duration::from_millis(100));
                                }
                            }
                            Err(e) => {
                                let mut file = self.file_logger.lock().unwrap();
                                file.log(LogLevel::Fail, &format!(
                                    "Error waiting for service '{}': {}", self.service.name, e
                                ));
                                break;
                            }
                        },
                        None => break,
                    }
                }

                break; // exit supervise_loop after shutdown completes
            }

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

                                if shutdown_flag.load(Ordering::SeqCst) {
                                    let _ = self.shutdown();
                                    break;
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

                                    if shutdown_flag.load(Ordering::SeqCst) {
                                        let _ = self.shutdown();
                                        break;
                                    }

                                    self.state = ServiceState::Starting;
                                    if let Err(e) = self.start() {
                                        self.state = ServiceState::Failed;
                                        return Err(e);
                                    }
                                    ServiceState::Starting
                                } else {
                                    break;
                                }
                            }
                            RestartPolicy::Never => break,
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
                None => break,
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

