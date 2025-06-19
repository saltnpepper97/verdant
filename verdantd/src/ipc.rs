use std::os::unix::net::{UnixListener, UnixStream};
use std::io::{BufReader, BufRead, Write};
use std::sync::mpsc::{Sender};
use std::thread;

use common::{print_step, status_fail, status_ok};
use ipc_protocol::{Request, Response, SOCKET_PATH};
use serde_json;

use crate::runtime::SystemAction;

pub fn start_ipc_listener(tx: Sender<SystemAction>) -> std::io::Result<()> {
    // Clean up old socket
    let _ = std::fs::remove_file(SOCKET_PATH);

    let listener = UnixListener::bind(SOCKET_PATH)?;
    print_step(&format!("IPC listener running on {}", SOCKET_PATH), &status_ok());

    thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    if let Err(e) = handle_client(stream, &tx) {
                        print_step(&format!("IPC client handler error: {}", e), &status_fail());
                    }
                }
                Err(e) => {
                    print_step(&format!("IPC listener error: {}", e), &status_fail());
                }
            }
        }
    });

    Ok(())
}

fn handle_client(mut stream: UnixStream, tx: &Sender<SystemAction>) -> std::io::Result<()> {
    let mut reader = BufReader::new(&stream);
    let mut buf = String::new();
    reader.read_line(&mut buf)?;
    
    let request: Result<Request, _> = serde_json::from_str(buf.trim());
    let response = match request {
        Ok(req) => handle_request(req, tx),
        Err(e) => {
            Response::Error(format!("Failed to parse request: {}", e))
        }
    };

    let resp_text = serde_json::to_string(&response).unwrap_or_else(|_| "{\"Error\":\"Serialization failed\"}".into());
    stream.write_all(resp_text.as_bytes())?;
    stream.write_all(b"\n")?;
    Ok(())
}

fn handle_request(req: Request, tx: &Sender<SystemAction>) -> Response {
    match req {
        Request::Reboot => {
            if tx.send(SystemAction::Reboot).is_ok() {
                Response::Ok
            } else {
                Response::Error("Failed to send reboot command".into())
            }
        }
        Request::Shutdown => {
            if tx.send(SystemAction::Shutdown).is_ok() {
                Response::Ok
            } else {
                Response::Error("Failed to send shutdown command".into())
            }
        }
        // You can extend here for other commands (start/stop/restart service etc)
        _ => Response::Error("Command not implemented yet".into()),
    }
}
