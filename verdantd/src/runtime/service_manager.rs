use std::io;
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::Duration;
use std::sync::{Arc, Mutex};
use ipc_protocol::Response;

use crate::service::ServiceConfig;
use crate::managed_service::ManagedService;
use crate::runtime::supervisor::supervise_services;
use crate::runtime::system_action::{shutdown_or_reboot, SystemAction};
use crate::runtime::dependency::resolve_services;
use crate::runtime::service_ops::{start_service, stop_service, enable_service, disable_service};

use common::{print_info_step, print_substep, print_substep_last, status_ok};

pub struct ServiceManager {
    pub services: Vec<ManagedService>,
}

impl ServiceManager {
    pub fn new(configs: Vec<ServiceConfig>) -> Self {
        let services = resolve_services(configs);
        Self { services }
    }

    pub fn start_service(&mut self, name: &str) -> Response {
        start_service(&mut self.services, name)
    }

    pub fn stop_service(&mut self, name: &str) -> Response {
        stop_service(&mut self.services, name)
    }

    pub fn enable_service(&mut self, name: &str) -> Response {
        enable_service(&mut self.services, name)
    }

    pub fn disable_service(&mut self, name: &str) -> Response {
        disable_service(&mut self.services, name)
    }


    pub fn run_with_ipc(svc_manager: Arc<Mutex<ServiceManager>>, rx: Receiver<SystemAction>) -> io::Result<()> {
        // initial launch
        {
            let mut sm = svc_manager.lock().unwrap();

            print_info_step("Launching enabled services in /etc/verdant/enabled ...");

            let total = sm.services.len();
            for (i, svc) in sm.services.iter_mut().enumerate() {
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
                    }
                }
            }
        }

        loop {
            {
                let mut sm = svc_manager.lock().unwrap();
                supervise_services(&mut sm.services)?;
            }

            if let Ok(action) = rx.try_recv() {
                let mut sm = svc_manager.lock().unwrap();
                shutdown_or_reboot(&mut sm.services, action)?;
                break;
            }

            thread::sleep(Duration::from_secs(1));
        }

        Ok(())
    }
}

