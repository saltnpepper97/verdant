use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::process;

pub const LOCK_PATH: &str = "/run/verdantd.pid";

pub fn acquire_pid_lock() -> Result<(), String> {
    if let Ok(mut file) = OpenOptions::new().read(true).open(LOCK_PATH) {
        let mut contents = String::new();
        if file.read_to_string(&mut contents).is_ok() {
            if let Ok(pid) = contents.trim().parse::<u32>() {
                // Check if /proc/<pid> exists
                if fs::metadata(format!("/proc/{}", pid)).is_ok() {
                    return Err(format!("verdantd is already running as PID {}", pid));
                }
            }
        }
    }

    // Overwrite with our own PID
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(LOCK_PATH)
        .map_err(|e| format!("Failed to write lock file: {}", e))?;

    writeln!(file, "{}", process::id()).map_err(|e| format!("Failed to write PID: {}", e))?;
    Ok(())
}
