use std::fs;
use std::path::Path;
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

    restarting: bool,
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
            restarting: false,
        }
    }

    pub fn start(&mut self) -> Result<(), BloomError> {
        if self.child.is_some() {
            return Err(BloomError::Custom(format!("Service '{}' already running", self.service.name)));
        }

        self.set_state(ServiceState::Starting);

        let launch = {
            let mut file = self.file_logger.lock().unwrap();
            start_service(&self.service, &mut *file)?
        };

        self.child = Some(launch.child);
        self.last_start = Some(launch.start_time);
        self.restart_count = 0;
        self.restarting = false;

        self.set_state(ServiceState::Running);
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), BloomError> {
        self.set_state(ServiceState::Stopping);

        if let Some(child) = self.child.take() {
            let pid = child.id();
            let mut con = self.console_logger.lock().unwrap();
            let mut file = self.file_logger.lock().unwrap();

            if let Err(e) = stop_service(&self.service, pid, &mut *con, &mut *file) {
                let err_msg = format!("Error stopping service '{}': {}", self.service.name, e);
                con.message(LogLevel::Warn, &err_msg, Duration::ZERO);
                file.log(LogLevel::Warn, &err_msg);
            }
        }

        self.set_state(ServiceState::Stopped);
        Ok(())
    }

    pub fn shutdown(&mut self) -> Result<(), BloomError> {
        // Skip stopping if tty@ and logged in, assume service exits cleanly on shutdown
        if self.service.name.starts_with("tty@") && self.is_tty_logged_in() {
            {
                let mut file = self.file_logger.lock().unwrap();
                file.log(LogLevel::Info, &format!("Skipping stop for logged-in '{}'", self.service.name));
            } 
            self.set_state(ServiceState::Stopped);
            return Ok(());
        }

        let result = self.stop();
        let mut file = self.file_logger.lock().unwrap();
        match &result {
            Ok(_) => file.log(LogLevel::Info, &format!("Supervisor shutdown: '{}'", self.service.name)),
            Err(e) => file.log(LogLevel::Fail, &format!("Shutdown failed for '{}': {}", self.service.name, e)),
        }
        result
    }

    fn set_state(&mut self, new_state: ServiceState) {
        if self.state != new_state {
            self.state = new_state;
            let mut file = self.file_logger.lock().unwrap();
            file.log(LogLevel::Info, &format!("Service '{}' state changed to {:?}", self.service.name, self.state));
        }
    }

    fn child_has_exited(&mut self) -> Option<bool> {
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.child = None;
                    if status.success() {
                        self.set_state(ServiceState::Stopped);
                        Some(true)
                    } else {
                        self.set_state(ServiceState::Failed);
                        Some(false)
                    }
                }
                Ok(None) => Some(false),
                Err(_) => {
                    self.child = None;
                    self.set_state(ServiceState::Failed);
                    Some(false)
                }
            }
        } else {
            Some(true)
        }
    }

    fn is_tty_logged_in(&self) -> bool {
        if !self.service.name.starts_with("tty@") {
            return false;
        }

        let tty_instance = self.service.name.split('@').nth(1).unwrap_or("");
        if tty_instance.is_empty() {
            return false;
        }

        let tty_path = format!("/dev/{}", tty_instance);
        let proc_dir_iter = match fs::read_dir("/proc") {
            Ok(d) => d,
            Err(_) => return false,
        };

        for entry in proc_dir_iter.flatten() {
            let file_name = entry.file_name();
            let file_name_owned = file_name.to_string_lossy().to_string(); // fix for E0716
            let pid: u32 = match file_name_owned.parse() {
                Ok(n) => n,
                Err(_) => continue,
            };

            let fd_dir_path = format!("/proc/{}/fd", pid);
            let fd_dir = match fs::read_dir(fd_dir_path) {
                Ok(d) => d,
                Err(_) => continue,
            };

            for fd_entry in fd_dir.flatten() {
                if let Ok(link_target) = fs::read_link(fd_entry.path()) {
                    if link_target == Path::new(&tty_path) {
                        let comm_path = format!("/proc/{}/comm", pid);
                        if let Ok(comm) = fs::read_to_string(&comm_path) {
                            let proc_name = comm.trim();
                            if proc_name != "getty" && proc_name != "agetty" {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        false
    }

    pub fn supervise_loop(&mut self, shutdown_flag: Arc<AtomicBool>) -> Result<(), BloomError> {
        if self.child.is_none() {
            self.start()?;
        }

        loop {
            if shutdown_flag.load(Ordering::SeqCst) {
                break;
            }


            match self.child_has_exited() {
                Some(true) => {
                    if shutdown_flag.load(Ordering::SeqCst) || self.service.restart == RestartPolicy::Never {
                        break;
                    }
                    self.set_state(ServiceState::Starting);
                }
                Some(false) => {
                    self.set_state(ServiceState::Starting);
                }
                None => {
                    if shutdown_flag.load(Ordering::SeqCst) {
                        break;
                    }
                }
            }

            thread::sleep(Duration::from_millis(300));
        }

        self.shutdown()?;
        Ok(())
    }
}

