
use std::{collections::HashMap, fs, path::Path, sync::{Arc, Mutex}};

use anyhow::{anyhow, Result};
use common::utils::*;
use crate::service::{ServiceConfig, Service, LogMode};
use crate::service_runner::spawn_service;

const SERVICES_DIR: &str = "/etc/verdant/services";
const ENABLED_DIR: &str = "/etc/verdant/enabled";

#[derive(Debug, Default)]
pub struct Supervisor {
    /// Loaded service configurations
    services: HashMap<String, ServiceConfig>,
    /// Currently running services protected by mutex
    running: Arc<Mutex<HashMap<String, Service>>>,
}

impl Supervisor {
    pub fn load_all_services(&mut self) -> Result<()> {
        for entry in fs::read_dir(SERVICES_DIR)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    let config = Self::parse_service_file(&path, name)?;
                    let stem = Path::new(name)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or(name);
                    self.services.insert(stem.to_string(), config);
                }
            }
        }
        Ok(())
    }

    pub fn list_enabled_services(&self) -> Result<Vec<String>> {
        let mut enabled = Vec::new();
        for entry in fs::read_dir(ENABLED_DIR)? {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str() {
                enabled.push(name.to_string());
            }
        }
        Ok(enabled)
    }

    pub fn start_enabled_services(&mut self) -> Result<()> {
        for service_name in self.list_enabled_services()? {
            if let Err(e) = self.start_service(&service_name) {
                eprintln!("Warning: Failed to start enabled service '{}': {}", service_name, e);
            }
        }
        Ok(())
    }

    pub fn start_service(&mut self, name: &str) -> Result<()> {
        let config = self.services.get(name)
            .ok_or_else(|| anyhow!("Service '{}' not found", name))?;
        spawn_service(self, name, config)
    }

    pub fn stop_service(&mut self, name: &str) -> Result<()> {
        let mut running = self.running.lock().unwrap();
        if let Some(service) = running.get_mut(name) {
            service.stop()?; // <-- you'll need this method on Service to stop gracefully
            running.remove(name);
            print_info(&format!("Service '{}' stopped", name));
            Ok(())
        } else {
            Err(anyhow!("Service '{}' is not running", name))
        }
    }

    pub fn stop_all_services(&mut self) -> Result<()> {
        let mut running = self.running.lock().unwrap();
        // Collect names first to avoid borrowing issues
        let names: Vec<String> = running.keys().cloned().collect();

        for name in names {
            if let Some(service) = running.get_mut(&name) {
                if let Err(e) = service.stop() {
                    eprintln!("Warning: Failed to stop service '{}': {}", name, e);
                }
            }
            running.remove(&name);
            print_info(&format!("Service '{}' stopped", name));
        }
        Ok(())
    }

    fn parse_service_file(path: &Path, name: &str) -> Result<ServiceConfig> {
        let content = fs::read_to_string(path)?;

        let mut exec = None;
        let mut args = Vec::new();
        let mut env = HashMap::new();
        let mut restart = false;
        let mut log_mode = LogMode::File; // default to File

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("exec =") {
                exec = Some(rest.trim().to_string());
            } else if let Some(rest) = line.strip_prefix("args =") {
                args = rest.split_whitespace().map(String::from).collect();
            } else if let Some(rest) = line.strip_prefix("env =") {
                for pair in rest.split(',') {
                    if let Some((k, v)) = pair.split_once('=') {
                        env.insert(k.trim().to_string(), v.trim().to_string());
                    }
                }
            } else if let Some(rest) = line.strip_prefix("restart =") {
                restart = rest.trim().eq_ignore_ascii_case("yes");
            } else if let Some(rest) = line.strip_prefix("log =") {
                let mode = rest.trim().to_lowercase();
                log_mode = match mode.as_str() {
                    "file" => LogMode::File,
                    "null" => LogMode::Null,
                    _ => {
                        eprintln!("Warning: Unknown log mode '{}', defaulting to 'file'", mode);
                        LogMode::File
                    }
                };
            } else if let Some(rest) = line.strip_prefix("name =") {
                let file_stem = Path::new(name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(name);

                if rest.trim() != file_stem {
                    eprintln!(
                        "Warning: Service file name '{}' and name field '{}' mismatch",
                        name,
                        rest.trim()
                    );
                }
            }
        }

        let exec = exec.ok_or_else(|| anyhow!("Service '{}' missing 'exec' directive", name))?;

        Ok(ServiceConfig {
            name: name.to_string(),
            exec,
            args,
            env,
            restart,
            log: log_mode,
        })
    }

    pub fn running_lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Service>> {
        self.running.lock().unwrap()
    }
}

