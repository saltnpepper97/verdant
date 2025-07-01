use std::collections::HashMap;
use std::sync::{Arc, Mutex};
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

        // First start all startup_package services and collect their names
        for (name, supervisor) in self.supervisors.iter_mut() {
            if supervisor.service.startup_package.is_some() {
                supervisor.start()?;
                to_spawn.push(name.clone());
                started_count += 1;
            }
        }

        // Then spawn supervisor threads after iteration to avoid double mutable borrow
        for name in to_spawn {
            self.spawn_supervisor_thread(name)?;
        }

        {
            let mut con = self.console_logger.lock().unwrap();
            let mut file = self.file_logger.lock().unwrap();

            if started_count > 0 {
                let msg = format!("Started {} service(s) marked with startup_package", started_count);
                file.log(bloom::status::LogLevel::Info, &msg);
            } else {
                let msg = "No startup packages found";
                con.message(bloom::status::LogLevel::Warn, msg, std::time::Duration::ZERO);
                file.log(bloom::status::LogLevel::Warn, msg);
            }
        }

        Ok(())
    }

    pub fn start_service(&mut self, name: &str) -> Result<(), BloomError> {
        let supervisor = self.supervisors.get_mut(name)
            .ok_or_else(|| BloomError::Custom(format!("Service '{}' not found", name)))?;
        supervisor.start()?;
        self.spawn_supervisor_thread(name.to_string())
    }

    pub fn stop_service(&mut self, name: &str) -> Result<(), BloomError> {
        // ToDo: Consider stopping the thread and cleaning up the handle.
        let supervisor = self.supervisors.get_mut(name)
            .ok_or_else(|| BloomError::Custom(format!("Service '{}' not found", name)))?;
        supervisor.stop()
    }

    pub fn spawn_supervisor_thread(&mut self, name: String) -> Result<(), BloomError> {
        if self.handles.contains_key(&name) {
            // Thread already running for this service
            return Ok(());
        }

        let supervisor = self.supervisors.get_mut(&name)
            .ok_or_else(|| BloomError::Custom(format!("Service '{}' not found", name)))?;

        // Move supervisor into thread closure
        // We clone Arc loggers for the thread since Supervisor has those Arcs
        let mut supervisor = std::mem::replace(supervisor, Supervisor::new(
            supervisor.service.clone(),
            Arc::clone(&self.console_logger),
            Arc::clone(&self.file_logger),
        ));

        let handle = thread::spawn(move || {
            if let Err(e) = supervisor.supervise_loop() {
                eprintln!("Supervisor for service '{}' exited with error: {:?}", supervisor.service.name, e);
            }
        });

        self.handles.insert(name, handle);
        Ok(())
    }

    /// Spawns supervisor threads for all services currently in `self.supervisors`.
    /// Returns immediately, threads run in the background.
    pub fn supervise_all(&mut self) -> Result<(), BloomError> {
        for name in self.supervisors.keys().cloned().collect::<Vec<_>>() {
            self.spawn_supervisor_thread(name)?;
        }
        Ok(())
    }

    /// Shutdown all started services and their supervisor threads.
    pub fn shutdown(&mut self) -> Result<(), BloomError> {
        let mut file = self.file_logger.lock().unwrap();

        if self.supervisors.is_empty() {
            let msg = "Shutdown: No services to stop";
            file.log(LogLevel::Warn, msg);
        } else {
            // Stop all services
            for supervisor in self.supervisors.values_mut() {
                let _ = supervisor.shutdown(); // Ignore individual failures
            }
        }

        // Join all supervisor threads to wait for clean exit
        if self.handles.is_empty() {
            let msg = "Shutdown: No supervisor threads to join";
            file.log(LogLevel::Info, msg);
        } else {
            for (name, handle) in self.handles.drain() {
                if let Err(e) = handle.join() {
                    eprintln!("Supervisor thread for service '{}' panicked: {:?}", name, e);
                }
            }
        }
        
        self.supervisors.clear();
        Ok(())
    }
}

