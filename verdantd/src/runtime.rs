use std::process::{Child, Command};
use std::io;
use std::thread;
use std::time::Duration;
use std::collections::{HashMap, HashSet, VecDeque};

use common::{print_step, print_substep, print_substep_last, status_fail, status_ok};

use crate::service::{ServiceConfig, RestartPolicy};

pub struct ManagedService {
    config: ServiceConfig,
    child: Option<Child>,
}

impl ManagedService {
    pub fn new(config: ServiceConfig) -> Self {
        Self { config, child: None }
    }

    // Launch returns PID but does not print
    pub fn launch(&mut self) -> io::Result<u32> {
        let mut cmd = Command::new(&self.config.exec);
        if let Some(args) = &self.config.args {
            cmd.args(args);
        }
        let mut child = cmd.spawn()?;
        let pid = child.id();
        self.child = Some(child);
        Ok(pid)
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
                            self.launch()?; // You might want to handle printing here too, but up to you
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
        let mut name_to_config: HashMap<String, ServiceConfig> = HashMap::new();
        for config in configs {
            name_to_config.insert(config.name.clone(), config);
        }

        // Build dependency graph
        let mut graph: HashMap<String, HashSet<String>> = HashMap::new();

        for (name, config) in &name_to_config {
            let mut deps = HashSet::new();

            for req in &config.requires {
                deps.insert(req.clone());
            }
            for after in &config.after {
                deps.insert(after.clone());
            }

            graph.insert(name.clone(), deps);
        }

        // Topological sort
        let sorted = match topological_sort(&graph) {
            Ok(sorted) => sorted,
            Err(cycle) => panic!("Cycle detected in service dependencies: {:?}", cycle),
        };

        let services = sorted
            .into_iter()
            .filter_map(|name| name_to_config.remove(&name))
            .map(ManagedService::new)
            .collect();

        Self { services }
    }

    pub fn run(&mut self) -> io::Result<()> {
        let last_index = self.services.len().saturating_sub(1);

        // Launch all services first with proper printing
        for (i, svc) in self.services.iter_mut().enumerate() {
            let pid = svc.launch()?;
            if i == last_index {
                print_substep_last(
                    &format!("Launched service {} (PID {})", svc.config.name, pid),
                    &status_ok(),
                );
            } else {
                print_substep(
                    &format!("Launched service {} (PID {})", svc.config.name, pid),
                    &status_ok(),
                );
            }
        }

        // Supervise services in a loop
        loop {
            for svc in self.services.iter_mut() {
                svc.supervise()?;
            }
            thread::sleep(Duration::from_secs(1));
        }
    }
}

fn topological_sort(graph: &HashMap<String, HashSet<String>>) -> Result<Vec<String>, Vec<String>> {
    let mut in_degree = HashMap::new();
    let mut result = Vec::new();

    for node in graph.keys() {
        in_degree.insert(node.clone(), 0);
    }

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

    while let Some(node) = queue.pop_front() {
        result.push(node.clone());
        if let Some(dependents) = graph.get(&node) {
            for dep in dependents {
                if let Some(count) = in_degree.get_mut(dep) {
                    *count -= 1;
                    if *count == 0 {
                        queue.push_back(dep.clone());
                    }
                }
            }
        }
    }

    if result.len() == graph.len() {
        Ok(result)
    } else {
        // cycle detected
        let cycle_nodes: Vec<String> = in_degree
            .into_iter()
            .filter(|(_, v)| *v > 0)
            .map(|(k, _)| k)
            .collect();
        Err(cycle_nodes)
    }
}

