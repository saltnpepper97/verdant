use std::fs;
use std::path::Path;
use std::os::unix::net::{UnixListener, UnixStream};
use std::io::{BufRead, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;

use bloom::ipc::{IpcRequest, IpcResponse, IpcCommand, serialize_response, INIT_SOCKET_PATH};
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;
use serde_json;

pub fn run_ipc_server(
    shutdown_flag: Arc<AtomicBool>,
    reboot_flag: Arc<AtomicBool>,
    console_logger: Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
    file_logger: Arc<Mutex<dyn FileLogger + Send + Sync>>,
    main_thread: std::thread::Thread,
) -> std::io::Result<()> {
    if Path::new(INIT_SOCKET_PATH).exists() {
        fs::remove_file(INIT_SOCKET_PATH)?;
    }

    let listener = UnixListener::bind(INIT_SOCKET_PATH)?;

    log_message(&console_logger, &file_logger, LogLevel::Info, &format!(
        "Init IPC server listening on {}",
        INIT_SOCKET_PATH
    ));

    for stream_result in listener.incoming() {
        match stream_result {
            Ok(mut stream) => {
                let shutdown_flag = Arc::clone(&shutdown_flag);
                let reboot_flag = Arc::clone(&reboot_flag);
                let console_logger = Arc::clone(&console_logger);
                let file_logger = Arc::clone(&file_logger);
                let main_thread = main_thread.clone();

                if let Err(e) = handle_client(
                    &mut stream,
                    shutdown_flag,
                    reboot_flag,
                    console_logger,
                    file_logger,
                    main_thread,
                ) {
                    eprintln!("Error handling IPC client: {}", e);
                }
            }
            Err(e) => eprintln!("Failed to accept IPC connection: {}", e),
        }
    }

    Ok(())
}

fn handle_client(
    stream: &mut UnixStream,
    shutdown_flag: Arc<AtomicBool>,
    reboot_flag: Arc<AtomicBool>,
    console_logger: Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
    file_logger: Arc<Mutex<dyn FileLogger + Send + Sync>>,
    main_thread: std::thread::Thread,
) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut reader = std::io::BufReader::new(stream.try_clone()?);
    reader.read_until(b'\n', &mut buf)?;

    let request = match serde_json::from_slice::<IpcRequest>(&buf) {
        Ok(req) => req,
        Err(_) => {
            let resp = IpcResponse {
                success: false,
                message: "Invalid IPC request".into(),
                data: None,
            };
            let _ = stream.write_all(&serialize_response(&resp));
            return Ok(());
        }
    };

    match request.command {
        IpcCommand::Shutdown => {
            // Respond immediately
            let resp = IpcResponse {
                success: true,
                message: "Shutdown scheduled".into(),
                data: None,
            };
            stream.write_all(&serialize_response(&resp))?;

            // Delay flag set/unpark to avoid blocking client
            let shutdown_flag_clone = Arc::clone(&shutdown_flag);
            thread::spawn(move || {
                shutdown_flag_clone.store(true, Ordering::SeqCst);
                main_thread.unpark();
            });
        }
        IpcCommand::Reboot => {
            // Respond immediately
            let resp = IpcResponse {
                success: true,
                message: "Reboot scheduled".into(),
                data: None,
            };
            stream.write_all(&serialize_response(&resp))?;

            // Delay flag set/unpark to avoid blocking client
            let reboot_flag_clone = Arc::clone(&reboot_flag);
            main_thread.unpark();
            thread::spawn(move || {
                reboot_flag_clone.store(true, Ordering::SeqCst);
            });
        }
        _ => {
            let resp = IpcResponse {
                success: false,
                message: "Unsupported command for init".into(),
                data: None,
            };
            stream.write_all(&serialize_response(&resp))?;
            log_message(&console_logger, &file_logger, LogLevel::Fail, "Unsupported command for init");
        }
    }

    Ok(())
}

fn log_message(
    console_logger: &Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
    file_logger: &Arc<Mutex<dyn FileLogger + Send + Sync>>,
    level: LogLevel,
    msg: &str,
) {
    if let Ok(mut con) = console_logger.lock() {
        con.message(level, msg, std::time::Duration::ZERO);
    }
    if let Ok(mut file) = file_logger.lock() {
        file.log(level, msg);
    }
}

