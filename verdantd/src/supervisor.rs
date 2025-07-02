use std::process::{Child, Command};
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
            Err(BloomError::Custom(format!(
                "Service '{}' not running",
                self.service.name
            )))
        }
    }

    pub fn shutdown(&mut self) -> Result<(), BloomError> {
        let result = self.stop();

        {
            let mut file = self.file_logger.lock().unwrap();
            let msg = if result.is_ok() {
                format!("Supervisor shutdown: '{}'", self.service.name)
            } else {
                format!("Supervisor shutdown failed: '{}'", self.service.name)
            };
            file.log(LogLevel::Info, &msg);
        }

        result
    }

    /// Checks if the service is a tty@ instance (e.g. "tty@tty1")
    fn is_tty_instance(&self) -> Option<String> {
        // Expecting service.name like "tty@tty1"
        if let Some(pos) = self.service.name.find('@') {
            let prefix = &self.service.name[..pos];
            if prefix == "tty" {
                let tty_id = &self.service.name[pos + 1..];
                return Some(tty_id.to_string());
            }
        }
        None
    }

    /// Uses `fuser` command to check if a tty device is in use by any process
    fn is_tty_in_use(&self, tty_id: &str) -> bool {
        let dev_path = format!("/dev/{}", tty_id);

        let output = Command::new("fuser")
            .arg(&dev_path)
            .output();

        match output {
            Ok(out) => {
                // fuser returns exit status 0 if any processes are using the file,
                // and non-zero if none
                out.status.success()
            }
            Err(_) => {
                // If fuser is missing or fails, be conservative and say not in use
                false
            }
        }
    }

    pub fn supervise_loop(&mut self, shutdown_flag: Arc<AtomicBool>) -> Result<(), BloomError> {
        let mut last_state = self.state;

        // Start service initially if not running and not a tty@ in-use service
        if self.child.is_none() {
            self.state = ServiceState::Starting;

            // If tty@ instance and in use, do not start yet
            if let Some(tty_id) = self.is_tty_instance() {
                if self.is_tty_in_use(&tty_id) {
                    self.state = ServiceState::Stopped;
                    {
                        let mut file = self.file_logger.lock().unwrap();
                        file.log(
                            LogLevel::Info,
                            &format!(
                                "TTY '{}' is in use on startup; not starting service '{}'",
                                tty_id, self.service.name
                            ),
                        );
                    }
                } else {
                    if let Err(e) = self.start() {
                        self.state = ServiceState::Failed;
                        return Err(e);
                    }
                }
            } else {
                if let Err(e) = self.start() {
                    self.state = ServiceState::Failed;
                    return Err(e);
                }
            }
        }

        loop {
            if shutdown_flag.load(Ordering::SeqCst) {
                {
                    let mut file = self.file_logger.lock().unwrap();
                    file.log(
                        LogLevel::Info,
                        &format!(
                            "Shutdown flag detected. Attempting graceful stop of '{}'",
                            self.service.name
                        ),
                    );
                }

                let _ = self.shutdown();
                self.wait_for_exit_with_timeout(Duration::from_secs(5));
                break;
            }

            // If this is a tty@ service, handle login/logout user detection and start/stop accordingly
            if let Some(tty_id) = self.is_tty_instance() {
                let in_use = self.is_tty_in_use(&tty_id);

                if in_use {
                    // TTY is in use - stop service if running
                    if self.state != ServiceState::Stopped {
                        let mut file = self.file_logger.lock().unwrap();
                        file.log(
                            LogLevel::Info,
                            &format!(
                                "TTY '{}' is in use - stopping service '{}'",
                                tty_id, self.service.name
                            ),
                        );
                        drop(file);
                        let _ = self.stop();
                    }
                } else {
                    // TTY not in use - start service if restart: always and currently stopped
                    if self.state == ServiceState::Stopped
                        && self.service.restart == RestartPolicy::Always
                    {
                        let mut file = self.file_logger.lock().unwrap();
                        file.log(
                            LogLevel::Info,
                            &format!(
                                "TTY '{}' is free - starting service '{}'",
                                tty_id, self.service.name
                            ),
                        );
                        drop(file);

                        if let Err(e) = self.start() {
                            self.state = ServiceState::Failed;
                            return Err(e);
                        }
                    }
                }
            } else {
                // Not a tty@ service - proceed with normal supervise logic

                let current_state = match self.child.as_mut() {
                    Some(child) => match child.try_wait() {
                        Ok(Some(status)) => {
                            {
                                let mut file = self.file_logger.lock().unwrap();
                                file.log(
                                    LogLevel::Warn,
                                    &format!(
                                        "Service '{}' exited with status {}",
                                        self.service.name, status
                                    ),
                                );
                            }

                            self.child = None;

                            // Exit immediately if shutdown in progress
                            if shutdown_flag.load(Ordering::SeqCst) {
                                break;
                            }

                            match self.service.restart {
                                RestartPolicy::Always => {
                                    self.restart_count += 1;
                                    if let Some(delay) = self.service.restart_delay {
                                        thread::sleep(Duration::from_secs(delay));
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
                                            thread::sleep(Duration::from_secs(delay));
                                        }

                                        self.state = ServiceState::Starting;
                                        if let Err(e) = self.start() {
                                            self.state = ServiceState::Failed;
                                            return Err(e);
                                        }
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
                                "Failed to wait for service {}: {}",
                                self.service.name, e
                            )));
                        }
                    },
                    None => break,
                };

                if current_state != last_state {
                    {
                        let mut file = self.file_logger.lock().unwrap();
                        file.log(
                            LogLevel::Info,
                            &format!(
                                "Service '{}' state changed: {:?} → {:?}",
                                self.service.name, last_state, current_state
                            ),
                        );
                    }
                    last_state = current_state;
                }

                self.state = current_state;
            }

            thread::sleep(Duration::from_millis(500));
        }

        Ok(())
    }

    fn wait_for_exit_with_timeout(&mut self, timeout: Duration) {
        let start = Instant::now();
        loop {
            match self.child.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(Some(_)) => {
                        self.child = None;
                        self.state = ServiceState::Stopped;
                        {
                            let mut file = self.file_logger.lock().unwrap();
                            file.log(
                                LogLevel::Info,
                                &format!(
                                    "Service '{}' stopped cleanly on shutdown.",
                                    self.service.name
                                ),
                            );
                        }
                        break;
                    }
                    Ok(None) => {
                        if start.elapsed() > timeout {
                            {
                                let mut file = self.file_logger.lock().unwrap();
                                file.log(
                                    LogLevel::Warn,
                                    &format!(
                                        "Timeout waiting for service '{}' to stop; sending SIGKILL",
                                        self.service.name
                                    ),
                                );
                            }
                            #[cfg(unix)]
                            {
                                use nix::sys::signal::{kill, Signal};
                                use nix::unistd::Pid;
                                let _ = kill(Pid::from_raw(child.id() as i32), Signal::SIGKILL);
                            }
                            thread::sleep(Duration::from_millis(200));
                            break;
                        } else {
                            thread::sleep(Duration::from_millis(100));
                        }
                    }
                    Err(e) => {
                        {
                            let mut file = self.file_logger.lock().unwrap();
                            file.log(
                                LogLevel::Fail,
                                &format!(
                                    "Error waiting for service '{}': {}",
                                    self.service.name, e
                                ),
                            );
                        }
                        break;
                    }
                },
                None => break,
            }
        }
    }
}

