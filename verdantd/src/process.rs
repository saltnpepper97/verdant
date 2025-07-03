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
    _pid: u32,
    console_logger: &mut dyn ConsoleLogger,
    file_logger: &mut dyn FileLogger,
) -> Result<(), BloomError> {
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
    } else {
        file_logger.log(LogLevel::Info, &format!(
            "No stop command defined for '{}', assuming service exits on shutdown",
            service.name
        ));
    }

    Ok(())
}

