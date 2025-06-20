use std::process::{Child, Command};
use std::io;
use std::thread;
use std::time::Duration;
use std::sync::mpsc::Receiver;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::process::Stdio;

use common::{print_step, print_substep, print_substep_last, status_warn, status_fail, status_ok};
use crate::service::{ServiceConfig, RestartPolicy};

pub struct ManagedService {
    pub config: ServiceConfig,
    pub child: Option<Child>,
}

pub enum SystemAction {
    Reboot,
    Shutdown,
}

impl ManagedService {
    pub fn new(config: ServiceConfig) -> Self {
        Self { config, child: None }
    }


pub fn launch(&mut self) -> io::Result<u32> {
    let mut cmd = Command::new(&self.config.exec);
    if let Some(args) = &self.config.args {
        cmd.args(args);
    }

    if let Some(tty_path) = &self.config.tty {
        use std::os::unix::io::AsRawFd;
        use std::os::unix::process::CommandExt;
        use std::fs::OpenOptions;
        use libc::{self, setsid, ioctl, TIOCSCTTY};

        let tty = OpenOptions::new()
            .read(true)
            .write(true)
            .open(tty_path)?;

        let tty_fd = tty.as_raw_fd();

        // Safety: we're in a pre-exec closure so it's okay to call these
        unsafe {
            cmd.stdin(Stdio::from(tty.try_clone()?));
            cmd.stdout(Stdio::from(tty.try_clone()?));
            cmd.stderr(Stdio::from(tty));

            cmd.pre_exec(move || {
                if setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }

                if ioctl(tty_fd, TIOCSCTTY, 1) == -1 {
                    return Err(io::Error::last_os_error());
                }

                Ok(())
            });
        }
    } else {
        // Default: redirect output to /dev/null
        let devnull = File::open("/dev/null")?;
        cmd.stdout(Stdio::from(devnull.try_clone()?));
        cmd.stderr(Stdio::from(devnull));
    }

    let child = cmd.spawn()?;
    let pid = child.id();
    self.child = Some(child);
    Ok(pid)
}


    pub fn supervise(&mut self) -> io::Result<()> {
        if let Some(child) = &mut self.child {
            match child.try_wait()? {
                Some(status) => {
                    if !status.success() {
                        print_step(
                            &format!("Service {} exited with {:?}", self.config.name, status),
                            &status_fail(),
                        );
                    }
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
                None => {}
            }
        } else if matches!(self.config.restart, RestartPolicy::Always) {
            self.launch()?;
        }
        Ok(())
    }
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

        let cmd = match action {
            SystemAction::Reboot => "reboot",
            SystemAction::Shutdown => "poweroff",
        };

        print_step(&format!("Executing system command: {}", cmd), &status_ok());

        Command::new(cmd)
            .spawn()
            .map(|_| ())
            .or_else(|e| Err(io::Error::new(io::ErrorKind::Other, format!("Failed to execute {}: {}", cmd, e))))
    }
}

fn topological_sort(graph: &HashMap<String, HashSet<String>>) -> Result<Vec<String>, Vec<String>> {
    let mut in_degree: HashMap<String, usize> = graph
        .keys()
        .map(|k| (k.clone(), 0))
        .collect();

    for deps in graph.values() {
        for dep in deps {
            if let Some(count) = in_degree.get_mut(dep) {
                *count += 1;
            }
        }
    }

    let mut queue: VecDeque<String> = in_degree
        .iter()
        .filter_map(|(k, &v)| if v == 0 { Some(k.clone()) } else { None })
        .collect();

    let mut result = Vec::new();

    while let Some(node) = queue.pop_front() {
        result.push(node.clone());

        for neighbor in graph.get(&node).unwrap_or(&HashSet::new()) {
            if let Some(count) = in_degree.get_mut(neighbor) {
                *count -= 1;
                if *count == 0 {
                    queue.push_back(neighbor.clone());
                }
            }
        }
    }

    if result.len() == graph.len() {
        Ok(result)
    } else {
        let cycle_nodes = in_degree
            .into_iter()
            .filter(|(_, v)| *v > 0)
            .map(|(k, _)| k)
            .collect();
        Err(cycle_nodes)
    }
}

