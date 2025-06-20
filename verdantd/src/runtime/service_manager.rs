use std::io;
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::Duration;

use crate::service::ServiceConfig;
use crate::managed_service::ManagedService;
use crate::runtime::supervisor::supervise_services;
use crate::runtime::system_action::{shutdown_or_reboot, SystemAction};
use crate::runtime::dependency::resolve_services;

use common::{print_step, print_substep, print_substep_last, status_ok};

pub struct ServiceManager {
    pub services: Vec<ManagedService>,
}

impl ServiceManager {
    pub fn new(configs: Vec<ServiceConfig>) -> Self {
        let services = resolve_services(configs);
        Self { services }
    }

    pub fn run_with_ipc(&mut self, rx: Receiver<SystemAction>) -> io::Result<()> {
        print_step("Launching services...", &status_ok());

        let total = self.services.len();
        for (i, svc) in self.services.iter_mut().enumerate() {
            match svc.launch() {
                Ok(pid) => {
                    let msg = format!("Launched service {} (PID {})", svc.config.name, pid);
                    if i == total - 1 {
                        print_substep_last(&msg, &status_ok());
                    } else {
                        print_substep(&msg, &status_ok());
                    }
                }
                Err(e) => {
                    let msg = format!("Failed to launch service {}: {}", svc.config.name, e);
                    if i == total - 1 {
                        print_substep_last(&msg, &common::status_fail());
                    } else {
                        print_substep(&msg, &common::status_fail());
                    }
                    // Optionally log or collect failures here, but DO NOT return Err(e)
                }
            }
        }

        loop {
            supervise_services(&mut self.services)?;

            if let Ok(action) = rx.try_recv() {
                shutdown_or_reboot(&mut self.services, action)?;
                break;
            }

            thread::sleep(Duration::from_secs(1));
        }

        Ok(())
    }
}
