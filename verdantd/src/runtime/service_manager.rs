use std::thread;
use std::io::{self, BufRead};
use std::time::Duration;
use std::sync::mpsc::Receiver;
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
            let pid = svc.launch()?;
            let msg = format!("Launched service {} (PID {})", svc.config.name, pid);
            if i == total - 1 {
                print_substep_last(&msg, &status_ok());
            } else {
                print_substep(&msg, &status_ok());
            }
        }

        // Start thread to listen for "shutdown" or "reboot" from stdin
        let (stdin_tx, stdin_rx) = std::sync::mpsc::channel::<SystemAction>();
        thread::spawn(move || {
            let stdin = io::stdin();
            for line in stdin.lock().lines() {
                match line {
                    Ok(cmd) if cmd.trim() == "shutdown" => {
                        let _ = stdin_tx.send(SystemAction::Shutdown);
                        break;
                    }
                    Ok(cmd) if cmd.trim() == "reboot" => {
                        let _ = stdin_tx.send(SystemAction::Reboot);
                        break;
                    }
                    _ => {
                        // Ignore unrecognized input
                    }
                }
            }
        });

        // Supervision loop
        loop {
            supervise_services(&mut self.services)?;

            // Handle IPC
            if let Ok(action) = rx.try_recv() {
                shutdown_or_reboot(&mut self.services, action)?;
                break;
            }

            // Handle stdin
            if let Ok(action) = stdin_rx.try_recv() {
                shutdown_or_reboot(&mut self.services, action)?;
                break;
            }

            thread::sleep(Duration::from_secs(1));
        }

        Ok(())
    }
}

