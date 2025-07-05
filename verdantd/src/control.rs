
use std::fs::OpenOptions;
use std::process::{Command, Child};
use std::io;
use std::os::unix::process::CommandExt; // for pre_exec
use std::os::unix::io::AsRawFd; // for fd conversion
use std::time::{Duration, Instant};
use std::thread::sleep;

use nix::unistd::setsid;
use nix::fcntl::{open, OFlag};
use nix::sys::stat::Mode;
use nix::unistd::close;
use nix::errno::Errno;

use crate::service::{RestartPolicy, Service};
use bloom::errors::BloomError;

pub struct ServiceHandle {
    pub child: Child,
    pub start_time: Instant,
    pub exit_status: Option<i32>,
}

impl ServiceHandle {
    pub fn is_running(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(Some(status)) => {
                self.exit_status = status.code();
                false
            }
            Ok(None) => true,
            Err(_) => false,
        }
    }

    pub fn wait_with_timeout(&mut self, timeout: Duration) -> io::Result<Option<i32>> {
        let start = Instant::now();
        while start.elapsed() < timeout {
            match self.child.try_wait()? {
                Some(status) => {
                    self.exit_status = status.code();
                    return Ok(status.code());
                }
                None => sleep(Duration::from_millis(50)),
            }
        }
        Ok(None)
    }

    pub fn kill(&mut self) -> io::Result<()> {
        self.child.kill()
    }
}

fn setup_getty_preexec(tty_path: &str) -> Result<(), std::io::Error> {
    setsid().map_err(|e| io::Error::new(io::ErrorKind::Other, format!("setsid failed: {}", e)))?;

    let fd = open(tty_path, OFlag::O_RDWR | OFlag::O_NOCTTY, Mode::empty())
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("open tty failed: {}", e)))?;

    // Convert OwnedFd to raw fd for ioctl:
    let raw_fd = fd.as_raw_fd();

    let res = unsafe { libc::ioctl(raw_fd, libc::TIOCSCTTY, 0) };
    if res != 0 {
        let err = Errno::last();
        let _ = close(fd);
        return Err(io::Error::new(io::ErrorKind::Other, format!("ioctl TIOCSCTTY failed: {}", err)));
    }

    close(fd).ok();

    Ok(())
}

pub fn start_service(service: &Service) -> Result<ServiceHandle, BloomError> {
    let mut cmd = Command::new(&service.cmd);

    if !service.args.is_empty() {
        cmd.args(&service.args);
    }

    let is_getty = service.cmd.contains("getty") || service.cmd.contains("agetty") ||
                   service.name.starts_with("getty@") || service.name.starts_with("agetty@");

    if is_getty {
        // Fix: unify types to &str by mapping service.args iter & service.name split to &str
        let tty_opt = service.args.iter()
            .map(|s| s.as_str())
            .find(|arg| arg.starts_with("tty"))
            .or_else(|| service.name.split('@').nth(1));

        if let Some(tty) = tty_opt {
            let tty_path = format!("/dev/{}", tty);

            unsafe {
            cmd.pre_exec(move || {
                setup_getty_preexec(&tty_path)?;
                Ok(())
            });
            }
            // For getty, avoid stdio redirection to prevent conflicts
            cmd.stdin(std::process::Stdio::null());
            cmd.stdout(std::process::Stdio::null());
            cmd.stderr(std::process::Stdio::null());
        }
    } else {
        if let Some(ref path) = service.stdout {
            let stdout_file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(BloomError::Io)?;
            cmd.stdout(stdout_file);
        }

        if let Some(ref path) = service.stderr {
            let stderr_file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(BloomError::Io)?;
            cmd.stderr(stderr_file);
        }
    }

    let child = cmd.spawn().map_err(BloomError::Io)?;

    Ok(ServiceHandle {
        child,
        start_time: Instant::now(),
        exit_status: None,
    })
}


/// Stop a running service cleanly.
/// Returns Ok(true) if stopped gracefully, Ok(false) if killed forcibly.
pub fn stop_service(handle: &mut ServiceHandle, timeout: Duration) -> Result<bool, BloomError> {
    #[cfg(unix)]
    {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;

        let pid = Pid::from_raw(handle.child.id() as i32);

        // Check if it's already exited before signaling
        if let Ok(Some(_)) = handle.child.try_wait() {
            // Already exited
            return Ok(true);
        }

        kill(pid, Signal::SIGTERM).map_err(BloomError::from)?;

        match handle.wait_with_timeout(timeout)? {
            Some(_) => Ok(true),
            None => {
                kill(pid, Signal::SIGKILL).map_err(BloomError::from)?;
                match handle.wait_with_timeout(Duration::from_secs(5))? {
                    Some(_) => Ok(false),
                    None => Err(BloomError::Custom("Failed to kill service process".into())),
                }
            }
        }
    }
    #[cfg(not(unix))]
    {
        handle.kill().map_err(BloomError::Io)?;
        Ok(false)
    }
}

/// Restart a service according to its restart policy.
/// Returns Ok(Some(handle)) if restarted, Ok(None) if not restarted.
pub fn restart_service(
    service: &Service,
    current_handle: Option<ServiceHandle>,
) -> Result<Option<ServiceHandle>, BloomError> {
    match service.restart {
        RestartPolicy::Never => {
            if let Some(mut handle) = current_handle {
                stop_service(&mut handle, Duration::from_secs(5))?;
            }
            Ok(None)
        }
        RestartPolicy::Always => {
            if let Some(mut handle) = current_handle {
                let _ = stop_service(&mut handle, Duration::from_secs(5));
            }
            let new_handle = start_service(service)?;
            Ok(Some(new_handle))
        }
        RestartPolicy::OnFailure => {
            if let Some(mut handle) = current_handle {
                if handle.is_running() {
                    return Ok(Some(handle)); // still running
                }

                // Check if last exit status was a failure (non-zero)
                match handle.exit_status {
                    Some(code) if code != 0 => {
                        let new_handle = start_service(service)?;
                        Ok(Some(new_handle))
                    }
                    _ => Ok(None), // Exit code was 0 or unknown, don't restart
                }
            } else {
                let new_handle = start_service(service)?;
                Ok(Some(new_handle))
            }
        }
    }
}

