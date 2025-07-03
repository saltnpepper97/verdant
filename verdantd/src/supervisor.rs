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

        self.state = ServiceState::Starting;
        {
            let mut file = self.file_logger.lock().unwrap();
            let launch = start_service(&self.service, &mut *file)?;
            self.child = Some(launch.child);
            self.last_start = Some(launch.start_time);
        }
        self.restart_count = 0;
        self.restarting = false;

        {
            let mut file = self.file_logger.lock().unwrap();
            file.log(LogLevel::Info, &format!("Service '{}' started", self.service.name));
        }
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), BloomError> {
        if let Some(mut child) = self.child.take() {
            self.state = ServiceState::Stopping;
            let pid = child.id();

            let mut con = self.console_logger.lock().unwrap();
            let mut file = self.file_logger.lock().unwrap();

            // Call stop_service to send signals and wait for process exit
            stop_service(&self.service, pid, &mut *con, &mut *file)?;

            // Wait explicitly for child process exit with timeout
            let wait_start = Instant::now();
            let wait_timeout = Duration::from_secs(3);

            loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        file.log(LogLevel::Info, &format!("Process '{}' exited with {:?}", self.service.name, status));
                        break;
                    }
                    Ok(None) => {
                        if wait_start.elapsed() > wait_timeout {
                            file.log(LogLevel::Warn, &format!("Process '{}' did not exit after {}s, sending SIGKILL", self.service.name, wait_timeout.as_secs()));
                            #[cfg(unix)]
                            {
                                use nix::sys::signal::{kill, Signal};
                                use nix::unistd::Pid;
                                let nix_pid = Pid::from_raw(pid as i32);
                                let _ = kill(nix_pid, Signal::SIGKILL);
                                file.log(LogLevel::Warn, &format!("Sent SIGKILL to '{}'", self.service.name));
                            }
                            break;
                        }
                        thread::sleep(Duration::from_millis(100));
                    }
                    Err(e) => {
                        file.log(LogLevel::Fail, &format!("Failed to wait on process '{}': {}", self.service.name, e));
                        break;
                    }
                }
            }
        }

        self.state = ServiceState::Stopped;
        Ok(())
    }

    pub fn shutdown(&mut self) -> Result<(), BloomError> {
        let result = self.stop();
        {
            let mut file = self.file_logger.lock().unwrap();
            match &result {
                Ok(_) => file.log(LogLevel::Info, &format!("Supervisor shutdown: '{}'", self.service.name)),
                Err(e) => file.log(LogLevel::Fail, &format!("Shutdown failed for '{}': {}", self.service.name, e)),
            }
        }
        result
    }

    fn child_has_exited(&mut self) -> Option<bool> {
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.child = None;
                    if status.success() {
                        self.state = ServiceState::Stopped;
                        Some(true)
                    } else {
                        self.state = ServiceState::Failed;
                        Some(false)
                    }
                }
                Ok(None) => None,
                Err(_) => {
                    self.child = None;
                    self.state = ServiceState::Failed;
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

        let proc_dir = match fs::read_dir("/proc") {
            Ok(d) => d,
            Err(_) => return false,
        };

        for entry in proc_dir.flatten() {
            let file_name = entry.file_name();
            let pid_str = match file_name.to_str() {
                Some(s) => s,
                None => continue,
            };
            let pid: u32 = match pid_str.parse() {
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

        let mut _logged_clean_exit = false;

        while !shutdown_flag.load(Ordering::SeqCst) {
            if self.service.name.starts_with("tty@") && self.is_tty_logged_in() {
                if self.state != ServiceState::Stopped {
                    self.state = ServiceState::Stopped;
                    let mut file = self.file_logger.lock().unwrap();
                    file.log(LogLevel::Info, &format!("Service '{}' stopped due to active user login", self.service.name));
                }
                thread::sleep(Duration::from_secs(1));
                continue;
            }

            match self.child_has_exited() {
                None => {
                    self.restarting = false;
                }
                Some(true) => {
                    if !_logged_clean_exit {
                        let mut file = self.file_logger.lock().unwrap();
                        file.log(
                            LogLevel::Info,
                            &format!("Service '{}' exited cleanly (status 0)", self.service.name),
                        );
                        _logged_clean_exit = true;
                    }
                    break;
                }
                Some(false) => {
                    if !self.restarting {
                        let mut file = self.file_logger.lock().unwrap();
                        file.log(
                            LogLevel::Warn,
                            &format!("Service '{}' exited with failure", self.service.name),
                        );
                        self.restarting = true;
                    }

                    if shutdown_flag.load(Ordering::SeqCst) {
                        break;
                    }

                    match self.service.restart {
                        RestartPolicy::Always | RestartPolicy::OnFailure => {
                            thread::sleep(Duration::from_secs(self.service.restart_delay.unwrap_or(1)));
                            self.start()?;
                            self.state = ServiceState::Starting;
                            self.restarting = false;
                        }
                        RestartPolicy::Never => break,
                    }
                }
            }

            thread::sleep(Duration::from_millis(250));
        }

        if shutdown_flag.load(Ordering::SeqCst) {
            let _ = self.stop();
        }

        Ok(())
    }
}

