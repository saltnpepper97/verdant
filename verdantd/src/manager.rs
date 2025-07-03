use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

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

    pub fn shutdown(&mut self, shutdown_flag: Arc<AtomicBool>) -> Result<(), BloomError> {
        {
            let mut file = self.file_logger.lock().unwrap();
            file.log(LogLevel::Info, "Shutdown: Setting shutdown flag for all supervisors");
        }

        // Signal all supervisor loops to exit
        shutdown_flag.store(true, Ordering::SeqCst);

        // Stop all child services gracefully
        for supervisor in self.supervisors.values_mut() {
            if let Err(e) = supervisor.shutdown() {
                let msg = format!("Error shutting down supervisor '{}': {}", supervisor.service.name, e);
                if let Ok(mut file) = self.file_logger.lock() {
                    file.log(LogLevel::Warn, &msg);
                }
            }
        }

        {
            let mut file = self.file_logger.lock().unwrap();
            file.log(LogLevel::Info, "Shutdown: Joining all supervisor threads");
        }

        // Join supervisor threads to ensure clean exit, with timeout per thread
        let join_timeout = Duration::from_secs(10);

        for (name, handle) in self.handles.drain() {
            let _start = Instant::now();
            loop {
                // There's no direct way to check if JoinHandle finished,
                // so we attempt to join with try_join (nightly) or just spawn a blocking join in another thread.
                // Here, we'll do a simple approach with blocking join in a thread with timeout:

                // Spawn a thread to join handle with timeout
                let join_result = std::thread::spawn(move || handle.join()).join_timeout(join_timeout);

                match join_result {
                    Ok(join_res) => {
                        if let Err(e) = join_res {
                            eprintln!("Supervisor thread for service '{}' panicked: {:?}", name, e);
                        }
                        break;
                    }
                    Err(_) => {
                        // Timeout happened, log and continue waiting or break?
                        let msg = format!("Timeout waiting for supervisor thread '{}' to join after {:?}. Continuing...", name, join_timeout);
                        if let Ok(mut file) = self.file_logger.lock() {
                            file.log(LogLevel::Warn, &msg);
                        }
                        break; // or continue waiting longer if you want
                    }
                }
            }
        }

        // Clear all supervisors after shutdown complete
        self.supervisors.clear();

        {
            let mut file = self.file_logger.lock().unwrap();
            file.log(LogLevel::Info, "Shutdown: Completed");
        }

        Ok(())
    }
}

trait JoinTimeout<T> {
    fn join_timeout(self, dur: Duration) -> Result<T, ()>;
}

impl<T: Send + 'static> JoinTimeout<T> for thread::JoinHandle<T> {
    fn join_timeout(self, dur: Duration) -> Result<T, ()> {
        use std::sync::mpsc;
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let res = self.join();
            let _ = tx.send(res);
        });

        match rx.recv_timeout(dur) {
            Ok(Ok(val)) => Ok(val),
            Ok(Err(_panic)) => Err(()), // thread panicked
            Err(_) => Err(()),          // timeout
        }
    }
}

