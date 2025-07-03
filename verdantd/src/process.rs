use std::process::{Command, Child, Stdio};
use std::fs::OpenOptions;
use std::time::{Instant, Duration};

use bloom::errors::BloomError;
use bloom::log::{FileLogger, ConsoleLogger};
use bloom::status::LogLevel;

use crate::service_file::ServiceFile;

pub struct LaunchInfo {
    pub child: Child,
    pub start_time: Instant,
}

/// Start the given service command with stdout/stderr redirected to log files.
/// Returns a LaunchInfo with the child process handle.
pub fn start_service(
    service: &ServiceFile,
    file_logger: &mut dyn FileLogger,
) -> Result<LaunchInfo, BloomError> {
    let now = Instant::now();

    // Open log files
    let stdout_log_path = service.stdout_log.as_ref()
        .ok_or_else(|| BloomError::Custom("Missing stdout_log path".into()))?;
    let stderr_log_path = service.stderr_log.as_ref()
        .ok_or_else(|| BloomError::Custom("Missing stderr_log path".into()))?;

    let stdout_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(stdout_log_path)
        .map_err(BloomError::Io)?;

    let stderr_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(stderr_log_path)
        .map_err(BloomError::Io)?;

    // Build command
    let mut cmd = Command::new(&service.cmd);

    if let Some(args) = &service.args {
        cmd.args(args);
    }

    if let Some(dir) = &service.working_dir {
        cmd.current_dir(dir);
    }

    // Set environment variables
    if let Some(envs) = &service.env {
        for env_var in envs {
            if let Some((k, v)) = env_var.split_once('=') {
                cmd.env(k, v);
            }
        }
    }

    // Redirect output
    cmd.stdout(Stdio::from(stdout_file));
    cmd.stderr(Stdio::from(stderr_file));

    // Spawn child
    let child = cmd.spawn().map_err(|e| {
        let msg = format!("Failed to start service '{}': {}", service.name, e);
        file_logger.log(LogLevel::Fail, &msg);
        BloomError::Io(e)
    })?;

    file_logger.log(LogLevel::Info, &format!("Started process '{}' (pid {})", service.name, child.id()));

    Ok(LaunchInfo { child, start_time: now })
}

/// Stop the service by sending stop command or killing the process
pub fn stop_service(
    service: &ServiceFile,
    pid: u32,
    console_logger: &mut dyn ConsoleLogger,
    file_logger: &mut dyn FileLogger,
) -> Result<(), BloomError> {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;
    use std::process::Command;
    use std::time::{Duration, Instant};

    let nix_pid = Pid::from_raw(pid as i32);

    // Check if process exists
    match kill(nix_pid, None) {
        Err(nix::Error::ESRCH) => {
            let msg = format!("Process {} already exited", pid);
            console_logger.message(LogLevel::Info, &msg, Duration::ZERO);
            file_logger.log(LogLevel::Info, &msg);
            return Ok(());
        }
        Err(e) => {
            let msg = format!("Error checking process {}: {}", pid, e);
            console_logger.message(LogLevel::Warn, &msg, Duration::ZERO);
            file_logger.log(LogLevel::Warn, &msg);
        }
        Ok(_) => {}
    }

    // Only run stop_cmd if process still alive
    if let Some(stop_cmd) = &service.stop_cmd {
        console_logger.message(LogLevel::Info, &format!("Running stop command for '{}'", service.name), Duration::ZERO);
        file_logger.log(LogLevel::Info, &format!("Running stop command for '{}'", service.name));

        let status = Command::new("sh")
            .arg("-c")
            .arg(stop_cmd)
            .status()
            .map_err(BloomError::Io)?;

        let level = if status.success() { LogLevel::Info } else { LogLevel::Warn };
        let msg = if status.success() {
            format!("Stop command succeeded for '{}'", service.name)
        } else {
            format!("Stop command failed for '{}'", service.name)
        };
        console_logger.message(level, &msg, Duration::ZERO);
        file_logger.log(level, &msg);

        // Short sleep to let stop_cmd do its job, capped to 1s max
        std::thread::sleep(Duration::from_secs(service.timeout_stop.unwrap_or(1).min(1)));
    }

    // Send SIGTERM
    let _ = kill(nix_pid, Signal::SIGTERM);
    console_logger.message(LogLevel::Info, &format!("Sent SIGTERM to pid {}", pid), Duration::ZERO);
    file_logger.log(LogLevel::Info, &format!("Sent SIGTERM to pid {}", pid));

    // Poll quickly for up to 500ms for process exit
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(500) {
        match kill(nix_pid, None) {
            Ok(_) => std::thread::sleep(Duration::from_millis(50)),
            Err(nix::Error::ESRCH) => {
                let msg = format!("Process {} exited after SIGTERM", pid);
                console_logger.message(LogLevel::Info, &msg, Duration::ZERO);
                file_logger.log(LogLevel::Info, &msg);
                return Ok(());
            }
            Err(e) => {
                let msg = format!("Error checking process {} status: {}", pid, e);
                console_logger.message(LogLevel::Warn, &msg, Duration::ZERO);
                file_logger.log(LogLevel::Warn, &msg);
                break;
            }
        }
    }

    // Send SIGKILL if still alive
    let _ = kill(nix_pid, Signal::SIGKILL);
    console_logger.message(LogLevel::Warn, &format!("Sent SIGKILL to pid {}", pid), Duration::ZERO);
    file_logger.log(LogLevel::Warn, &format!("Sent SIGKILL to pid {}", pid));

    // Wait briefly after SIGKILL (max 200ms)
    let kill_start = Instant::now();
    while kill_start.elapsed() < Duration::from_millis(200) {
        match kill(nix_pid, None) {
            Ok(_) => std::thread::sleep(Duration::from_millis(50)),
            Err(nix::Error::ESRCH) => {
                let msg = format!("Process {} exited after SIGKILL", pid);
                console_logger.message(LogLevel::Info, &msg, Duration::ZERO);
                file_logger.log(LogLevel::Info, &msg);
                return Ok(());
            }
            Err(e) => {
                let msg = format!("Error checking process {} status: {}", pid, e);
                console_logger.message(LogLevel::Warn, &msg, Duration::ZERO);
                file_logger.log(LogLevel::Warn, &msg);
                break;
            }
        }
    }

    // If still here, process stubbornly alive or gone but weird
    let msg = format!("Process {} did not exit after SIGKILL, continuing anyway", pid);
    console_logger.message(LogLevel::Warn, &msg, Duration::ZERO);
    file_logger.log(LogLevel::Warn, &msg);

    Ok(())
}

