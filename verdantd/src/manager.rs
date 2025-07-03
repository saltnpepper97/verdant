use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::sync::atomic::AtomicBool;
use std::thread::{self, JoinHandle};

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;

use crate::service_file::ServiceFile;
use crate::supervisor::Supervisor;

/// Manages all supervisors and their threads
pub struct ServiceManager {
    supervisors: HashMap<String, Arc<Mutex<Supervisor>>>,
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

    /// Add service by creating a Supervisor
    pub fn add_service(&mut self, service: ServiceFile) -> Result<(), BloomError> {
        if self.supervisors.contains_key(&service.name) {
            return Err(BloomError::Custom(format!("Service '{}' already added", service.name)));
        }

        let supervisor = Supervisor::new(
            service,
            Arc::clone(&self.console_logger),
            Arc::clone(&self.file_logger),
        );

        self.supervisors.insert(supervisor.service.name.clone(), Arc::new(Mutex::new(supervisor)));

        Ok(())
    }

    /// Start all services that have a startup_package defined
    pub fn start_startup_services(&mut self) -> Result<(), BloomError> {
        let mut started_count = 0;
        let mut startup_packages = HashSet::new();

        // Silence unused variable warning by prefixing with underscore
        for (_name, supervisor_mutex) in &self.supervisors {
            let mut supervisor = supervisor_mutex.lock().unwrap();
            if supervisor.service.startup_package.is_some() {
                supervisor.start()?;
                started_count += 1;
                if let Some(pkg) = &supervisor.service.startup_package {
                    startup_packages.insert(pkg.clone());
                }
            }
        }

        {
            let mut con = self.console_logger.lock().unwrap();
            let mut file = self.file_logger.lock().unwrap();

            if started_count > 0 {
                for (name, supervisor_mutex) in &self.supervisors {
                    let supervisor = supervisor_mutex.lock().unwrap();
                    if let Some(pkg) = &supervisor.service.startup_package {
                        let msg = format!("Started service '{}' with startup package '{}'", name, pkg);
                        con.message(LogLevel::Info, &msg, std::time::Duration::ZERO);
                        file.log(LogLevel::Info, &msg);
                    }
                }

                let mut packages: Vec<_> = startup_packages.into_iter().collect();
                packages.sort();
                let summary_msg = format!(
                    "Started {} service(s) with startup package(s): {}",
                    started_count,
                    packages.join(", ")
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

    /// Start a single service by name
    pub fn start_service(&self, name: &str) -> Result<(), BloomError> {
        let supervisor_mutex = self.supervisors.get(name)
            .ok_or_else(|| BloomError::Custom(format!("Service '{}' not found", name)))?;

        let mut supervisor = supervisor_mutex.lock().unwrap();
        supervisor.start()
    }

    /// Stop a single service by name
    pub fn stop_service(&self, name: &str) -> Result<(), BloomError> {
        let supervisor_mutex = self.supervisors.get(name)
            .ok_or_else(|| BloomError::Custom(format!("Service '{}' not found", name)))?;

        let mut supervisor = supervisor_mutex.lock().unwrap();
        supervisor.stop()
    }

    /// Spawn supervisor threads for all services and store JoinHandles
    pub fn supervise_all(&mut self, shutdown_flag: Arc<AtomicBool>) -> Result<(), BloomError> {
        for (name, supervisor_mutex) in &self.supervisors {
            if self.handles.contains_key(name) {
                continue; // Already spawned
            }

            let supervisor_mutex = Arc::clone(supervisor_mutex);
            let shutdown_flag = Arc::clone(&shutdown_flag);

            // Clone twice: once for closure move, once for insert
            let name_clone_for_thread = name.clone();
            let name_clone_for_insert = name.clone();

            let handle = thread::Builder::new()
                .name(format!("supervisor-{}", name_clone_for_thread))
                .spawn(move || {
                    let mut supervisor = supervisor_mutex.lock().unwrap();
                    if let Err(e) = supervisor.supervise_loop(shutdown_flag) {
                        eprintln!("Supervisor for '{}' exited with error: {:?}", name_clone_for_thread, e);
                    }
                })?;

            self.handles.insert(name_clone_for_insert, handle);
        }
        Ok(())
    }

    /// Shutdown all supervisors and join their threads
    pub fn shutdown(&mut self) -> Result<(), BloomError> {
        if self.supervisors.is_empty() {
            let mut file = self.file_logger.lock().unwrap();
            file.log(LogLevel::Warn, "Shutdown requested but no services to stop");
        } else {
            for supervisor_mutex in self.supervisors.values() {
                let mut supervisor = supervisor_mutex.lock().unwrap();
                if let Err(e) = supervisor.shutdown() {
                    let mut file = self.file_logger.lock().unwrap();
                    file.log(LogLevel::Fail, &format!("Failed to shutdown service '{}': {}", supervisor.service.name, e));
                }
            }
        }

        // Join all threads
        for (name, handle) in self.handles.drain() {
            if let Err(e) = handle.join() {
                let mut file = self.file_logger.lock().unwrap();
                file.log(LogLevel::Fail, &format!("Failed to join thread '{}': {:?}", name, e));
            }
        }

        Ok(())
    }

    pub fn get_console_logger(&self) -> &Arc<Mutex<dyn ConsoleLogger + Send + Sync>> {
        &self.console_logger
    }

    pub fn get_file_logger(&self) -> &Arc<Mutex<dyn FileLogger + Send + Sync>> {
        &self.file_logger
    }
}

