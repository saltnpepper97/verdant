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

pub fn run_ipc_server(
    service_manager: Arc<Mutex<ServiceManager>>,
    shutdown_flag: Arc<AtomicBool>,
) -> std::io::Result<()> {
    use std::time::Duration;

    let (console_logger, file_logger) = {
        let sm = service_manager.lock().unwrap();
        (sm.get_console_logger(), sm.get_file_logger())
    };

    let socket_path = Path::new(VERDANTD_SOCKET_PATH);

    // ✅ Ensure parent directory exists
    if let Some(parent) = socket_path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }

    // ✅ Remove any stale socket file
    if socket_path.exists() {
        fs::remove_file(socket_path)?;
    }

    let listener = UnixListener::bind(socket_path)?;

    // ✅ Log server start
    {
        let msg = format!("Verdantd IPC server listening on {}", VERDANTD_SOCKET_PATH);
        if let Ok(mut con) = console_logger.lock() {
            con.message(LogLevel::Info, &msg, Duration::ZERO);
        }
        if let Ok(mut file) = file_logger.lock() {
            file.log(LogLevel::Info, &msg);
        }
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
            let _ = stream.write_all(&data); // best effort
            return Ok(());
        }
    };

    match request.command {
        IpcCommand::Shutdown | IpcCommand::Reboot => {
            // ✅ Step 1: Acknowledge the vctl request first
            let ack = IpcResponse {
                success: true,
                message: format!("{:?} initiated", request.command),
                data: None,
            };
            let _ = stream.write_all(&serialize_response(&ack));
            let _ = stream.flush();

            // ✅ Step 2: Spawn shutdown coordination in a new thread
            let sm = Arc::clone(&service_manager);
            thread::spawn(move || {
                let mut sm = match sm.lock() {
                    Ok(sm) => sm,
                    Err(_) => return,
                };

                // ✅ Step 3: Shutdown local services
                if let Err(e) = sm.shutdown() {
                    let msg = format!("Failed to shutdown services: {}", e);
                    if let Ok(mut con) = sm.get_console_logger().lock() {
                        con.message(LogLevel::Fail, &msg, Duration::ZERO);
                    }
                    if let Ok(mut file) = sm.get_file_logger().lock() {
                        file.log(LogLevel::Fail, &msg);
                    }
                }

                // ✅ Step 4: Send command to init
                let init_request = IpcRequest {
                    target: IpcTarget::Init,
                    command: request.command.clone(),
                };

                let success = match send_ipc_request(INIT_SOCKET_PATH, &init_request) {
                    Ok(response) if response.success => {
                        let msg = format!("Sent {:?} to init: {}", request.command, response.message);
                        if let Ok(mut con) = sm.get_console_logger().lock() {
                            con.message(LogLevel::Info, &msg, Duration::ZERO);
                        }
                        if let Ok(mut file) = sm.get_file_logger().lock() {
                            file.log(LogLevel::Info, &msg);
                        }
                        true
                    }
                    Ok(response) => {
                        let msg = format!("Init rejected {:?}: {}", request.command, response.message);
                        if let Ok(mut con) = sm.get_console_logger().lock() {
                            con.message(LogLevel::Warn, &msg, Duration::ZERO);
                        }
                        if let Ok(mut file) = sm.get_file_logger().lock() {
                            file.log(LogLevel::Warn, &msg);
                        }
                        false
                    }
                    Err(e) => {
                        let msg = format!("Failed to send command to init: {}", e);
                        if let Ok(mut con) = sm.get_console_logger().lock() {
                            con.message(LogLevel::Warn, &msg, Duration::ZERO);
                        }
                        if let Ok(mut file) = sm.get_file_logger().lock() {
                            file.log(LogLevel::Warn, &msg);
                        }
                        false
                    }
                };

                // ✅ Step 5: Only quit verdantd if init acknowledged
                if success {
                    shutdown_flag.store(true, Ordering::SeqCst);
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

