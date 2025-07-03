use std::fs;
use std::sync::{Arc, Mutex, atomic::AtomicBool};
use std::os::unix::net::{UnixListener, UnixStream};
use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;
use std::thread;

use crate::manager::ServiceManager;

use bloom::ipc::{
    send_ipc_request, IpcRequest, IpcTarget, IpcCommand, IpcResponse,
    serialize_response, VERDANTD_SOCKET_PATH, INIT_SOCKET_PATH,
};
use bloom::status::LogLevel;
use serde_json;

pub fn run_ipc_server(
    service_manager: Arc<Mutex<ServiceManager>>,
    shutdown_flag: Arc<AtomicBool>,
    ready_tx: Option<mpsc::Sender<()>>,
) -> std::io::Result<()> {
    let (console_logger, file_logger) = {
        let sm = service_manager.lock().unwrap();
        (sm.get_console_logger().clone(), sm.get_file_logger().clone())
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

    if let Some(tx) = ready_tx {
        let _ = tx.send(());
    }

    for stream_result in listener.incoming() {
        match stream_result {
            Ok(mut stream) => {
                let sm_clone = Arc::clone(&service_manager);
                let flag_clone = Arc::clone(&shutdown_flag);
                if let Err(e) = handle_client(&mut stream, sm_clone, flag_clone) {
                    let (console_logger, file_logger) = {
                        let sm = service_manager.lock().unwrap();
                        (sm.get_console_logger().clone(), sm.get_file_logger().clone())
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
                    (sm.get_console_logger().clone(), sm.get_file_logger().clone())
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
            let ack = IpcResponse {
                success: true,
                message: format!("{:?} initiated", request.command),
                data: None,
            };
            let _ = stream.write_all(&serialize_response(&ack));
            let _ = stream.flush();

            let sm = Arc::clone(&service_manager);
            let shutdown_flag_clone = Arc::clone(&shutdown_flag);
            let cmd = request.command.clone();

            thread::spawn(move || {
                let mut sm_guard = match sm.lock() {
                    Ok(sm) => sm,
                    Err(_) => return,
                };

                let console_logger = sm_guard.get_console_logger().clone();
                let file_logger = sm_guard.get_file_logger().clone();

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

                drop(sm_guard); // explicitly release the lock before sending IPC

                let init_request = IpcRequest {
                    target: IpcTarget::Init,
                    command: cmd,
                };

                match send_ipc_request(INIT_SOCKET_PATH, &init_request) {
                    Ok(response) if response.success => {
                        let msg = format!("Sent shutdown command to init: {}", response.message);
                        if let Ok(mut con) = console_logger.lock() {
                            con.message(LogLevel::Info, &msg, Duration::ZERO);
                        }
                        if let Ok(mut file) = file_logger.lock() {
                            file.log(LogLevel::Info, &msg);
                        }
                    }
                    Ok(response) => {
                        let msg = format!("Init rejected shutdown command: {}", response.message);
                        if let Ok(mut con) = console_logger.lock() {
                            con.message(LogLevel::Warn, &msg, Duration::ZERO);
                        }
                        if let Ok(mut file) = file_logger.lock() {
                            file.log(LogLevel::Warn, &msg);
                        }
                    }
                    Err(e) => {
                        let msg = format!("Failed to send shutdown command to init: {}", e);
                        if let Ok(mut con) = console_logger.lock() {
                            con.message(LogLevel::Warn, &msg, Duration::ZERO);
                        }
                        if let Ok(mut file) = file_logger.lock() {
                            file.log(LogLevel::Warn, &msg);
                        }
                    }
                }

                // Only after services are shutdown and IPC sent do we signal the main loop to exit
                shutdown_flag_clone.store(true, Ordering::SeqCst);
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

