use std::collections::{HashMap, HashSet};

use crate::service::ServiceConfig;
use crate::managed_service::ManagedService;
use crate::sort::topological_sort;
use common::{print_step, status_warn, status_fail};

pub fn resolve_services(configs: Vec<ServiceConfig>) -> Vec<ManagedService> {
    let mut name_to_config: HashMap<String, ServiceConfig> = HashMap::new();
    for config in configs {
        name_to_config.insert(config.name.clone(), config);
    }

    let mut services_with_missing_deps = HashSet::new();

    for (name, config) in &name_to_config {
        for req in config.requires.iter().chain(config.after.iter()) {
            if !name_to_config.contains_key(req) {
                print_step(
                    &format!("Service '{}' did not start. Missing dependency: '{}'", name, req),
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
                if let Some(deps) = graph.get_mut(dep) {
                    deps.insert(name.clone());
                }
            }
        }
    }

    let sorted = match topological_sort(&graph) {
        Ok(s) => s,
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

    sorted
        .into_iter()
        .filter_map(|name| name_to_config.remove(&name))
        .map(ManagedService::new)
        .collect()
}
