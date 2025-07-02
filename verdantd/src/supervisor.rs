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
        {
            let mut file = self.file_logger.lock().unwrap();
            let launch_result = start_service(&self.service, &mut *file)?;
            self.child = Some(launch_result.child);
            self.last_start = Some(launch_result.start_time);
        }
        self.restart_count = 0;

        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), BloomError> {
        if let Some(child) = &mut self.child {
            self.state = ServiceState::Stopping;

            let mut con = self.console_logger.lock().unwrap();
            let mut file = self.file_logger.lock().unwrap();
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
        let _ = self.stop(); // Always try to stop
        self.state = ServiceState::Stopped;

        let mut file = self.file_logger.lock().unwrap();
        file.log(
            LogLevel::Info,
            &format!("Supervisor shutdown: '{}'", self.service.name),
        );

        Ok(())
    }

    pub fn supervise_loop(&mut self, shutdown_flag: Arc<AtomicBool>) -> Result<(), BloomError> {
        let mut last_state = self.state;

        if self.child.is_none() {
            if !self.is_tty_logged_in() {
                if let Err(e) = self.start() {
                    self.state = ServiceState::Failed;
                    return Err(e);
                }
                self.state = ServiceState::Starting;
            } else {
                self.state = ServiceState::Stopped;
            }
        }

        loop {
            if shutdown_flag.load(Ordering::SeqCst) {
                let _ = self.shutdown();
                self.wait_for_exit_with_timeout(Duration::from_secs(5));
                break;
            }

            let current_state = match self.child.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(Some(status)) => {
                        self.child = None;

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

                        if shutdown_flag.load(Ordering::SeqCst) {
                            ServiceState::Stopped
                        } else if self.service.restart == RestartPolicy::Always {
                            if self.is_tty_logged_in() {
                                let mut file = self.file_logger.lock().unwrap();
                                file.log(
                                    LogLevel::Info,
                                    &format!(
                                        "TTY '{}' still has user logged in. Delaying restart.",
                                        self.service.name
                                    ),
                                );
                                ServiceState::Stopped
                            } else {
                                if let Some(delay) = self.service.restart_delay {
                                    thread::sleep(Duration::from_secs(delay));
                                }

                                // Drop file lock *before* mutable borrow for start()
                                drop(self.file_logger.lock().unwrap());

                                if let Err(e) = self.start() {
                                    self.state = ServiceState::Failed;
                                    return Err(e);
                                }
                                ServiceState::Starting
                            }
                        } else if self.service.restart == RestartPolicy::OnFailure
                            && !status.success()
                        {
                            if let Some(delay) = self.service.restart_delay {
                                thread::sleep(Duration::from_secs(delay));
                            }

                            // Drop file lock *before* mutable borrow for start()
                            drop(self.file_logger.lock().unwrap());

                            if let Err(e) = self.start() {
                                self.state = ServiceState::Failed;
                                return Err(e);
                            }
                            ServiceState::Starting
                        } else {
                            ServiceState::Stopped
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
                        return Err(BloomError::Custom(format!(
                            "Failed to wait for service '{}': {}",
                            self.service.name, e
                        )));
                    }
                },
                None => {
                    if self.service.restart == RestartPolicy::Always && !self.is_tty_logged_in() {
                        {
                            let mut file = self.file_logger.lock().unwrap();
                            file.log(
                                LogLevel::Info,
                                &format!("Restarting '{}' after logout.", self.service.name),
                            );
                        }

                        if let Err(e) = self.start() {
                            self.state = ServiceState::Failed;
                            return Err(e);
                        }
                        ServiceState::Starting
                    } else {
                        ServiceState::Stopped
                    }
                }
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
                    Ok(None) => {
                        if start.elapsed() > timeout {
                            #[cfg(unix)]
                            {
                                use nix::sys::signal::{kill, Signal};
                                use nix::unistd::Pid;
                                let _ = kill(Pid::from_raw(child.id() as i32), Signal::SIGKILL);
                            }
                            thread::sleep(Duration::from_millis(200));
                            self.child = None;
                            self.state = ServiceState::Stopped;
                            break;
                        } else {
                            thread::sleep(Duration::from_millis(100));
                        }
                    }
                    Err(_) => break,
                },
                None => break,
            }
        }
    }

    fn is_tty_logged_in(&self) -> bool {
        if let Some(tty_name) = self.service.name.strip_prefix("tty@") {
            let path = format!("/dev/{}", tty_name);
            if let Ok(output) = Command::new("fuser").arg(&path).output() {
                return !output.stdout.is_empty();
            }
        }
        false
    }
}

