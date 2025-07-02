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
        let launch_result = {
            let mut file = self.file_logger.lock().unwrap();
            start_service(&self.service, &mut *file)?
        };

        self.child = Some(launch_result.child);
        self.last_start = Some(launch_result.start_time);
        self.restart_count = 0;

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
        file.log(
            LogLevel::Info,
            &format!(
                "Supervisor shutdown{}: '{}'",
                if result.is_err() { " failed" } else { "" },
                self.service.name
            ),
        );
        result
    }

    pub fn supervise_loop(&mut self, shutdown_flag: Arc<AtomicBool>) -> Result<(), BloomError> {
        let mut last_state = self.state;

        // On first boot, only start tty@ service if TTY is not already in use
        if self.child.is_none() && !self.should_block_tty_start() {
            if let Err(e) = self.start() {
                self.state = ServiceState::Failed;
                return Err(e);
            }
        }

        loop {
            if shutdown_flag.load(Ordering::SeqCst) {
                {
                    let mut file = self.file_logger.lock().unwrap();
                    file.log(LogLevel::Info, &format!("Shutdown requested. Stopping '{}'", self.service.name));
                }
                let _ = self.shutdown();
                self.wait_for_exit_with_timeout(Duration::from_secs(5));
                break;
            }

            if let Some(tty_id) = self.is_tty_instance() {
                let in_use = self.is_tty_logged_in(&tty_id);

                // Stop if someone logs in
                if in_use && self.state != ServiceState::Stopped {
                    {
                        let mut file = self.file_logger.lock().unwrap();
                        file.log(LogLevel::Info, &format!("TTY '{}' in use. Stopping '{}'", tty_id, self.service.name));
                    }
                    let _ = self.stop();
                }

                // Restart after logout
                if !in_use && self.state == ServiceState::Stopped && self.service.restart == RestartPolicy::Always {
                    {
                        let mut file = self.file_logger.lock().unwrap();
                        file.log(LogLevel::Info, &format!("TTY '{}' free. Starting '{}'", tty_id, self.service.name));
                    }
                    if let Err(e) = self.start() {
                        self.state = ServiceState::Failed;
                        return Err(e);
                    }
                }
            } else {
                // Normal (non-tty@) service logic
                let current_state = match self.child.as_mut() {
                    Some(child) => match child.try_wait() {
                        Ok(Some(status)) => {
                            self.child = None;
                            self.handle_exit(status.code().unwrap_or(1), shutdown_flag.clone())?
                        }
                        Ok(None) => {
                            if self.state == ServiceState::Starting {
                                ServiceState::Running
                            } else {
                                self.state
                            }
                        }
                        Err(e) => {
                            return Err(BloomError::Custom(format!(
                                "Failed to wait for service '{}': {}",
                                self.service.name, e
                            )))
                        }
                    },
                    None => self.state,
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
            }

            thread::sleep(Duration::from_millis(500));
        }

        Ok(())
    }

    fn handle_exit(&mut self, exit_code: i32, shutdown_flag: Arc<AtomicBool>) -> Result<ServiceState, BloomError> {
        let mut file = self.file_logger.lock().unwrap();
        file.log(
            LogLevel::Warn,
            &format!("Service '{}' exited with code {}", self.service.name, exit_code),
        );
        drop(file);

        if shutdown_flag.load(Ordering::SeqCst) {
            return Ok(ServiceState::Stopped);
        }

        match self.service.restart {
            RestartPolicy::Always => {
                if let Some(delay) = self.service.restart_delay {
                    thread::sleep(Duration::from_secs(delay));
                }
                self.start()?;
                Ok(ServiceState::Starting)
            }
            RestartPolicy::OnFailure if exit_code != 0 => {
                if let Some(delay) = self.service.restart_delay {
                    thread::sleep(Duration::from_secs(delay));
                }
                self.start()?;
                Ok(ServiceState::Starting)
            }
            _ => Ok(ServiceState::Stopped),
        }
    }

    fn wait_for_exit_with_timeout(&mut self, timeout: Duration) {
        let start = Instant::now();
        loop {
            match self.child.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(Some(_)) => {
                        self.child = None;
                        self.state = ServiceState::Stopped;
                        break;
                    }
                    Ok(None) if start.elapsed() > timeout => {
                        #[cfg(unix)]
                        {
                            use nix::sys::signal::{kill, Signal};
                            use nix::unistd::Pid;
                            let _ = kill(Pid::from_raw(child.id() as i32), Signal::SIGKILL);
                        }
                        self.child = None;
                        self.state = ServiceState::Stopped;
                        break;
                    }
                    Ok(None) => thread::sleep(Duration::from_millis(100)),
                    Err(_) => break,
                },
                None => break,
            }
        }
    }

    fn is_tty_instance(&self) -> Option<String> {
        self.service.name.strip_prefix("tty@").map(|s| s.to_string())
    }

    /// Blocks start if tty is already in use (e.g. by a login shell)
    fn should_block_tty_start(&self) -> bool {
        self.is_tty_instance()
            .map(|tty| self.is_tty_logged_in(&tty))
            .unwrap_or(false)
    }

    /// Returns true if *any* user is currently logged into the given tty (e.g., tty1)
    fn is_tty_logged_in(&self, tty_id: &str) -> bool {
        let dev_path = format!("/dev/{}", tty_id);
        match Command::new("fuser").arg(&dev_path).output() {
            Ok(output) => !output.stdout.is_empty(),
            Err(_) => false, // fallback: assume not in use
        }
    }
}

