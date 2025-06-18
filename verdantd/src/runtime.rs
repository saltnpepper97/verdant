use std::process::{Child, Command};
use std::io;
use std::thread;
use std::time::Duration;

use common::{print_step, status_fail, status_ok};

use crate::service::{ServiceConfig, RestartPolicy};

pub struct ManagedService {
    config: ServiceConfig,
    child: Option<Child>,
}

impl ManagedService {
    pub fn new(config: ServiceConfig) -> Self {
        Self { config, child: None }
    }

    pub fn launch(&mut self) -> io::Result<()> {
        let mut cmd = Command::new(&self.config.exec);
        if let Some(args) = &self.config.args {
            cmd.args(args);
        }
        let mut child = cmd.spawn()?;
        let pid = child.id();
        print_step(
            &format!("Launched service {} (PID {})", self.config.name, pid),
            &status_ok(),
        );
        self.child = Some(child);
        Ok(())
    }

    pub fn supervise(&mut self) -> io::Result<()> {
        if let Some(child) = &mut self.child {
            match child.try_wait()? {
                Some(status) => {
                    print_step(
                        &format!("Service {} exited with {:?}", self.config.name, status),
                        &status_fail(),
                    );

                    match self.config.restart {
                        RestartPolicy::Always => {
                            print_step(
                                &format!("Restarting service {} (policy: always)", self.config.name),
                                &status_ok(),
                            );
                            self.launch()?;
                        }
                        RestartPolicy::OnFailure if !status.success() => {
                            print_step(
                                &format!("Restarting service {} (policy: on-failure)", self.config.name),
                                &status_ok(),
                            );
                            self.launch()?;
                        }
                        RestartPolicy::Never | RestartPolicy::OnFailure => {
                            self.child = None;
                        }
                    }
                }
                None => {
                }
            }
        } else {
            // Service not started yet; launch it
            self.launch()?;
        }
        Ok(())
    }
}

pub struct ServiceManager {
    services: Vec<ManagedService>,
}

impl ServiceManager {
    pub fn new(configs: Vec<ServiceConfig>) -> Self {
        let services = configs.into_iter().map(ManagedService::new).collect();
        Self { services }
    }

    pub fn run(&mut self) -> io::Result<()> {
        // Launch all services first
        for svc in self.services.iter_mut() {
            svc.launch()?;
        }

        loop {
            for svc in self.services.iter_mut() {
                svc.supervise()?;
            }
            thread::sleep(Duration::from_secs(1));
        }
    }
}

