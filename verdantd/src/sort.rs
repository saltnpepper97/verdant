use std::collections::{HashMap, HashSet, VecDeque};

pub fn topological_sort(graph: &HashMap<String, HashSet<String>>) -> Result<Vec<String>, Vec<String>> {
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
