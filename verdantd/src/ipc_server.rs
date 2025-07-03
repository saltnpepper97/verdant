use std::fs;
use std::sync::{Arc, Mutex, atomic::AtomicBool};
use std::os::unix::net::{UnixListener, UnixStream};
use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::atomic::Ordering;

use crate::manager::ServiceManager;

use bloom::ipc::{send_ipc_request, IpcRequest, IpcTarget, IpcCommand, IpcResponse, serialize_response, VERDANTD_SOCKET_PATH, INIT_SOCKET_PATH};
use bloom::status::LogLevel;
use serde_json;
use std::sync::mpsc;

pub fn run_ipc_server(
    service_manager: Arc<Mutex<ServiceManager>>,
    shutdown_flag: Arc<AtomicBool>,
    ready_tx: Option<mpsc::Sender<()>>,
) -> std::io::Result<()> {
    use std::time::Duration;

    let (console_logger, file_logger) = {
        let sm = service_manager.lock().unwrap();
        (sm.get_console_logger(), sm.get_file_logger())
    };

    let socket_path = Path::new(VERDANTD_SOCKET_PATH);

    if let Some(parent) = socket_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    if socket_path.exists() {
        fs::remove_file(socket_path)?;
    }

    let listener = UnixListener::bind(socket_path)?;

    {
        let msg = format!("Verdantd IPC server listening on {}", VERDANTD_SOCKET_PATH);
        if let Ok(mut con) = console_logger.lock() {
            con.message(LogLevel::Info, &msg, Duration::ZERO);
        }
        if let Ok(mut file) = file_logger.lock() {
            file.log(LogLevel::Info, &msg);
        }
    }

    // Send ready signal if sender provided
    if let Some(tx) = ready_tx {
        let _ = tx.send(());
    }

    for stream_result in listener.incoming() {
        match stream_result {
            Ok(mut stream) => {
                let sm = Arc::clone(&service_manager);
                if let Err(e) = handle_client(&mut stream, sm, Arc::clone(&shutdown_flag)) {
                    let (console_logger, file_logger) = {
                        let sm = service_manager.lock().unwrap();
                        (sm.get_console_logger(), sm.get_file_logger())
                    };
                    let msg = format!("Error handling IPC client: {}", e);
                    if let Ok(mut con) = console_logger.lock() {
                        con.message(LogLevel::Fail, &msg, Duration::ZERO);
                    }
                    if let Ok(mut file) = file_logger.lock() {
                        file.log(LogLevel::Fail, &msg);
                    }
                }
            }
            Err(e) => {
                let (console_logger, file_logger) = {
                    let sm = service_manager.lock().unwrap();
                    (sm.get_console_logger(), sm.get_file_logger())
                };
                let msg = format!("Failed to accept IPC connection: {}", e);
                if let Ok(mut con) = console_logger.lock() {
                    con.message(LogLevel::Fail, &msg, Duration::ZERO);
                }
                if let Ok(mut file) = file_logger.lock() {
                    file.log(LogLevel::Fail, &msg);
                }
            }
        }
    }

    Ok(())
}

fn handle_client(
    stream: &mut UnixStream,
    service_manager: Arc<Mutex<ServiceManager>>,
    shutdown_flag: Arc<AtomicBool>,
) -> std::io::Result<()> {
    use std::io::BufReader;
    use std::thread;

    let mut buf = Vec::new();
    let mut reader = BufReader::new(stream.try_clone()?);
    reader.read_until(b'\n', &mut buf)?;

    let request = match serde_json::from_slice::<IpcRequest>(&buf) {
        Ok(req) => req,
        Err(_) => {
            let resp = IpcResponse {
                success: false,
                message: "Invalid IPC request".into(),
                data: None,
            };
            let data = serialize_response(&resp);
            let _ = stream.write_all(&data);
            return Ok(());
        }
    };

    match request.command {
        IpcCommand::Shutdown | IpcCommand::Reboot => {
            // Step 1: Acknowledge request immediately
            let ack = IpcResponse {
                success: true,
                message: format!("{:?} initiated", request.command),
                data: None,
            };
            let _ = stream.write_all(&serialize_response(&ack));
            let _ = stream.flush();

            // Step 2: Spawn thread to coordinate shutdown
            let sm = Arc::clone(&service_manager);
            let shutdown_flag_clone = Arc::clone(&shutdown_flag);
            thread::spawn(move || {
                // Step 3: Set shutdown flag BEFORE shutting down services
                shutdown_flag_clone.store(true, Ordering::SeqCst);

                // NOTE: Do NOT call sm_guard.shutdown() here!
                // The main thread watches the shutdown_flag and will call shutdown() once.

                let sm_guard = match sm.lock() {
                    Ok(sm) => sm,
                    Err(_) => return,
                };

                // Step 4: Send shutdown/reboot command to init
                let init_request = IpcRequest {
                    target: IpcTarget::Init,
                    command: request.command.clone(),
                };

                match send_ipc_request(INIT_SOCKET_PATH, &init_request) {
                    Ok(response) if response.success => {
                        let msg = format!("Sent {:?} to init: {}", request.command, response.message);
                        if let Ok(mut con) = sm_guard.get_console_logger().lock() {
                            con.message(LogLevel::Info, &msg, std::time::Duration::ZERO);
                        }
                        if let Ok(mut file) = sm_guard.get_file_logger().lock() {
                            file.log(LogLevel::Info, &msg);
                        }
                    }
                    Ok(response) => {
                        let msg = format!("Init rejected {:?}: {}", request.command, response.message);
                        if let Ok(mut con) = sm_guard.get_console_logger().lock() {
                            con.message(LogLevel::Warn, &msg, std::time::Duration::ZERO);
                        }
                        if let Ok(mut file) = sm_guard.get_file_logger().lock() {
                            file.log(LogLevel::Warn, &msg);
                        }
                    }
                    Err(e) => {
                        let msg = format!("Failed to send command to init: {}", e);
                        if let Ok(mut con) = sm_guard.get_console_logger().lock() {
                            con.message(LogLevel::Warn, &msg, std::time::Duration::ZERO);
                        }
                        if let Ok(mut file) = sm_guard.get_file_logger().lock() {
                            file.log(LogLevel::Warn, &msg);
                        }
                    }
                }
            });

            Ok(())
        }
        _ => {
            let resp = IpcResponse {
                success: false,
                message: "Unsupported command".into(),
                data: None,
            };
            stream.write_all(&serialize_response(&resp))?;
            Ok(())
        }
    }
}

