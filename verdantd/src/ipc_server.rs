use std::sync::mpsc::Sender;
use std::time::Duration;
use bloom::ipc::{IpcCommand, IpcRequest, IpcResponse, IpcTarget, serve_ipc_socket, send_ipc_request, INIT_SOCKET_PATH, VERDANTD_SOCKET_PATH};
use bloom::log::{ConsoleLogger, FileLogger};

/// Spawns the IPC server for verdantd. Handles shutdown and reboot commands.
///
/// Sends a `Shutdown` or `Reboot` command to the main manager thread via the provided channel.
pub fn run_ipc_server(shutdown_tx: Sender<IpcCommand>) -> std::io::Result<()> {
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

pub fn send_boot_complete(
    file_logger: &mut dyn FileLogger,
) {
    let notify = IpcRequest {
        target: IpcTarget::Init,
        command: IpcCommand::BootComplete,
    };

    match send_ipc_request(INIT_SOCKET_PATH, &notify) {
        Ok(resp) if resp.success => {
            file_logger.log(bloom::status::LogLevel::Info, "Notified init: Verdantd boot complete.");
        }
        Ok(resp) => {
            let msg = format!("Init responded with failure: {}", resp.message);
            file_logger.log(bloom::status::LogLevel::Warn, &msg);
        }
        Err(e) => {
            let msg = format!("Failed to notify init of boot complete: {}", e);
            file_logger.log(bloom::status::LogLevel::Fail, &msg);
        }
    }
}
