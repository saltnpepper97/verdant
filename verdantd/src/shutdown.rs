use bloom::errors::BloomError;
use std::sync::{Arc, Mutex};

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

        if sup.handle.is_some() {
            if let Err(e) = sup.stop() {
                failures.push(format!("Failed to stop {}: {}", sup.service.name, e));
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

