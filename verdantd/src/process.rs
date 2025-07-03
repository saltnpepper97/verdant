use std::fs::{self, File};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use nix::unistd::{setgid, setuid, Gid, Uid};
use nix::libc;
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;

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
    let stdout_log = service.stdout_log.clone().map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/var/log/verdant/services/{}.out.log", service.name)));
    let stderr_log = service.stderr_log.clone().map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/var/log/verdant/services/{}.err.log", service.name)));

    if let Some(parent) = stdout_log.parent() {
        fs::create_dir_all(parent).map_err(BloomError::Io)?;
    }

    let stdout_file = File::create(&stdout_log).map_err(BloomError::Io)?;
    let stderr_file = File::create(&stderr_log).map_err(BloomError::Io)?;

    let mut cmd = Command::new(&service.cmd);
    if let Some(args) = &service.args {
        cmd.args(args);
    }

    if let Some(envs) = &service.env {
        for env_var in envs {
            if let Some((k, v)) = env_var.split_once('=') {
                cmd.env(k, v);
            }
        }
    }

    if let Some(dir) = &service.working_dir {
        cmd.current_dir(dir);
    }

    cmd.stdout(Stdio::from(stdout_file));
    cmd.stderr(Stdio::from(stderr_file));

    // Pre-exec UID/GID/nice
    let user_uid = service.user.as_ref().and_then(|user| {
        nix::unistd::User::from_name(user).ok().flatten().map(|u| u.uid)
    });

    let group_gid = service.group.as_ref().and_then(|group| {
        nix::unistd::Group::from_name(group).ok().flatten().map(|g| g.gid)
    });

    let nice_val = service.nice;

    let umask_val = service.umask.as_ref().and_then(|s| u32::from_str_radix(s, 8).ok());

    unsafe {
        cmd.pre_exec(move || {
            if let Some(gid) = group_gid {
                setgid(Gid::from_raw(gid.as_raw()))?;
            }
            if let Some(uid) = user_uid {
                setuid(Uid::from_raw(uid.as_raw()))?;
            }
            if let Some(umask) = umask_val {
                libc::umask(umask);
            }
            if let Some(nice) = nice_val {
                libc::setpriority(libc::PRIO_PROCESS, 0, nice);
            }
            Ok(())
        });
    }

    let child = cmd.spawn()
        .map_err(|e| BloomError::Custom(format!("Failed to spawn service {}: {}", service.name, e)))?;

    file_logger.log(LogLevel::Ok, &format!("Launched service '{}', pid {}", service.name, child.id()));

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
    let pid = Pid::from_raw(child_pid as i32);

    if let Some(stop_cmd) = &service.stop_cmd {
        let cmdline = stop_cmd.replace("$MAINPID", &child_pid.to_string());
        let status = Command::new("/bin/sh")
            .arg("-c")
            .arg(&cmdline)
            .status()
            .map_err(|e| BloomError::Custom(format!("Failed to run stop-cmd: {}", e)))?;

        if !status.success() {
            let msg = format!("stop-cmd failed: '{}', status: {:?}", cmdline, status.code());
            console_logger.message(LogLevel::Warn, &msg, timer.elapsed());
            file_logger.log(LogLevel::Warn, &msg);
        }
    } else {
        kill(pid, Signal::SIGTERM)
            .map_err(|e| BloomError::Custom(format!("Failed to send SIGTERM to pid {}: {}", child_pid, e)))?;
    }

    console_logger.message(LogLevel::Ok, &format!("Sent stop signal to '{}'", service.name), timer.elapsed());
    file_logger.log(LogLevel::Ok, &format!("Stopped '{}', pid {}", service.name, child_pid));
    Ok(())
}

