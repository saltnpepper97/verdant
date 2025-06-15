mod ipc;
mod supervisor;
mod service;
mod service_runner;
mod module_loader;

use std::{path::Path, sync::{Arc, Mutex}};
use nix::sys::reboot::{reboot, RebootMode};
use supervisor::Supervisor;
use common::utils::*;
use ipc_protocol::{Request, Response};

#[tokio::main]
async fn main() {
    // Load kernel modules
    module_loader::load_all_modules();

    // Create shared Supervisor inside Mutex + Arc
    let supervisor = Arc::new(Mutex::new(Supervisor::default()));

    {
        let mut sup = supervisor.lock().unwrap();
        if let Err(e) = sup.load_all_services() {
            print_error(&format!("Failed to load services: {}", e));
            std::process::exit(1);
        }

        detect_device_manager(&mut *sup);

        if let Err(e) = sup.start_enabled_services() {
            print_error(&format!("Failed to start enabled services: {}", e));
            std::process::exit(1);
        }
    }

    // Start IPC server task
    let ipc_supervisor = Arc::clone(&supervisor);
    tokio::spawn(async move {
        let handler: Arc<dyn Fn(Request) -> Response + Send + Sync> = Arc::new(move |req: Request| -> Response {
            let mut sup = ipc_supervisor.lock().unwrap();
            handle_request(req, &mut *sup)
        });

        if let Err(e) = ipc::run_ipc_server(handler).await {
            eprintln!("IPC server failed: {}", e);
        }
    });

    // Run blocking supervisor run_loop on separate thread, locking supervisor once
    let supervisor_clone = Arc::clone(&supervisor);
    
    let handle = std::thread::spawn(move || {
        if let Err(e) = service_runner::run_loop(&supervisor_clone) {
            eprintln!("Supervisor run_loop exited with error: {}", e);
        }
    });


    // Wait for the supervisor thread to finish (if ever)
    if let Err(e) = handle.join() {
        print_error(&format!("Supervisor thread panicked: {:?}", e));
        std::process::exit(1);
    }
}

fn detect_device_manager(supervisor: &mut Supervisor) {
    if Path::new("/usr/lib/systemd/systemd-udevd").exists()
        || Path::new("/usr/bin/udevadm").exists()
    {
        print_info("Detected systemd-udevd, starting udev...");
        if let Err(e) = supervisor.start_service("udev") {
            print_error(&format!("Failed to start 'udev' service: {}", e));
        }
    } else if Path::new("/sbin/mdev").exists() || Path::new("/bin/mdev").exists() {
        print_info("Detected busybox mdev, started 'mdev' service.");
        if let Err(e) = supervisor.start_service("mdev") {
            print_error(&format!("Failed to start 'mdev' service: {}", e));
        }
    } else {
        print_info("No known device manager detected (udev or mdev).");
    }
}

fn handle_request(req: Request, supervisor: &mut Supervisor) -> Response {
    match req {
        Request::StartService { name } => {
            supervisor.start_service(&name).map(|_| Response::Ok)
                .unwrap_or_else(|e| Response::Error(e.to_string()))
        }
        Request::StopService { name } => {
            supervisor.stop_service(&name).map(|_| Response::Ok)
                .unwrap_or_else(|e| Response::Error(e.to_string()))
        }
        Request::RestartService { name } => {
            if let Err(e) = supervisor.stop_service(&name) {
                return Response::Error(format!("Failed to stop '{}': {}", name, e));
            }
            match supervisor.start_service(&name) {
                Ok(_) => Response::Ok,
                Err(e) => Response::Error(format!("Failed to start '{}': {}", name, e)),
            }
        }
        Request::Shutdown => {
            if let Err(e) = supervisor.stop_all_services() {
                Response::Error(format!("Failed to stop services: {}", e))
            } else {
                Response::Ok
            }
        }
        
        Request::Reboot => {
            if let Err(e) = supervisor.stop_all_services() {
                Response::Error(format!("Failed to stop services: {}", e))
            } else if let Err(e) = do_reboot() {
                Response::Error(format!("Failed to reboot: {}", e))
            } else {
                Response::Ok
            }
        }

        Request::EnableModule { name } => Response::Ok,
        Request::DisableModule { name } => Response::Ok,
        Request::Status => Response::StatusInfo("Supervisor is running".to_string()),
        _ => Response::Error("Unsupported request".to_string()),
    }
}

fn do_reboot() -> anyhow::Result<()> {
    reboot(RebootMode::RB_AUTOBOOT)?;
    Ok(())
}
