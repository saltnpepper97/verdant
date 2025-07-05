use std::fs;
use std::path::Path;
use std::sync::mpsc::Sender;

use bloom::ipc::{IpcCommand, IpcRequest, IpcResponse, serve_ipc_socket, VERDANTD_SOCKET_PATH};

/// Spawns the IPC server for verdantd. Handles shutdown and reboot commands.
///
/// Sends a `Shutdown` or `Reboot` command to the main manager thread via the provided channel.
pub fn run_ipc_server(shutdown_tx: Sender<IpcCommand>) -> std::io::Result<()> {
    let socket_path = Path::new(VERDANTD_SOCKET_PATH);

    // Ensure parent directory exists
    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Clean up stale socket if it exists
    if socket_path.exists() {
        fs::remove_file(socket_path)?;
    }

    // Now serve IPC
    serve_ipc_socket(VERDANTD_SOCKET_PATH, move |request: IpcRequest| {
        if request.target != bloom::ipc::IpcTarget::Verdantd {
            return IpcResponse {
                success: false,
                message: "Incorrect target".into(),
                data: None,
            };
        }

        match request.command {
            IpcCommand::Shutdown | IpcCommand::Reboot => {
                match shutdown_tx.send(request.command.clone()) {
                    Ok(_) => IpcResponse {
                        success: true,
                        message: format!("Proceeding with {:?}", request.command),
                        data: None,
                    },
                    Err(e) => IpcResponse {
                        success: false,
                        message: format!("Failed to trigger shutdown: {}", e),
                        data: None,
                    },
                }
            }

            _ => IpcResponse {
                success: false,
                message: format!("Unhandled command: {:?}", request.command),
                data: None,
            },
        }
    });

    Ok(())
}

