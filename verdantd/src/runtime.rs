use std::process::Command;
use std::io;
use std::thread;
use std::time::Duration;
use std::sync::mpsc::Receiver;
use std::collections::{HashMap, HashSet};

use common::{print_step, print_substep, print_substep_last, status_warn, status_fail, status_ok};
use crate::service::ServiceConfig;
use crate::managed_service::ManagedService;
use crate::sort::topological_sort;

pub enum SystemAction {
    Reboot,
    Shutdown,
}

pub struct ServiceManager {
    pub services: Vec<ManagedService>,
}

impl ServiceManager {
    pub fn new(configs: Vec<ServiceConfig>) -> Self {
        let mut name_to_config: HashMap<String, ServiceConfig> = HashMap::new();
        for config in configs {
            name_to_config.insert(config.name.clone(), config);
        }

        let mut services_with_missing_deps = HashSet::new();
        for (name, config) in &name_to_config {
            for req in &config.requires {
                if !name_to_config.contains_key(req) {
                    services_with_missing_deps.insert(name.clone());
                }
            }
        }

        for (name, config) in &name_to_config {
            for dep in config.requires.iter().chain(config.after.iter()) {
                if !name_to_config.contains_key(dep) {
                    print_step(
                        &format!("Service '{}' did not start. Missing dependency: '{}'", name, dep),
                        &status_warn(),
                    );
                    services_with_missing_deps.insert(name.clone());
                }
            }
        }

        let mut graph: HashMap<String, HashSet<String>> = HashMap::new();
        for name in name_to_config.keys() {
            if !services_with_missing_deps.contains(name) {
                graph.insert(name.clone(), HashSet::new());
            }
        }

        for (name, config) in &name_to_config {
            if services_with_missing_deps.contains(name) {
                continue;
            }
            for dep in config.requires.iter().chain(config.after.iter()) {
                if !services_with_missing_deps.contains(dep) {
                    if graph.contains_key(dep) {
                        graph.get_mut(dep).unwrap().insert(name.clone());
                    }
                }
            }
        }

        let sorted = match topological_sort(&graph) {
            Ok(sorted) => sorted,
            Err(cycle) => {
                print_step(
                    &format!("Cycle detected in service dependencies: {:?}", cycle),
                    &status_fail(),
                );
                graph.keys().cloned().collect()
            }
        };

        for missing in &services_with_missing_deps {
            name_to_config.remove(missing);
        }

        let services = sorted
            .into_iter()
            .filter_map(|name| name_to_config.remove(&name))
            .map(ManagedService::new)
            .collect();

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

        loop {
            for svc in self.services.iter_mut() {
                svc.supervise()?;
            }

            if let Ok(action) = rx.try_recv() {
                match action {
                    SystemAction::Reboot => {
                        print_step("Received reboot command via IPC", &status_ok());
                        self.shutdown_or_reboot(SystemAction::Reboot)?;
                        break;
                    }
                    SystemAction::Shutdown => {
                        print_step("Received shutdown command via IPC", &status_ok());
                        self.shutdown_or_reboot(SystemAction::Shutdown)?;
                        break;
                    }
                }
            }

            thread::sleep(Duration::from_secs(1));
        }

        Ok(())
    }

    pub fn shutdown_or_reboot(&mut self, action: SystemAction) -> io::Result<()> {
        print_step("Stopping all services...", &status_ok());

        for svc in self.services.iter_mut().rev() {
            if let Some(child) = &mut svc.child {
                print_substep(&format!("Stopping service {}", svc.config.name), &status_ok());

                #[cfg(unix)]
                {
                    use nix::sys::signal::{kill, Signal};
                    use nix::unistd::Pid;

                    let pid = Pid::from_raw(child.id() as i32);
                    if let Err(e) = kill(pid, Signal::SIGTERM) {
                        eprintln!("Failed to send SIGTERM to {}: {}", svc.config.name, e);
                    } else {
                        thread::sleep(Duration::from_secs(1));
                    }

                    match child.try_wait()? {
                        Some(_) => {}
                        None => {
                            eprintln!("Service {} did not exit after SIGTERM, killing...", svc.config.name);
                            if let Err(e) = kill(pid, Signal::SIGKILL) {
                                eprintln!("Failed to kill {}: {}", svc.config.name, e);
                            }
                        }
                    }
                }

                #[cfg(not(unix))]
                {
                    child.kill()?;
                }

                svc.child = None;
            }
        }

        print_step("All services stopped.", &status_ok());

        print_step("Syncing disks before shutdown/reboot...", &status_ok());
        let _ = Command::new("sync").status();

        let candidates = match action {
            SystemAction::Reboot => vec!["/sbin/reboot", "/bin/reboot", "reboot", "systemctl reboot", "shutdown -r now", "halt -f"],
            SystemAction::Shutdown => vec!["/sbin/poweroff", "/bin/poweroff", "poweroff", "systemctl poweroff", "shutdown -h now", "halt -f"],
        };

        for cmd_str in candidates {
            let parts: Vec<&str> = cmd_str.split_whitespace().collect();
            let (cmd, args) = parts.split_first().unwrap();

            print_step(&format!("Trying: {} {:?}", cmd, args), &status_ok());
            let result = Command::new(cmd).args(args).spawn();

            if let Ok(_child) = result {
                return Command::new(cmd).args(args).spawn().map(|_| ());
            } else {
                eprintln!("Failed to execute command: {}", cmd_str);
            }
        }

        Err(io::Error::new(io::ErrorKind::Other, "All shutdown/reboot attempts failed"))
    }
}


