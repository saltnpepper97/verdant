use std::{
    fs::{self, OpenOptions},
    process::{Command, Stdio},
    sync::{Arc, atomic::{AtomicBool, Ordering}, Mutex},
    thread,
    time::Duration,
};

use anyhow::Result;
use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;

use common::utils::*;
use crate::supervisor::Supervisor;
use crate::service::{Service, ServiceConfig, LogMode};

const LOG_DIR: &str = "/var/log/verdant";

pub fn spawn_service(supervisor: &Supervisor, name: &str, config: &ServiceConfig) -> Result<()> {
    fs::create_dir_all(LOG_DIR).ok();

    // Prepare stdout/stderr based on log mode
    let (stdout, stderr) = match config.log {
        LogMode::File => {
            let log_path = format!("{}/{}.log", LOG_DIR, name);
            let log_file = OpenOptions::new()
                .create(true)
                .write(true)
                .append(true)
                .open(&log_path)?;
            (Stdio::from(log_file.try_clone()?), Stdio::from(log_file))
        }
        LogMode::Null => {
            let devnull = OpenOptions::new()
                .write(true)
                .open("/dev/null")?;
            (Stdio::from(devnull.try_clone()?), Stdio::from(devnull))
        }
    };

    let mut cmd = Command::new(&config.exec);
    cmd.args(&config.args)
        .stdout(stdout)
        .stderr(stderr)
        .stdin(Stdio::null());

    for (k, v) in &config.env {
        cmd.env(k, v);
    }

    let child = cmd.spawn()?;

    let mut running = supervisor.running_lock();
    running.insert(
        name.to_string(),
        Service {
            config: config.clone(),
            child: Some(child),
            running: true,
        },
    );

    print_boot_info(&format!("Started service '{}': exec='{}'", name, config.exec));
    Ok(())
}

/// Periodically checks service states and restarts as needed.
/// Releases the mutex every loop iteration to allow IPC access.
pub fn run_loop(supervisor: &Arc<Mutex<Supervisor>>) -> Result<()> {
    let mut signals = Signals::new(&[SIGINT, SIGTERM])?;
    let terminate_flag = Arc::new(AtomicBool::new(false));
    let term_thread = Arc::clone(&terminate_flag);

    // Signal handling thread
    thread::spawn(move || {
        for signal in signals.forever() {
            if signal == SIGINT || signal == SIGTERM {
                print_boot_info(&format!("Received signal {}, shutting down...", signal));
                term_thread.store(true, Ordering::SeqCst);
                break;
            }
        }
    });

    loop {
        thread::sleep(Duration::from_millis(500));

        if terminate_flag.load(Ordering::SeqCst) {
            println!("{} Termination signal received, cleaning up...", tag_shutdown());
            break;
        }

        // Step 1: Check for stopped services and collect names to restart
        let to_restart = {
            let sup = supervisor.lock().unwrap();
            let mut running_map = sup.running_lock();
            let mut restart_list = Vec::new();

            for (name, svc) in running_map.iter_mut() {
                if let Some(child) = &mut svc.child {
                    match child.try_wait()? {
                        Some(status) => {
                            svc.running = false;
                            print_boot_info(&format!("Service '{}' exited with status: {}", name, status));
                            svc.child = None;

                            if svc.config.restart {
                                restart_list.push(name.clone());
                            }
                        }
                        None => {}
                    }
                }
            }

            restart_list
        };

        // Step 2: Restart services that exited (outside lock)
        for name in to_restart {
            print_boot_info(&format!("Restarting service '{}'", name));
            let _ = supervisor.lock().unwrap().start_service(&name);
        }
    }

    // Cleanup on shutdown
    {
        let sup = supervisor.lock().unwrap();
        let mut running_map = sup.running_lock();

        for (name, svc) in running_map.iter_mut() {
            if let Some(child) = &mut svc.child {
                print_boot_info(&format!("Killing service '{}'", name));
                let _ = child.kill();
                let _ = child.wait();
                svc.running = false;
                svc.child = None;
            }
        }
    }

    print_boot_info("Supervisor exiting cleanly.");
    Ok(())
}

