use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::sleep;
use std::time::Duration;

use bloom::status::ServiceState;
use bloom::errors::BloomError;

use crate::service::Service;
use crate::control::{ServiceHandle, start_service, stop_service, restart_service};

pub struct Supervisor {
    pub service: Service,
    pub handle: Option<ServiceHandle>,
    pub should_run: bool, // NEW: track if this service should continue running
}

impl Supervisor {
    pub fn new(service: Service) -> Self {
        Self {
            service,
            handle: None,
            should_run: true,
        }
    }

    /// Start the service if not already running.
    pub fn start(&mut self) -> Result<(), BloomError> {
        if self.handle.is_some() || !self.should_run {
            // Already running or not allowed to run again
            return Ok(());
        }

        self.service.state = ServiceState::Starting;

        let handle = start_service(&self.service)?;
        self.handle = Some(handle);
        self.service.state = ServiceState::Running;

        Ok(())
    }

    /// Stop the service if running.
    pub fn stop(&mut self) -> Result<(), BloomError> {
        if let Some(mut handle) = self.handle.take() {
            self.service.state = ServiceState::Stopping;

            // Timeout 5 seconds to stop cleanly
            let stopped_cleanly = stop_service(&mut handle, Duration::from_secs(5))?;

            self.service.state = if stopped_cleanly {
                ServiceState::Stopped
            } else {
                ServiceState::Failed
            };

            self.should_run = false; // Once stopped manually, don't restart

            Ok(())
        } else {
            // Not running
            Ok(())
        }
    }

    /// Restart the service according to restart policy.
    pub fn restart(&mut self) -> Result<(), BloomError> {
        let current_handle = self.handle.take();
        let new_handle_opt = restart_service(&self.service, current_handle)?;

        self.handle = new_handle_opt;

        self.service.state = if self.handle.is_some() {
            ServiceState::Running
        } else {
            // Service was not restarted (e.g. restart: never or clean exit)
            self.should_run = false;
            ServiceState::Stopped
        };

        Ok(())
    }

    /// Main supervise loop.
    /// Checks the service status periodically and restarts if necessary.
    /// Will exit cleanly when `running` is set to false.
    pub fn supervise_loop(&mut self, running: Arc<AtomicBool>) -> Result<(), BloomError> {
        while running.load(Ordering::Relaxed) {
            if let Some(handle) = &mut self.handle {
                if !handle.is_running() {
                    // Process exited
                    self.service.state = ServiceState::Failed;

                    // Try to restart based on policy
                    self.restart()?;
                }
            } else if self.should_run {
                // Only auto-start if restart policy allows it
                self.start()?;
            }

            sleep(Duration::from_secs(2));
        }

        Ok(())
    }
}

