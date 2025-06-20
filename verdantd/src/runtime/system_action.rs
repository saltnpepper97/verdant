use std::{process::Command, io, thread, time::Instant};
use std::time::Duration;
use crate::managed_service::ManagedService;
use common::{print_step, print_substep, status_ok, status_warn};

pub enum SystemAction {
    Reboot,
    Shutdown,
}

pub fn shutdown_or_reboot(services: &mut [ManagedService], action: SystemAction) -> io::Result<()> {
    print_step("Stopping all services...", &status_ok());

    for svc in services.iter_mut().rev() {
        if let Some(child) = &mut svc.child {
            print_substep(&format!("Stopping service {}", svc.config.name), &status_ok());

            #[cfg(unix)]
            {
                use nix::sys::signal::{kill, Signal};
                use nix::unistd::Pid;

                let pid = Pid::from_raw(child.id() as i32);

                if let Err(e) = kill(pid, Signal::SIGTERM) {
                    eprintln!("Failed to send SIGTERM to {}: {}", svc.config.name, e);
                    continue;
                }

                let start = Instant::now();
                loop {
                    match child.try_wait()? {
                        Some(status) => {
                            print_substep(&format!("Service {} exited with {:?}", svc.config.name, status), &status_ok());
                            break;
                        }
                        None => {
                            if start.elapsed() > Duration::from_secs(5) {
                                eprintln!("Timeout. Sending SIGKILL to {}...", svc.config.name);
                                let _ = kill(pid, Signal::SIGKILL);
                                break;
                            }
                            thread::sleep(Duration::from_millis(200));
                        }
                    }
                }
            }

            #[cfg(not(unix))]
            {
                child.kill()?;
            }

            svc.child = None;
        }
    }

    print_step("All services stopped.", &status_ok());
    print_step("Syncing disks before reboot/shutdown...", &status_ok());
    let _ = Command::new("sync").status();
    thread::sleep(Duration::from_secs(1));

    #[cfg(target_os = "linux")]
    {
        use libc::{reboot, sync, RB_AUTOBOOT, RB_POWER_OFF};
        unsafe { sync(); }
        let result = unsafe {
            match action {
                SystemAction::Reboot => reboot(RB_AUTOBOOT),
                SystemAction::Shutdown => reboot(RB_POWER_OFF),
            }
        };
        if result == 0 {
            return Ok(());
        } else {
            eprintln!("libc reboot syscall failed.");
        }
    }

    let sysrq_result = std::fs::write(
        "/proc/sysrq-trigger",
        match action {
            SystemAction::Reboot => "b",
            SystemAction::Shutdown => "o",
        },
    );

    if sysrq_result.is_ok() {
        return Ok(());
    } else {
        eprintln!("/proc/sysrq-trigger write failed: {:?}", sysrq_result);
    }

    let _ = std::fs::remove_file("/run/verdantd.sock");

    let cmd = match action {
        SystemAction::Reboot => "reboot",
        SystemAction::Shutdown => "poweroff",
    };

    print_step(&format!("Fallback: Executing system command: {}", cmd), &status_warn());

    let status = Command::new(cmd)
        .status()
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Failed to execute {}: {}", cmd, e)))?;

    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("{} command failed: {:?}", cmd, status.code()),
        ));
    }

    Ok(())
}
