use std::process::{Child, Command, Stdio};
use std::io;
use std::fs::{File, OpenOptions};

use crate::service::{ServiceConfig, RestartPolicy};
use common::{print_step, status_fail, status_ok};

pub struct ManagedService {
    pub config: ServiceConfig,
    pub child: Option<Child>,
}

impl ManagedService {
    pub fn new(config: ServiceConfig) -> Self {
        Self { config, child: None }
    }

    pub fn launch(&mut self) -> io::Result<u32> {
        let mut cmd = Command::new(&self.config.exec);
        if let Some(args) = &self.config.args {
            cmd.args(args);
        }

        if let Some(tty_path) = &self.config.tty {
            use std::os::unix::io::AsRawFd;
            use std::os::unix::process::CommandExt;
            use libc::{self, setsid, ioctl, TIOCSCTTY};

            let tty = OpenOptions::new().read(true).write(true).open(tty_path)?;
            let tty_fd = tty.as_raw_fd();

            unsafe {
                cmd.stdin(Stdio::from(tty.try_clone()?));
                cmd.stdout(Stdio::from(tty.try_clone()?));
                cmd.stderr(Stdio::from(tty));

                cmd.pre_exec(move || {
                    if setsid() == -1 {
                        return Err(io::Error::last_os_error());
                    }
                    if ioctl(tty_fd, TIOCSCTTY, 1) == -1 {
                        return Err(io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        } else {
            let devnull = File::open("/dev/null")?;
            cmd.stdout(Stdio::from(devnull.try_clone()?));
            cmd.stderr(Stdio::from(devnull));
        }

        let child = cmd.spawn()?;
        let pid = child.id();
        self.child = Some(child);
        Ok(pid)
    }

    pub fn supervise(&mut self) -> io::Result<()> {
        if let Some(child) = &mut self.child {
            match child.try_wait()? {
                Some(status) => {
                    if !status.success() {
                        print_step(&format!("Service {} exited with {:?}", self.config.name, status), &status_fail());
                    }

                    match self.config.restart {
                        RestartPolicy::Always => {
                            print_step(&format!("Restarting service {} (policy: always)", self.config.name), &status_ok());
                            self.launch()?;
                        }
                        RestartPolicy::OnFailure if !status.success() => {
                            print_step(&format!("Restarting service {} (policy: on-failure)", self.config.name), &status_ok());
                            self.launch()?;
                        }
                        RestartPolicy::Never | RestartPolicy::OnFailure => {
                            self.child = None;
                        }
                    }
                }
                None => {}
            }
        } else if matches!(self.config.restart, RestartPolicy::Always) {
            self.launch()?;
        }
        Ok(())
    }
}
