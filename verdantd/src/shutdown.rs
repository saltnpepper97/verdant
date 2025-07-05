use bloom::errors::BloomError;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::supervisor::Supervisor;
use std::fs;
use std::sync::OnceLock;

const SHUTDOWN_TIMEOUT_SECS: u64 = 5;

static CONSOLE_TTY: OnceLock<Option<String>> = OnceLock::new();

fn get_console_tty() -> Option<String> {
    CONSOLE_TTY.get_or_init(|| {
        let content = fs::read_to_string("/proc/cmdline").unwrap_or_default();
        for token in content.split_whitespace() {
            if let Some(tty) = token.strip_prefix("console=") {
                return Some(tty.to_string());
            }
        }
        None
    }).clone()
}

pub fn shutdown_all(supervisors: &[Arc<Mutex<Supervisor>>]) -> Result<(), BloomError> {
    let mut failures = Vec::new();
    let console_tty = get_console_tty();

    for supervisor in supervisors {
        let mut sup = supervisor.lock().unwrap();

        // Skip stopping getty@console tty on shutdown to avoid hang
        if let Some(ref tty) = console_tty {
            if sup.service.name.starts_with("tty@") || sup.service.name.starts_with("getty@") {
                if let Some(instance) = sup.service.name.split('@').nth(1) {
                    if instance == tty {
                        // Skip stopping this getty service on shutdown
                        continue;
                    }
                }
            }
        }

        if let Some(handle) = sup.handle.as_mut() {
            match handle.wait_with_timeout(Duration::from_secs(SHUTDOWN_TIMEOUT_SECS)) {
                Ok(Some(_exit_code)) => {}
                Ok(None) => {
                    if let Err(e) = handle.kill() {
                        failures.push(format!("Failed to kill {}: {}", sup.service.name, e));
                    } else {
                        if let Err(e) = handle.wait_with_timeout(Duration::from_secs(3)) {
                            failures.push(format!("Post-kill wait failed for {}: {}", sup.service.name, e));
                        }
                    }
                }
                Err(e) => {
                    failures.push(format!("Error waiting for {}: {}", sup.service.name, e));
                }
            }
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(BloomError::Custom(format!(
            "Shutdown completed with errors: {}",
            failures.join("; ")
        )))
    }
}

