use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::{LogLevel, ServiceState};

use crate::supervisor::Supervisor;

pub struct ShutdownManager {
    pub supervisors: Vec<Arc<Mutex<Supervisor>>>,
    pub is_shutting_down: AtomicBool,

    console_logger: Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
    file_logger: Arc<Mutex<dyn FileLogger + Send + Sync>>,
}

impl ShutdownManager {
    pub fn new(
        supervisors: Vec<Arc<Mutex<Supervisor>>>,
        console_logger: Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
        file_logger: Arc<Mutex<dyn FileLogger + Send + Sync>>,
    ) -> Self {
        Self {
            supervisors,
            is_shutting_down: AtomicBool::new(false),
            console_logger,
            file_logger,
        }
    }

    /// Shuts down all services, with per-service timeout and force kill if needed.
    pub fn shutdown_all(&self, timeout_per_service: Duration) -> Result<(), BloomError> {
        self.is_shutting_down.store(true, Ordering::SeqCst);

        {
            let mut con = self.console_logger.lock().unwrap();
            let mut file = self.file_logger.lock().unwrap();

            con.message(LogLevel::Info, &format!("Beginning shutdown of {} services", self.supervisors.len()), Duration::ZERO);
            file.log(LogLevel::Info, &format!("Beginning shutdown of {} services", self.supervisors.len()));
        }

        for supervisor_arc in &self.supervisors {
            // Lock supervisor once, do everything while it's held
            let mut supervisor = supervisor_arc.lock().unwrap();
            let service_name = supervisor.service.name.clone(); // clone here to avoid borrow issues

            {
                let mut con = self.console_logger.lock().unwrap();
                let mut file = self.file_logger.lock().unwrap();
                con.message(LogLevel::Info, &format!("Stopping service '{}'", service_name), Duration::ZERO);
                file.log(LogLevel::Info, &format!("Stopping service '{}'", service_name));
            }

            if let Err(e) = supervisor.stop() {
                let msg = format!("Failed to stop service '{}': {:?}", service_name, e);
                let mut con = self.console_logger.lock().unwrap();
                let mut file = self.file_logger.lock().unwrap();
                con.message(LogLevel::Warn, &msg, Duration::ZERO);
                file.log(LogLevel::Warn, &msg);
                continue;
            }

            supervisor.wait_for_exit_with_timeout(timeout_per_service);

            if supervisor.state != ServiceState::Stopped {
                let msg = format!("Service '{}' did not stop in time, sending SIGKILL", service_name);
                let mut con = self.console_logger.lock().unwrap();
                let mut file = self.file_logger.lock().unwrap();
                con.message(LogLevel::Warn, &msg, Duration::ZERO);
                file.log(LogLevel::Warn, &msg);

                if let Some(child) = &supervisor.child {
                    #[cfg(unix)]
                    {
                        use nix::sys::signal::{kill, Signal};
                        use nix::unistd::Pid;
                        let _ = kill(Pid::from_raw(child.id() as i32), Signal::SIGKILL);
                    }
                }
            }
        }

        {
            let mut con = self.console_logger.lock().unwrap();
            let mut file = self.file_logger.lock().unwrap();
            con.message(LogLevel::Info, "Shutdown complete", Duration::ZERO);
            file.log(LogLevel::Info, "Shutdown complete");
        }

        Ok(())
    }

    pub fn is_shutting_down(&self) -> bool {
        self.is_shutting_down.load(Ordering::SeqCst)
    }
}

