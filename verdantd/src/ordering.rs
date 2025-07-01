use crate::service_file::ServiceFile;
use bloom::errors::BloomError;
use std::collections::{HashMap, HashSet, VecDeque};

/// Resolves service startup order based on dependencies and priority.
/// Returns services sorted in execution order.
pub fn order_services(services: Vec<ServiceFile>) -> Result<Vec<ServiceFile>, BloomError> {
    let mut name_map: HashMap<String, ServiceFile> = HashMap::new();
    for svc in services {
        name_map.insert(svc.name.clone(), svc);
    }

    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    let mut in_degree: HashMap<String, usize> = HashMap::new();

    // Initialize graph nodes
    for name in name_map.keys() {
        graph.insert(name.clone(), Vec::new());
        in_degree.insert(name.clone(), 0);
    }

    // Add edges from dependencies
    for (name, svc) in &name_map {
        if let Some(deps) = &svc.dependencies {
            for dep in deps {
                if name_map.contains_key(dep) {
                    graph.get_mut(dep).unwrap().push(name.clone());
                    *in_degree.get_mut(name).unwrap() += 1;
                } else {
                    return Err(BloomError::Custom(format!(
                        "Unknown dependency '{}' for service '{}'",
                        dep, name
                    )));
                }
            }
        }
    }

    // Kahn's algorithm for topological sorting
    let mut queue: VecDeque<String> = in_degree
        .iter()
        .filter(|&(_, &deg)| deg == 0)
        .map(|(name, _)| name.clone())
        .collect();

    // Use a min-heap-style buffer for priority sort
    let mut ordered = Vec::new();
    let mut visited = HashSet::new();

    while let Some(name) = queue.pop_front() {
        visited.insert(name.clone());

        ordered.push(name.clone());

        for neighbor in &graph[&name] {
            let deg = in_degree.get_mut(neighbor).unwrap();
            *deg -= 1;
            if *deg == 0 && !visited.contains(neighbor) {
                queue.push_back(neighbor.clone());
            }
        }

        // Sort queue by priority
        queue.make_contiguous().sort_by_key(|n| {
            name_map[n].priority.unwrap_or(50)
        });
    }

    if ordered.len() != name_map.len() {
        return Err(BloomError::Custom("Cycle detected in service dependencies".into()));
    }

    Ok(ordered
        .into_iter()
        .map(|name| name_map.remove(&name).unwrap())
        .collect())
}
