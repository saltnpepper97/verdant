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
    use std::time::Duration;

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
            let cmd = request.command.clone();

            thread::spawn(move || {
                shutdown_flag_clone.store(true, Ordering::SeqCst);

                let mut sm_guard = match sm.lock() {
                    Ok(sm) => sm,
                    Err(_) => return,
                };

                let console_logger = sm_guard.get_console_logger();
                let file_logger = sm_guard.get_file_logger();

                let shutdown_result = sm_guard.shutdown();

                match &shutdown_result {
                    Ok(_) => {
                        if let Ok(mut file) = file_logger.lock() {
                            file.log(LogLevel::Info, "All services stopped");
                        }
                    }
                    Err(e) => {
                        let msg = format!("Failed to shutdown services: {}", e);
                        if let Ok(mut con) = console_logger.lock() {
                            con.message(LogLevel::Fail, &msg, Duration::ZERO);
                        }
                        if let Ok(mut file) = file_logger.lock() {
                            file.log(LogLevel::Fail, &msg);
                        }
                    }
                }

                // Step 3: Send shutdown/reboot command to init
                let init_request = IpcRequest {
                    target: IpcTarget::Init,
                    command: cmd,
                };

                match send_ipc_request(INIT_SOCKET_PATH, &init_request) {
                    Ok(response) if response.success => {
                        if let Ok(mut con) = sm.lock().unwrap().get_console_logger().lock() {
                            con.message(LogLevel::Info, &format!("Sent shutdown command to init: {}", response.message), Duration::ZERO);
                        }
                        if let Ok(mut file) = sm.lock().unwrap().get_file_logger().lock() {
                            file.log(LogLevel::Info, &format!("Sent shutdown command to init: {}", response.message));
                        }
                    }
                    Ok(response) => {
                        if let Ok(mut con) = sm.lock().unwrap().get_console_logger().lock() {
                            con.message(LogLevel::Warn, &format!("Init rejected shutdown command: {}", response.message), Duration::ZERO);
                        }
                        if let Ok(mut file) = sm.lock().unwrap().get_file_logger().lock() {
                            file.log(LogLevel::Warn, &format!("Init rejected shutdown command: {}", response.message));
                        }
                    }
                    Err(e) => {
                        if let Ok(mut con) = sm.lock().unwrap().get_console_logger().lock() {
                            con.message(LogLevel::Warn, &format!("Failed to send shutdown command to init: {}", e), Duration::ZERO);
                        }
                        if let Ok(mut file) = sm.lock().unwrap().get_file_logger().lock() {
                            file.log(LogLevel::Warn, &format!("Failed to send shutdown command to init: {}", e));
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

