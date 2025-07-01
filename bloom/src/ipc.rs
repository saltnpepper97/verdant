use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::thread;

use serde::{Deserialize, Serialize};

//
// ─── SOCKET PATHS ────────────────────────────────────────────────────────

/// Socket path for the init process.
pub const INIT_SOCKET_PATH: &str = "/run/verdant/init.sock";

/// Socket path for the verdantd service manager.
pub const VERDANTD_SOCKET_PATH: &str = "/run/verdant/verdantd.sock";

//
// ─── MESSAGES ────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum IpcTarget {
    Init,
    Verdantd,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum IpcInternal {
    PreShutdown,
    ReloadConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum IpcCommand {
    // System-level
    Shutdown,
    Reboot,

    // Service control
    StartService(String),
    StopService(String),
    RestartService(String),
    EnableService(String),
    DisableService(String),

    // Status
    GetStatus,
    GetServiceStatus(String),

    // Internal messages
    Internal(IpcInternal),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IpcRequest {
    pub target: IpcTarget,
    pub command: IpcCommand,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IpcResponse {
    pub success: bool,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

//
// ─── SERIALIZATION HELPERS ───────────────────────────────────────────────

pub fn serialize_request(req: &IpcRequest) -> Vec<u8> {
    let mut vec = serde_json::to_vec(req).expect("Failed to serialize IPC request");
    vec.push(b'\n');
    vec
}

pub fn deserialize_request(buf: &[u8]) -> IpcRequest {
    serde_json::from_slice(buf).expect("Failed to deserialize IPC request")
}

pub fn serialize_response(resp: &IpcResponse) -> Vec<u8> {
    let mut vec = serde_json::to_vec(resp).expect("Failed to serialize IPC response");
    vec.push(b'\n');
    vec
}

pub fn deserialize_response(buf: &[u8]) -> IpcResponse {
    serde_json::from_slice(buf).expect("Failed to deserialize IPC response")
}

//
// ─── IPC TRANSPORT CLIENT ────────────────────────────────────────────

/// Sends an IPC request and waits for a response.
/// Used by `vctl` to communicate with `init` or `verdantd`.
pub fn send_ipc_request(socket_path: &str, request: &IpcRequest) -> Result<IpcResponse, std::io::Error> {
    let mut stream = match UnixStream::connect(socket_path) {
        Ok(s) => s,
        Err(e) => return Err(e),
    };

    let data = serialize_request(request);
    stream.write_all(&data)?;

    let mut reader = BufReader::new(stream);
    let mut buf = Vec::new();
    reader.read_until(b'\n', &mut buf)?;

    Ok(deserialize_response(&buf))
}


//
// ─── IPC SERVER HELPER ────────────────────────────────────────────

pub fn serve_ipc_socket<P: AsRef<Path>>(
    socket_path: P,
    handler: impl Fn(IpcRequest) -> IpcResponse + Send + Sync + 'static + Clone,
) {
    let _ = std::fs::remove_file(&socket_path);
    let listener = UnixListener::bind(&socket_path).expect("Failed to bind to IPC socket");

    for stream in listener.incoming() {
        if let Ok(mut stream) = stream {
            let handler = handler.clone();
            thread::spawn(move || {
                let mut reader = BufReader::new(&stream);
                let mut buf = Vec::new();
                if reader.read_until(b'\n', &mut buf).is_ok() {
                    if let Ok(request) = serde_json::from_slice::<IpcRequest>(&buf) {
                        let response = handler(request);
                        let data = serialize_response(&response);
                        let _ = stream.write_all(&data);
                    }
                }
            });
        }
    }
}

