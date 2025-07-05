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
    boot_complete_flag: Arc<AtomicBool>,
    console_logger: Arc<Mutex<dyn ConsoleLogger + Send + Sync>>,
    file_logger: Arc<Mutex<dyn FileLogger + Send + Sync>>,
    main_thread: std::thread::Thread,
) -> std::io::Result<()> {
    if Path::new(INIT_SOCKET_PATH).exists() {
        fs::remove_file(INIT_SOCKET_PATH)?;
    }

    let listener = UnixListener::bind(INIT_SOCKET_PATH)?;

    for stream_result in listener.incoming() {
        match stream_result {
            Ok(mut stream) => {
                let shutdown_flag = Arc::clone(&shutdown_flag);
                let reboot_flag = Arc::clone(&reboot_flag);
                let boot_complete_flag = Arc::clone(&boot_complete_flag);
                let console_logger = Arc::clone(&console_logger);
                let file_logger = Arc::clone(&file_logger);
                let main_thread = main_thread.clone();

                if let Err(e) = handle_client(
                    &mut stream,
                    shutdown_flag,
                    reboot_flag,
                    boot_complete_flag,
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
    boot_complete_flag: Arc<AtomicBool>,
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
            let resp = IpcResponse {
                success: true,
                message: "Shutdown scheduled".into(),
                data: None,
            };
            stream.write_all(&serialize_response(&resp))?;

            thread::spawn(move || {
                shutdown_flag.store(true, Ordering::SeqCst);
                main_thread.unpark();
            });
        }

        IpcCommand::Reboot => {
            let resp = IpcResponse {
                success: true,
                message: "Reboot scheduled".into(),
                data: None,
            };
            stream.write_all(&serialize_response(&resp))?;

            thread::spawn(move || {
                reboot_flag.store(true, Ordering::SeqCst);
                main_thread.unpark();
            });
        }

        IpcCommand::BootComplete => {
            let resp = IpcResponse {
                success: true,
                message: "Boot complete acknowledged".into(),
                data: None,
            };
            stream.write_all(&serialize_response(&resp))?;

            if let Ok(mut file) = file_logger.lock() {
                file.log(LogLevel::Info, "Verdantd reported boot complete.");
            }

            thread::spawn(move || {
                boot_complete_flag.store(true, Ordering::SeqCst);
                main_thread.unpark();
            });
        }

        _ => {
            let resp = IpcResponse {
                success: false,
                message: "Unsupported command for init".into(),
                data: None,
            };
            stream.write_all(&serialize_response(&resp))?;

            if let Ok(mut file) = file_logger.lock() {
                file.log(LogLevel::Fail, "Unsupported command for init");
            }
            if let Ok(mut con) = console_logger.lock() {
                con.message(LogLevel::Fail, "Unsupported command for init", std::time::Duration::ZERO);
            }
        }
    }

    Ok(())
}

