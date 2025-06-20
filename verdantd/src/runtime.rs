use std::process::Command;
use std::io;
use std::thread;
use std::time::{Duration, Instant};
use std::sync::mpsc::Receiver;
use std::collections::{HashMap, HashSet};


use common::{print_step, print_substep, print_substep_last, status_warn, status_fail, status_ok};
use crate::service::{ServiceConfig};
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
                        continue;
                    }

                    let start = Instant::now();
                    loop {
                        match child.try_wait()? {
                            Some(status) => {
                                print_substep(&format!("Service {} exited with {:?}", svc.config.name, status), &status_ok());
                                break;
                            }
                            None => {
                                if start.elapsed() > Duration::from_secs(5) {
                                    eprintln!("Service {} did not exit after SIGTERM timeout, sending SIGKILL...", svc.config.name);
                                    if let Err(e) = kill(pid, Signal::SIGKILL) {
                                        eprintln!("Failed to kill {}: {}", svc.config.name, e);
                                    }
                                    break;
                                }
                                thread::sleep(Duration::from_millis(200));
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

        print_step("Syncing disks before reboot/shutdown...", &status_ok());
        let _ = Command::new("sync").status();
        thread::sleep(Duration::from_secs(1));

        print_step("Attempting to perform system action...", &status_ok());

        // Attempt to use libc reboot syscall
        #[cfg(target_os = "linux")]
        {
            use libc::{reboot, sync, RB_AUTOBOOT, RB_POWER_OFF};
            unsafe { sync(); }

            let result = unsafe {
                match action {
                    SystemAction::Reboot => reboot(RB_AUTOBOOT),
                    SystemAction::Shutdown => reboot(RB_POWER_OFF),
                }
            };

            if result == 0 {
                return Ok(());
            } else {
                eprintln!("libc reboot syscall failed (not running as PID 1 or missing CAP_SYS_BOOT?)");
            }
        }

        // Try writing to /proc/sysrq-trigger
        let sysrq_result = std::fs::write(
            "/proc/sysrq-trigger",
            match action {
                SystemAction::Reboot => "b",
                SystemAction::Shutdown => "o",
            },
        );

        if sysrq_result.is_ok() {
            return Ok(());
        } else {
            eprintln!("/proc/sysrq-trigger write failed: {:?}", sysrq_result);
        }

        // Fallback to executing system command
        let cmd = match action {
            SystemAction::Reboot => "reboot",
            SystemAction::Shutdown => "poweroff",
        };

        print_step(&format!("Fallback: Executing system command: {}", cmd), &status_warn());

        let status = Command::new(cmd)
            .status()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to execute {}: {}", cmd, e)))?;

        if !status.success() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("{} command failed with status: {:?}", cmd, status.code()),
            ));
        }

        Ok(())
    }
}

