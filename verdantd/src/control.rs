use std::fs::OpenOptions;
use std::process::{Command, Child};
use std::io;
use std::time::{Duration, Instant};
use std::thread::sleep;

use crate::service::{RestartPolicy, Service};
use bloom::errors::BloomError;

pub struct ServiceHandle {
    pub child: Child,
    pub start_time: Instant,
    pub exit_status: Option<i32>, // Track exit code
}

impl ServiceHandle {
    pub fn is_running(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(Some(status)) => {
                self.exit_status = status.code(); // Record exit code
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
                    self.exit_status = status.code(); // Record on wait too
                    return Ok(status.code());
                }
                None => sleep(Duration::from_millis(50)),
            }
        }

        Ok(None) // timed out
    }

    pub fn kill(&mut self) -> io::Result<()> {
        self.child.kill()
    }
}

/// Start a service, spawning its process.
/// Returns a `ServiceHandle` on success.
pub fn start_service(service: &Service) -> Result<ServiceHandle, BloomError> {
    use std::os::unix::process::CommandExt;
    use std::os::unix::io::AsRawFd;
    use libc::{setsid, ioctl, TIOCSCTTY, close, dup2};

    let mut cmd = Command::new(&service.cmd);
    if !service.args.is_empty() {
        cmd.args(&service.args);
    }

    // Apply stdout redirection if explicitly set
    if let Some(ref path) = service.stdout {
        let stdout_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(BloomError::Io)?;
        cmd.stdout(stdout_file);
    }

    // Apply stderr redirection if explicitly set
    if let Some(ref path) = service.stderr {
        let stderr_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(BloomError::Io)?;
        cmd.stderr(stderr_file);
    }

    // If it's a tty@ service, set controlling terminal
    if service.name.starts_with("tty@") {
        // Extract tty device path, e.g. "tty1" → "/dev/tty1"
        let tty_name = service.name.trim_start_matches("tty@").to_string();
        let tty_path = format!("/dev/{}", tty_name);

        unsafe {
            cmd.pre_exec(move || {
                // Become session leader
                if setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }

                // Open the TTY device for read/write
                let fd = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&tty_path)
                    .map_err(|e| e)?;

                // Set the controlling terminal to this fd
                if ioctl(fd.as_raw_fd(), TIOCSCTTY, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }

                // Redirect stdin, stdout, stderr to the tty fd
                for stdfd in 0..3 {
                    // Close stdfd first
                    close(stdfd);
                    // Duplicate fd to stdfd
                    if dup2(fd.as_raw_fd(), stdfd) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }

                // fd will be closed automatically when going out of scope
                Ok(())
            });
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

