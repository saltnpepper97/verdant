use bloom::errors::BloomError;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::supervisor::Supervisor;

/// Timeout per service shutdown
const SHUTDOWN_TIMEOUT_SECS: u64 = 5;

/// Orchestrate clean shutdown of all supervisors.
/// Stops each service, waits for them to stop or forcibly kills after timeout.
/// Returns Ok(()) if all services stopped cleanly, else Err with details.
pub fn shutdown_all(supervisors: &[Arc<Mutex<Supervisor>>]) -> Result<(), BloomError> {
    let mut failures = Vec::new();

    for supervisor in supervisors {
        let mut sup = supervisor.lock().unwrap();

        if let Some(handle) = sup.handle.as_mut() {
            // First try clean stop
            match handle.wait_with_timeout(Duration::from_secs(SHUTDOWN_TIMEOUT_SECS)) {
                Ok(Some(_exit_code)) => {
                    // Stopped cleanly
                }
                Ok(None) => {
                    // Timeout: force kill
                    if let Err(e) = handle.kill() {
                        failures.push(format!("Failed to kill {}: {}", sup.service.name, e));
                    } else {
                        // Wait again after SIGKILL
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

