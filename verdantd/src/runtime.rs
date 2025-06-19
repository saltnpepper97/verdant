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

    pub fn launch(&mut self) -> io::Result<u32> {
        let mut cmd = Command::new(&self.config.exec);
        if let Some(args) = &self.config.args {
            cmd.args(args);
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
                None => {}
            }
        } else {
            if matches!(self.config.restart, RestartPolicy::Always) {
                self.launch()?;
            }
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

        // Validate `requires` dependencies, but just print errors instead of panic
        for (name, config) in &name_to_config {
            for req in &config.requires {
                if !name_to_config.contains_key(req) {
                    print_step(
                        &format!("Error: Service '{}' requires missing service '{}'", name, req),
                        &status_fail(),
                    );
                    // Don't panic, just continue
                }
            }
        }

        // Build dependency graph for topological sort
        // Edges point from dependency -> dependent service
        let mut graph: HashMap<String, HashSet<String>> = HashMap::new();

        // Initialize graph nodes with empty sets
        for name in name_to_config.keys() {
            graph.insert(name.clone(), HashSet::new());
        }

        // For each service, for each dependency, add edge from dependency to service,
        // but only if the dependency exists
        for (name, config) in &name_to_config {
            for dep in config.requires.iter().chain(config.after.iter()) {
                if name_to_config.contains_key(dep) {
                    graph.get_mut(dep).unwrap().insert(name.clone());
                } else {
                    print_step(
                        &format!("Warning: Service '{}' has dependency '{}' which does not exist", name, dep),
                        &status_fail(),
                    );
                }
            }
        }

        // Sort services topologically
        let sorted = match topological_sort(&graph) {
            Ok(sorted) => sorted,
            Err(cycle) => {
                print_step(
                    &format!("Cycle detected in service dependencies: {:?}", cycle),
                    &status_fail(),
                );
                // If cycle detected, fallback to original order ignoring dependencies
                name_to_config.keys().cloned().collect()
            }
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

        loop {
            for svc in self.services.iter_mut() {
                svc.supervise()?;
            }
            thread::sleep(Duration::from_secs(1));
        }
    }
}

// Topological sort using Kahn's algorithm
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
