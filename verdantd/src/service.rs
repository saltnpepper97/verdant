use std::process::Child;
use std::collections::HashMap;
use anyhow::{Result, anyhow};

#[derive(Debug, Clone)]
pub struct ServiceConfig {
    pub name: String,
    pub exec: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub restart: bool,
    pub log: LogMode, // NEW
}

#[derive(Debug)]
pub struct Service {
    pub config: ServiceConfig,
    pub child: Option<Child>,
    pub running: bool,
}

/// Defines how logs should be handled for a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogMode {
    File,
    Null,
}

impl Service {
    /// Stop the running service process if it exists.
    pub fn stop(&mut self) -> Result<()> {
        if let Some(child) = &mut self.child {
            // Try to terminate the child process gracefully
            child.kill().map_err(|e| anyhow!("Failed to kill process: {}", e))?;
            // Wait for the child process to exit to avoid zombies
            child.wait().map_err(|e| anyhow!("Failed to wait on process: {}", e))?;
            self.running = false;
            self.child = None;
            Ok(())
        } else {
            Err(anyhow!("Service process not running"))
        }
    }
}
