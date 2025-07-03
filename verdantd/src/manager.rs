use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;
use std::thread::{self, JoinHandle};

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;

use crate::service_file::ServiceFile;
use crate::supervisor::Supervisor;

pub struct ServiceManager {
    supervisors: HashMap<String, Supervisor>,
    handles: HashMap<String, JoinHandle<()>>,

    console_logger: Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
    file_logger: Arc<Mutex<dyn FileLogger + Send + Sync>>,
}

impl ServiceManager {
    pub fn new(
        console_logger: Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
        file_logger: Arc<Mutex<dyn FileLogger + Send + Sync>>,
    ) -> Self {
        Self {
            supervisors: HashMap::new(),
            handles: HashMap::new(),
            console_logger,
            file_logger,
        }
    }

    pub fn get_file_logger(&self) -> Arc<Mutex<dyn FileLogger + Send + Sync>> {
        Arc::clone(&self.file_logger)
    }

    pub fn get_console_logger(&self) -> Arc<Mutex<dyn ConsoleLogger + Send + Sync>> {
        Arc::clone(&self.console_logger)
    }

    pub fn add_service(&mut self, service: ServiceFile) -> Result<(), BloomError> {
        if self.supervisors.contains_key(&service.name) {
            return Err(BloomError::Custom(format!("Service '{}' already added", service.name)));
        }

        let supervisor = Supervisor::new(
            service,
            Arc::clone(&self.console_logger),
            Arc::clone(&self.file_logger),
        );

        self.supervisors.insert(supervisor.service.name.clone(), supervisor);

        Ok(())
    }

    pub fn start_startup_services(&mut self) -> Result<(), BloomError> {
        let mut started_count = 0;
        let mut to_spawn = Vec::new();
        let mut startup_packages = HashSet::new();

        for (name, supervisor) in self.supervisors.iter_mut() {
            if let Some(package_name) = supervisor.service.startup_package.clone() {
                supervisor.start()?;
                to_spawn.push(name.clone());
                startup_packages.insert(package_name);
                started_count += 1;
            }
        }

        {
            let mut con = self.console_logger.lock().unwrap();
            let mut file = self.file_logger.lock().unwrap();

            if started_count > 0 {
                for name in &to_spawn {
                    if let Some(pkg) = self.supervisors.get(name).and_then(|s| s.service.startup_package.clone()) {
                        let per_service_msg = format!("Started service '{}' with startup package '{}'", name, pkg);
                        con.message(LogLevel::Info, &per_service_msg, std::time::Duration::ZERO);
                        file.log(LogLevel::Info, &per_service_msg);
                    }
                }

                let mut packages_list: Vec<_> = startup_packages.into_iter().collect();
                packages_list.sort();
                let summary_msg = format!(
                    "Started {} service(s) marked with startup_package(s): {}",
                    started_count,
                    packages_list.join(", ")
                );
                file.log(LogLevel::Info, &summary_msg);
            } else {
                let msg = "No startup packages found";
                con.message(LogLevel::Warn, msg, std::time::Duration::ZERO);
                file.log(LogLevel::Warn, msg);
            }
        }

        Ok(())
    }

    pub fn start_service(&mut self, name: &str) -> Result<(), BloomError> {
        let supervisor = self.supervisors.get_mut(name)
            .ok_or_else(|| BloomError::Custom(format!("Service '{}' not found", name)))?;
        supervisor.start()?;
        Ok(())
    }

    pub fn stop_service(&mut self, name: &str) -> Result<(), BloomError> {
        let supervisor = self.supervisors.get_mut(name)
            .ok_or_else(|| BloomError::Custom(format!("Service '{}' not found", name)))?;
        supervisor.stop()
    }

    pub fn spawn_supervisor_thread(
        &mut self,
        name: String,
        shutdown_flag: Arc<AtomicBool>,
    ) -> Result<(), BloomError> {
        if self.handles.contains_key(&name) {
            return Ok(());
        }

        let supervisor = self.supervisors.get_mut(&name)
            .ok_or_else(|| BloomError::Custom(format!("Service '{}' not found", name)))?;

        let mut supervisor = std::mem::replace(supervisor, Supervisor::new(
            supervisor.service.clone(),
            Arc::clone(&self.console_logger),
            Arc::clone(&self.file_logger),
        ));

        let handle = thread::spawn(move || {
            if let Err(e) = supervisor.supervise_loop(shutdown_flag) {
                eprintln!("Supervisor for service '{}' exited with error: {:?}", supervisor.service.name, e);
            }
        });

        self.handles.insert(name, handle);
        Ok(())
    }

    pub fn supervise_all(&mut self, shutdown_flag: Arc<AtomicBool>) -> Result<(), BloomError> {
        for name in self.supervisors.keys().cloned().collect::<Vec<_>>() {
            self.spawn_supervisor_thread(name, Arc::clone(&shutdown_flag))?;
        }
        Ok(())
    }

    pub fn shutdown(&mut self) -> Result<(), BloomError> {
        // Shutdown services while holding the lock, collect results
        let shutdown_results = {
            if self.supervisors.is_empty() {
                None
            } else {
                let mut results = Vec::with_capacity(self.supervisors.len());
                for supervisor in self.supervisors.values_mut() {
                    results.push(supervisor.shutdown());
                }
                Some(results)
            }
        };

        // Join supervisor threads after dropping any locks
        let join_results = {
            if self.handles.is_empty() {
                None
            } else {
                let mut results = Vec::with_capacity(self.handles.len());
                for (name, handle) in self.handles.drain() {
                    let res = handle.join();
                    if let Err(e) = &res {
                        eprintln!("Supervisor thread for service '{}' panicked: {:?}", name, e);
                    }
                    results.push(res);
                }
                Some(results)
            }
        };

        // Clear supervisors after shutdown and joining
        self.supervisors.clear();

        // Logging after locks dropped
        let mut file = self.file_logger.lock().unwrap();

        if let Some(results) = shutdown_results {
            for res in results {
                if let Err(e) = res {
                    let msg = format!("Failed to shutdown a service: {:?}", e);
                    file.log(LogLevel::Fail, &msg);
                }
            }
        } else {
            file.log(LogLevel::Warn, "Shutdown: No services to stop");
        }

        if join_results.is_none() {
            file.log(LogLevel::Info, "Shutdown: No supervisor threads to join");
        }

        Ok(())
    }
}

