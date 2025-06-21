use std::os::unix::net::{UnixListener, UnixStream};
use std::io::{BufReader, BufRead, Write};
use std::sync::mpsc::Sender;
use std::thread;
use std::sync::{Arc, Mutex};
use std::panic;

use common::{print_step, status_fail, status_ok};
use ipc_protocol::{Request, Response, SOCKET_PATH};
use serde_json;

use crate::runtime::service_manager::ServiceManager;
use crate::runtime::SystemAction;

pub fn start_ipc_listener(tx: Sender<SystemAction>, svc_manager: Arc<Mutex<ServiceManager>>) -> std::io::Result<()> {
    let _ = std::fs::remove_file(SOCKET_PATH);

    let listener = UnixListener::bind(SOCKET_PATH)?;
    print_step(&format!("IPC listener running on {}", SOCKET_PATH), &status_ok());

    thread::spawn(move || {
        let result = panic::catch_unwind(|| {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let svc_manager_clone = Arc::clone(&svc_manager);
                        let tx_clone = tx.clone();

                        if let Err(e) = handle_client(stream, &tx_clone, svc_manager_clone) {
                            print_step(&format!("IPC client handler error: {}", e), &status_fail());
                        }
                    }
                    Err(e) => {
                        print_step(&format!("IPC listener error: {}", e), &status_fail());
                    }
                }
            }
        });

        if let Err(e) = result {
            eprintln!("IPC thread crashed: {:?}", e);
        }
    });

    Ok(())
}

fn handle_client(mut stream: UnixStream, tx: &Sender<SystemAction>, svc_manager: Arc<Mutex<ServiceManager>>) -> std::io::Result<()> {
    let mut reader = BufReader::new(&stream);
    let mut buf = String::new();
    reader.read_line(&mut buf)?;

    // Removed: println!("[IPC] Received raw: {}", buf.trim());

    let request: Result<Request, _> = serde_json::from_str(buf.trim());

    let response = match request {
        Ok(req) => {
            // Removed: println!("[IPC] Parsed request: {:?}", req);
            let mut svc_manager_guard = match svc_manager.lock() {
                Ok(guard) => guard,
                Err(poisoned) => {
                    // Removed: eprintln!("[IPC] Mutex poisoned. Recovering.");
                    poisoned.into_inner()
                }
            };
            handle_request(req, tx, &mut *svc_manager_guard)
        }
        Err(e) => {
            // Removed: println!("[IPC] Failed to parse request: {}", e);
            Response::Error {
                message: format!("Failed to parse request: {}", e),
            }
        }
    };

    let resp_text = serde_json::to_string(&response).unwrap_or_else(|_| "{\"Error\":\"Serialization failed\"}".into());
    stream.write_all(resp_text.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

fn handle_request(req: Request, tx: &Sender<SystemAction>, svc_manager: &mut ServiceManager) -> Response {
    // Removed: println!("[IPC] Handling request: {:?}", req);

    match req {
        Request::Start { name } => svc_manager.start_service(&name),
        Request::Stop { name } => svc_manager.stop_service(&name),
        Request::Enable { name } => svc_manager.enable_service(&name),
        Request::Disable { name } => svc_manager.disable_service(&name),
        Request::Reboot => {
            if tx.send(SystemAction::Reboot).is_ok() {
                Response::Success {
                    message: "System reboot initiated successfully.".into(),
                }
            } else {
                Response::Error {
                    message: "Failed to send reboot command.".into(),
                }
            }
        }
        Request::Shutdown => {
            if tx.send(SystemAction::Shutdown).is_ok() {
                Response::Success {
                    message: "System shutdown initiated successfully.".into(),
                }
            } else {
                Response::Error {
                    message: "Failed to send shutdown command.".into(),
                }
            }
        }
        _ => Response::Error {
            message: "Command not implemented yet.".into(),
        },
    }
}

