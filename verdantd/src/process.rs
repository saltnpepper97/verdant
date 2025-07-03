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

    let nix_pid = Pid::from_raw(pid as i32);

    // Run stop command if any
    if let Some(stop_cmd) = &service.stop_cmd {
        console_logger.message(LogLevel::Info, &format!("Running stop command for '{}'", service.name), Duration::ZERO);
        file_logger.log(LogLevel::Info, &format!("Running stop command for '{}'", service.name));

        let status = Command::new("sh")
            .arg("-c")
            .arg(stop_cmd)
            .status()
            .map_err(BloomError::Io)?;

        if !status.success() {
            let msg = format!("Stop command failed for '{}'", service.name);
            console_logger.message(LogLevel::Warn, &msg, Duration::ZERO);
            file_logger.log(LogLevel::Warn, &msg);
        } else {
            let msg = format!("Stop command succeeded for '{}'", service.name);
            console_logger.message(LogLevel::Info, &msg, Duration::ZERO);
            file_logger.log(LogLevel::Info, &msg);
        }

        std::thread::sleep(Duration::from_secs(service.timeout_stop.unwrap_or(5)));
    }

    // Helper: check if process exists
    fn process_exists(pid: Pid) -> Result<bool, BloomError> {
        match kill(pid, None) {
            Ok(_) => Ok(true),
            Err(nix::Error::ESRCH) => Ok(false),
            Err(e) => Err(BloomError::Custom(format!("Error checking process status: {}", e))),
        }
    }

    // Send SIGTERM and wait for graceful exit
    if let Err(e) = kill(nix_pid, Signal::SIGTERM) {
        if let nix::Error::ESRCH = e {
            let msg = format!("Process {} not found; already stopped", pid);
            console_logger.message(LogLevel::Info, &msg, Duration::ZERO);
            file_logger.log(LogLevel::Info, &msg);
            return Ok(());
        } else {
            let msg = format!("Failed to send SIGTERM to pid {}: {}", pid, e);
            console_logger.message(LogLevel::Fail, &msg, Duration::ZERO);
            file_logger.log(LogLevel::Fail, &msg);
            return Err(BloomError::Custom(msg));
        }
    }

    console_logger.message(LogLevel::Info, &format!("Sent SIGTERM to pid {}", pid), Duration::ZERO);
    file_logger.log(LogLevel::Info, &format!("Sent SIGTERM to pid {}", pid));

    let timeout = Duration::from_secs(service.timeout_stop.unwrap_or(5));
    let start = Instant::now();

    while start.elapsed() < timeout {
        match process_exists(nix_pid)? {
            false => {
                let msg = format!("Process {} exited after SIGTERM", pid);
                console_logger.message(LogLevel::Info, &msg, Duration::ZERO);
                file_logger.log(LogLevel::Info, &msg);
                return Ok(());
            }
            true => {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }

    // Timeout expired, send SIGKILL
    if let Err(e) = kill(nix_pid, Signal::SIGKILL) {
        if let nix::Error::ESRCH = e {
            let msg = format!("Process {} already exited before SIGKILL", pid);
            console_logger.message(LogLevel::Info, &msg, Duration::ZERO);
            file_logger.log(LogLevel::Info, &msg);
            return Ok(());
        } else {
            let msg = format!("Failed to send SIGKILL to pid {}: {}", pid, e);
            console_logger.message(LogLevel::Fail, &msg, Duration::ZERO);
            file_logger.log(LogLevel::Fail, &msg);
            return Err(BloomError::Custom(msg));
        }
    }

    console_logger.message(LogLevel::Warn, &format!("Sent SIGKILL to pid {}", pid), Duration::ZERO);
    file_logger.log(LogLevel::Warn, &format!("Sent SIGKILL to pid {}", pid));

    let kill_timeout = Duration::from_secs(2);
    let start_kill = Instant::now();

    while start_kill.elapsed() < kill_timeout {
        match process_exists(nix_pid)? {
            false => {
                let msg = format!("Process {} exited after SIGKILL", pid);
                console_logger.message(LogLevel::Info, &msg, Duration::ZERO);
                file_logger.log(LogLevel::Info, &msg);
                return Ok(());
            }
            true => {
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }

    let msg = format!("Process {} did not exit after SIGKILL", pid);
    console_logger.message(LogLevel::Fail, &msg, Duration::ZERO);
    file_logger.log(LogLevel::Fail, &msg);
    Err(BloomError::Custom(msg))
}

