use std::fs::{self, File};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use nix::unistd::{setgid, setuid, Gid, Uid};

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
    let stdout_log = match &service.stdout_log {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(format!("/var/log/verdant/services/{}.out.log", service.name)),
    };
    let stderr_log = match &service.stderr_log {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(format!("/var/log/verdant/services/{}.err.log", service.name)),
    };

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

    let user = service.user.clone();
    let group = service.group.clone();
    let umask_val = service.umask.as_ref().and_then(|u| u32::from_str_radix(u, 8).ok());
    let nice_val = service.nice;

    unsafe {
        cmd.pre_exec(move || {
            // Set process group leader
            libc::setpgid(0, 0);

            // Set group
            if let Some(group_name) = &group {
                let gid = nix::unistd::Group::from_name(group_name)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to lookup group {}: {}", group_name, e)))?
                    .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, format!("Group {} not found", group_name)))?
                    .gid;
                setgid(Gid::from_raw(gid.as_raw()))?;
            }

            // Set user
            if let Some(user_name) = &user {
                let uid = nix::unistd::User::from_name(user_name)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("Failed to lookup user {}: {}", user_name, e)))?
                    .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, format!("User {} not found", user_name)))?
                    .uid;
                setuid(Uid::from_raw(uid.as_raw()))?;
            }

            // Set umask
            if let Some(umask) = umask_val {
                libc::umask(umask);
            }

            // Set nice
            if let Some(nice) = nice_val {
                libc::setpriority(libc::PRIO_PROCESS, 0, nice);
            }

            Ok(())
        });
    }

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
        #[cfg(unix)]
        {
            use nix::sys::signal::{kill, Signal};
            use nix::unistd::Pid;

            let pgid = -(child_pid as i32);
            let res = kill(Pid::from_raw(pgid), Signal::SIGTERM);

            if let Err(e) = res {
                return Err(BloomError::Custom(format!("Failed to send SIGTERM to pgid {}: {}", pgid, e)));
            }
        }
    }

    let msg = format!("Stopped service '{}', pid {}", service.name, child_pid);
    console_logger.message(LogLevel::Ok, &msg, timer.elapsed());
    file_logger.log(LogLevel::Ok, &msg);

    Ok(())
}

