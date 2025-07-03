use std::fs::{self, File};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Instant;

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
    let timer = ProcessTimer::start();

    // Check if TTY is logged in via fuser
    let mut used_tty = false;
    if let Some(tty) = service.name.strip_prefix("tty@") {
        let tty_path = format!("/dev/{}", tty);
        let output = std::process::Command::new("fuser")
            .arg(&tty_path)
            .output();

        if let Ok(out) = output {
            used_tty = !out.stdout.is_empty();
        }

        if used_tty {
            let msg = format!("TTY '{}' is in use; sending SIGKILL to '{}'", tty, service.name);
            console_logger.message(LogLevel::Warn, &msg, timer.elapsed());
            file_logger.log(LogLevel::Warn, &msg);

            #[cfg(unix)]
            {
                use nix::sys::signal::{kill, Signal};
                use nix::unistd::Pid;
                let _ = kill(Pid::from_raw(child_pid as i32), Signal::SIGKILL);
            }

            let msg = format!("Forcefully killed service '{}', pid {}", service.name, child_pid);
            console_logger.message(LogLevel::Ok, &msg, timer.elapsed());
            file_logger.log(LogLevel::Ok, &msg);

            return Ok(());
        }
    }

    // Normal stop process
    if let Some(stop_cmd) = &service.stop_cmd {
        let cmdline = stop_cmd.replace("$MAINPID", &child_pid.to_string());

        let status = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg(&cmdline)
            .status()
            .map_err(|e| BloomError::Custom(format!("Failed to execute stop-cmd: {}", e)))?;

        if !status.success() {
            let msg = format!("stop-cmd '{}' failed with exit {:?}", cmdline, status.code());
            console_logger.message(LogLevel::Warn, &msg, timer.elapsed());
            file_logger.log(LogLevel::Warn, &msg);
        }
    } else {
        nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(child_pid as i32),
            nix::sys::signal::Signal::SIGTERM,
        )
        .map_err(|e| BloomError::Custom(format!("Failed to send SIGTERM: {}", e)))?;
    }

    let msg = format!("Stopped service '{}', pid {}", service.name, child_pid);
    console_logger.message(LogLevel::Ok, &msg, timer.elapsed());
    file_logger.log(LogLevel::Ok, &msg);

    Ok(())
}

