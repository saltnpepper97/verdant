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
    if let Some(stop_cmd) = &service.stop_cmd {
        console_logger.message(LogLevel::Info, &format!("Running stop command for '{}'", service.name), std::time::Duration::ZERO);
        file_logger.log(LogLevel::Info, &format!("Running stop command for '{}'", service.name));

        let status = Command::new("sh")
            .arg("-c")
            .arg(stop_cmd)
            .status()
            .map_err(BloomError::Io)?;

        if !status.success() {
            let msg = format!("Stop command failed for '{}'", service.name);
            console_logger.message(LogLevel::Warn, &msg, std::time::Duration::ZERO);
            file_logger.log(LogLevel::Warn, &msg);
        } else {
            let msg = format!("Stop command succeeded for '{}'", service.name);
            console_logger.message(LogLevel::Info, &msg, std::time::Duration::ZERO);
            file_logger.log(LogLevel::Info, &msg);
        }

        std::thread::sleep(Duration::from_secs(service.timeout_stop.unwrap_or(5)));
    }

    let pid_i32 = pid as i32;
    let nix_pid = nix::unistd::Pid::from_raw(pid_i32);

    // Try sending SIGTERM first
    if let Err(e) = nix::sys::signal::kill(nix_pid, nix::sys::signal::Signal::SIGTERM) {
        // Without errno, just treat ESRCH as process gone and ignore any error else return error
        // We guess ESRCH by error string containing "No such process" (not reliable, but no errno)
        let err_str = format!("{}", e);
        if err_str.contains("No such process") {
            let msg = format!("Process {} not found; assuming already stopped", pid);
            console_logger.message(LogLevel::Info, &msg, std::time::Duration::ZERO);
            file_logger.log(LogLevel::Info, &msg);
            return Ok(());
        } else {
            let msg = format!("Failed to send SIGTERM to pid {}: {}", pid, e);
            console_logger.message(LogLevel::Fail, &msg, std::time::Duration::ZERO);
            file_logger.log(LogLevel::Fail, &msg);
            return Err(BloomError::Custom(msg));
        }
    }

    console_logger.message(LogLevel::Info, &format!("Sent SIGTERM to pid {}", pid), std::time::Duration::ZERO);
    file_logger.log(LogLevel::Info, &format!("Sent SIGTERM to pid {}", pid));

    let timeout = Duration::from_secs(service.timeout_stop.unwrap_or(5));
    let start = Instant::now();

    loop {
        match nix::sys::signal::kill(nix_pid, None) {
            Ok(_) => {
                if start.elapsed() >= timeout {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                let err_str = format!("{}", e);
                if err_str.contains("No such process") {
                    let msg = format!("Process {} exited after SIGTERM", pid);
                    console_logger.message(LogLevel::Info, &msg, std::time::Duration::ZERO);
                    file_logger.log(LogLevel::Info, &msg);
                    return Ok(());
                } else {
                    let msg = format!("Error checking process {} status: {}", pid, e);
                    console_logger.message(LogLevel::Warn, &msg, std::time::Duration::ZERO);
                    file_logger.log(LogLevel::Warn, &msg);
                    break;
                }
            }
        }
    }

    if let Err(e) = nix::sys::signal::kill(nix_pid, nix::sys::signal::Signal::SIGKILL) {
        let err_str = format!("{}", e);
        if err_str.contains("No such process") {
            let msg = format!("Process {} already exited before SIGKILL", pid);
            console_logger.message(LogLevel::Info, &msg, std::time::Duration::ZERO);
            file_logger.log(LogLevel::Info, &msg);
            return Ok(());
        } else {
            let msg = format!("Failed to send SIGKILL to pid {}: {}", pid, e);
            console_logger.message(LogLevel::Fail, &msg, std::time::Duration::ZERO);
            file_logger.log(LogLevel::Fail, &msg);
            return Err(BloomError::Custom(msg));
        }
    }

    console_logger.message(LogLevel::Warn, &format!("Sent SIGKILL to pid {}", pid), std::time::Duration::ZERO);
    file_logger.log(LogLevel::Warn, &format!("Sent SIGKILL to pid {}", pid));

    let kill_timeout = Duration::from_secs(2);
    let start_kill = Instant::now();

    loop {
        match nix::sys::signal::kill(nix_pid, None) {
            Ok(_) => {
                if start_kill.elapsed() >= kill_timeout {
                    let msg = format!("Process {} did not exit after SIGKILL", pid);
                    console_logger.message(LogLevel::Fail, &msg, std::time::Duration::ZERO);
                    file_logger.log(LogLevel::Fail, &msg);
                    return Err(BloomError::Custom(msg));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                let err_str = format!("{}", e);
                if err_str.contains("No such process") {
                    let msg = format!("Process {} exited after SIGKILL", pid);
                    console_logger.message(LogLevel::Info, &msg, std::time::Duration::ZERO);
                    file_logger.log(LogLevel::Info, &msg);
                    return Ok(());
                } else {
                    let msg = format!("Error checking process {} status: {}", pid, e);
                    console_logger.message(LogLevel::Warn, &msg, std::time::Duration::ZERO);
                    file_logger.log(LogLevel::Warn, &msg);
                    return Err(BloomError::Custom(msg));
                }
            }
        }
    }
}

