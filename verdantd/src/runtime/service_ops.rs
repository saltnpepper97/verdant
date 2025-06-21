use std::fs;
use std::path::Path;
use ipc_protocol::Response;
use crate::loader;
use crate::managed_service::ManagedService;
use crate::runtime::dependency::resolve_services;
use common::{print_step, status_ok};

pub fn start_service(services: &mut Vec<ManagedService>, name: &str) -> Response {
    let enabled_dir = Path::new("/etc/verdant/enabled");
    let symlink_path = enabled_dir.join(format!("{}.vs", name));
    if !symlink_path.exists() {
        return Response::Error {
            message: format!("Service '{}' is not enabled.", name),
        };
    }

    if let Some(svc) = services.iter_mut().find(|s| s.config.name == name) {
        if let Some(child) = svc.child.as_mut() {
            if child.try_wait().ok().flatten().is_none() {
                return Response::Error {
                    message: format!("Service '{}' is already running.", name),
                };
            }
        }
        match svc.launch() {
            Ok(pid) => {
                let msg = format!("Launched service {} (PID {})", name, pid);
                print_step(&msg, &status_ok());

                Response::Success {
                    message: format!("Service '{}' started (PID {}).", name, pid),
                }
            }
            Err(e) => Response::Error {
                message: format!("Failed to start service '{}': {}", name, e),
            },
        }
    } else {
        let configs = loader::load_enabled_services_quiet();
        let resolved = resolve_services(configs);
        for mut svc in resolved {
            if svc.config.name == name {
                match svc.launch() {
                    Ok(pid) => {
                        services.push(svc);
                        let msg = format!("Launched service {} (PID {})", name, pid);
                        print_step(&msg, &status_ok());

                        return Response::Success {
                            message: format!("Service '{}' started (PID {}).", name, pid),
                        };
                    }
                    Err(e) => {
                        return Response::Error {
                            message: format!("Failed to start service '{}': {}", name, e),
                        };
                    }
                }
            }
        }
        Response::Error {
            message: format!("Service '{}' not found.", name),
        }
    }
}

pub fn stop_service(services: &mut Vec<ManagedService>, name: &str) -> Response {
    if let Some(pos) = services.iter().position(|s| s.config.name == name) {
        let mut svc = services.remove(pos);
        if let Some(child) = svc.child.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return Response::Error {
                        message: format!("Service '{}' already exited: {:?}", name, status),
                    };
                }
                Ok(None) => {
                    if let Err(e) = child.kill() {
                        return Response::Error {
                            message: format!("Failed to kill service '{}': {}", name, e),
                        };
                    }
                    let _ = child.wait();
                    let msg = format!("Stopped service {}", name);
                    print_step(&msg, &status_ok());

                    return Response::Success {
                        message: format!("Service '{}' stopped.", name),
                    };
                }
                Err(e) => {
                    return Response::Error {
                        message: format!("Failed to inspect service '{}': {}", name, e),
                    };
                }
            }
        } else {
            return Response::Error {
                message: format!("Service '{}' is not running.", name),
            };
        }
    }
    Response::Error {
        message: format!("Service '{}' not found or not running.", name),
    }
}

pub fn enable_service(_services: &mut Vec<ManagedService>, name: &str) -> Response {
    let services_dir = Path::new("/etc/verdant/services");
    let enabled_dir = Path::new("/etc/verdant/enabled");

    let service_file = services_dir.join(format!("{}.vs", name));
    if !service_file.exists() {
        return Response::Error {
            message: format!("Service '{}' does not exist in {}", name, services_dir.display()),
        };
    }

    let symlink_path = enabled_dir.join(format!("{}.vs", name));
    if symlink_path.exists() {
        return Response::Error {
            message: format!("Service '{}' is already enabled.", name),
        };
    }

    if let Err(e) = std::os::unix::fs::symlink(&service_file, &symlink_path) {
        return Response::Error {
            message: format!("Failed to enable service '{}': {}", name, e),
        };
    }

    let msg = format!("Enabled service {}", name);
    print_step(&msg, &status_ok());

    Response::Success {
        message: format!("Service '{}' enabled.", name),
    }
}

pub fn disable_service(services: &mut Vec<ManagedService>, name: &str) -> Response {
    let enabled_dir = Path::new("/etc/verdant/enabled");
    let symlink_path = enabled_dir.join(format!("{}.vs", name));

    if !symlink_path.exists() {
        return Response::Error {
            message: format!("Service '{}' is not enabled.", name),
        };
    }

    if let Some(pos) = services.iter().position(|svc| svc.config.name == name) {
        let mut svc = services.remove(pos);
        if let Some(child) = svc.child.as_mut() {
            match child.try_wait() {
                Ok(Some(_)) => {}
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                Err(_) => {}
            }
        }
    }

    if let Err(e) = fs::remove_file(&symlink_path) {
        return Response::Error {
            message: format!("Failed to remove symlink for '{}': {}", name, e),
        };
    }

    let configs = loader::load_enabled_services_quiet();
    let resolved = resolve_services(configs);

    let mut still_running = std::collections::HashMap::new();
    for mut svc in services.drain(..) {
        if let Some(child) = svc.child.as_mut() {
            if child.try_wait().ok().flatten().is_none() {
                still_running.insert(svc.config.name.clone(), svc);
            }
        }
    }

    let mut new_services = Vec::new();
    for mut svc in resolved {
        let svc_name = svc.config.name.clone();
        if let Some(existing_svc) = still_running.remove(&svc_name) {
            new_services.push(existing_svc);
        } else {
            if let Err(e) = svc.launch() {
                return Response::Error {
                    message: format!("Failed to launch '{}': {}", svc_name, e),
                };
            }
            new_services.push(svc);
        }
    }

    *services = new_services;

    let msg = format!("Stopped and disabled service {}", name);
    print_step(&msg, &status_ok());

    Response::Success {
        message: format!("Service '{}' disabled and stopped.", name),
    }
}

