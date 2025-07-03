use std::fs::{self, File};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Instant;
use nix::unistd::Pid;

use nix::unistd::{setgid, setuid, Gid, Uid};
use nix::libc;

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;
use bloom::time::ProcessTimer;

use crate::service_file::ServiceFile;

pub struct LaunchResult {
    pub child: Child,
    pub start_time: Instant,
}

pub fn start_service(
    service: &ServiceFile,
    file_logger: &mut dyn FileLogger,
) -> Result<LaunchResult, BloomError> {

    // Prepare stdout/stderr log paths or defaults if not set
    let stdout_log = match &service.stdout_log {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(format!("/var/log/verdant/services/{}.out.log", service.name)),
    };
    let stderr_log = match &service.stderr_log {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(format!("/var/log/verdant/services/{}.err.log", service.name)),
    };

    // Ensure log directory exists
    if let Some(parent) = stdout_log.parent() {
        fs::create_dir_all(parent).map_err(BloomError::Io)?;
    }

    // Open log files
    let stdout_file = File::create(&stdout_log).map_err(BloomError::Io)?;
    let stderr_file = File::create(&stderr_log).map_err(BloomError::Io)?;

    // Prepare command
    let mut cmd = Command::new(&service.cmd);
    if let Some(args) = &service.args {
        cmd.args(args);
    }

    // Setup env vars
    if let Some(envs) = &service.env {
        for env_var in envs {
            if let Some((k, v)) = env_var.split_once('=') {
                cmd.env(k, v);
            }
        }
    }

    // Working directory
    if let Some(dir) = &service.working_dir {
        cmd.current_dir(dir);
    }

    // Redirect stdout/stderr
    cmd.stdout(Stdio::from(stdout_file));
    cmd.stderr(Stdio::from(stderr_file));

    // User and group switching (must be root for this to work)
    if let Some(user) = &service.user {
        // Resolve user to uid
        let uid = nix::unistd::User::from_name(user)
            .map_err(|e| BloomError::Custom(format!("Failed to lookup user {}: {}", user, e)))?
            .ok_or_else(|| BloomError::Custom(format!("User {} not found", user)))?
            .uid;

        // Use pre_exec to set UID after fork, before exec
        unsafe {
            cmd.pre_exec(move || {
                setuid(Uid::from_raw(uid.as_raw()))?;
                Ok(())
            });
        }
    }
    if let Some(group) = &service.group {
        let gid = nix::unistd::Group::from_name(group)
            .map_err(|e| BloomError::Custom(format!("Failed to lookup group {}: {}", group, e)))?
            .ok_or_else(|| BloomError::Custom(format!("Group {} not found", group)))?
            .gid;

        unsafe {
            cmd.pre_exec(move || {
                setgid(Gid::from_raw(gid.as_raw()))?;
                Ok(())
            });
        }
    }

    // Set umask
    if let Some(umask_str) = &service.umask {
        if let Ok(umask_val) = u32::from_str_radix(umask_str, 8) {
            unsafe {
                cmd.pre_exec(move || {
                    libc::umask(umask_val);
                    Ok(())
                });
            }
        }
    }

    // Set nice priority
    if let Some(nice_val) = service.nice {
        unsafe {
            cmd.pre_exec(move || {
                libc::setpriority(libc::PRIO_PROCESS, 0, nice_val);
                Ok(())
            });
        }
    }

    // Spawn child
    let child = cmd.spawn().map_err(|e| {
        BloomError::Custom(format!("Failed to spawn service {}: {}", service.name, e))
    })?;

    let msg = format!("Launched service '{}', pid {}", service.name, child.id());
    file_logger.log(LogLevel::Ok, &msg);

    Ok(LaunchResult {
        child,
        start_time: Instant::now(),
    })
}

pub fn stop_service(
    service: &ServiceFile,
    child_pid: u32,
    console_logger: &mut dyn ConsoleLogger,
    file_logger: &mut dyn FileLogger,
) -> Result<(), BloomError> {
    use nix::unistd::Pid;
    use nix::sys::signal::{kill, Signal};
    use std::{thread, time::Duration};

    let timer = ProcessTimer::start();
    let pid = Pid::from_raw(child_pid as i32);

    let mut was_custom_stopped = false;

    if let Some(stop_cmd) = &service.stop_cmd {
        let cmdline = stop_cmd.replace("$MAINPID", &child_pid.to_string());

        let status = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(&cmdline)
            .status()
            .map_err(|e| BloomError::Custom(format!("Failed to execute stop-cmd: {}", e)))?;

        was_custom_stopped = true;

        if !status.success() {
            let msg = format!("stop-cmd '{}' failed with exit {:?}", cmdline, status.code());
            console_logger.message(LogLevel::Warn, &msg, timer.elapsed());
            file_logger.log(LogLevel::Warn, &msg);
        }
    } else {
        // Send SIGTERM
        let _ = kill(pid, Signal::SIGTERM).map_err(|e| {
            BloomError::Custom(format!("Failed to send SIGTERM to pid {}: {}", pid, e))
        })?;
    }

    // Give the process up to 2 seconds to exit
    for _ in 0..20 {
        if !process_exists(pid.as_raw()) {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    // If it's still running, send SIGKILL
    if process_exists(pid.as_raw()) {
        let _ = kill(pid, Signal::SIGKILL).map_err(|e| {
            BloomError::Custom(format!("Failed to send SIGKILL to pid {}: {}", pid, e))
        })?;

        let msg = format!("Service '{}' did not exit, sent SIGKILL", service.name);
        console_logger.message(LogLevel::Warn, &msg, timer.elapsed());
        file_logger.log(LogLevel::Warn, &msg);

        // Optionally wait a bit longer after SIGKILL
        thread::sleep(Duration::from_millis(300));
    }

    let msg = format!(
        "Stopped service '{}' (pid {}), method: {}",
        service.name,
        child_pid,
        if was_custom_stopped { "stop-cmd or SIGTERM" } else { "SIGTERM/SIGKILL" }
    );
    console_logger.message(LogLevel::Ok, &msg, timer.elapsed());
    file_logger.log(LogLevel::Ok, &msg);

    Ok(())
}

fn process_exists(pid: i32) -> bool {
    // Sending signal 0 doesn't kill the process but checks if it exists
    nix::sys::signal::kill(Pid::from_raw(pid), None).is_ok()
}
