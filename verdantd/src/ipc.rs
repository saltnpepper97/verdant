use common::utils::print_info;
use ipc_protocol::*;
use std::path::Path;
use std::sync::Arc;
use tokio::{io::{AsyncBufReadExt, AsyncWriteExt, BufReader as TokioBufReader}, net::UnixListener as TokioUnixListener};


/// The socket path for the IPC Unix Domain Socket.
pub const SOCKET_PATH: &str = "/run/verdantd.sock";

/// Helper: serialize & send a Response over the stream.
pub async fn send_response(stream: &mut tokio::net::UnixStream, resp: &Response) -> std::io::Result<()> {
    let data = serde_json::to_string(resp).unwrap();
    stream.write_all(data.as_bytes()).await?;
    stream.write_all(b"\n").await?; // newline delimiter
    stream.flush().await?;
    Ok(())
}

/// Helper: receive & deserialize a Request from the stream.
pub async fn receive_request(stream: &mut tokio::net::UnixStream) -> std::io::Result<Option<Request>> {
    let mut reader = TokioBufReader::new(stream);
    let mut line = String::new();
    let bytes = reader.read_line(&mut line).await?;
    if bytes == 0 {
        // EOF
        return Ok(None);
    }
    let req: Request = serde_json::from_str(line.trim_end()).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, format!("Failed to parse request: {}", e))
    })?;
    Ok(Some(req))
}

/// Start IPC server — listen on SOCKET_PATH and handle incoming connections.
pub async fn run_ipc_server(handle_request: Arc<dyn Fn(Request) -> Response + Send + Sync>, ) -> std::io::Result<()>
{
    // Remove old socket if exists
    if Path::new(SOCKET_PATH).exists() {
        std::fs::remove_file(SOCKET_PATH)?;
    }

    let listener = TokioUnixListener::bind(SOCKET_PATH)?;
    print_info(&format!("IPC server listening on {}", SOCKET_PATH));

    loop {
        let (mut stream, _) = listener.accept().await?;
        let handle_request = handle_request.clone();

        // Spawn a task to handle the client connection
        tokio::spawn(async move {
            // For simplicity: expect one request, send one response, close
            match receive_request(&mut stream).await {
                Ok(Some(req)) => {
                    let resp = handle_request(req);
                    if let Err(e) = send_response(&mut stream, &resp).await {
                        eprintln!("Failed to send response: {}", e);
                    }
                }
                Ok(None) => {
                    // Client closed connection early
                }
                Err(e) => {
                    eprintln!("Failed to receive request: {}", e);
                }
            }
        });
    }
}
