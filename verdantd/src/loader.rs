use std::fs;
use common::*;

use crate::service::ServiceConfig;

pub const ENABLED_DIR: &str = "/etc/verdant/enabled";

pub fn load_enabled_services() -> Vec<ServiceConfig> {
    let mut configs = Vec::new();

    match fs::read_dir(ENABLED_DIR) {
        Ok(entries) => {
            print_step(&format!("Loaded service files from '{}'", ENABLED_DIR), &status_ok());

            for entry in entries.flatten() {
                let path = entry.path();
                match ServiceConfig::from_file(&path) {
                    Ok(cfg) => configs.push(cfg),
                    Err(e) => print_step(&format!("Failed to load {:?}: {}", path, e), &status_warn()),
                }
            }
        }
        Err(e) => {
            print_step(&format!("Could not read enabled dir '{}': {}", ENABLED_DIR, e), &status_fail());
        }
    }

    configs
}

