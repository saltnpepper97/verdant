use clap::{Parser, Subcommand};
use bloom::ipc::{IpcRequest, IpcTarget, IpcCommand, send_ipc_request, VERDANTD_SOCKET_PATH};

#[derive(Parser)]
#[command(name = "vctl")]
#[command(about = "Verdant Control CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Shutdown,
    Reboot,
}

fn main() {
    let cli = Cli::parse();

    let ipc_command = match cli.command {
        Commands::Shutdown => IpcCommand::Shutdown,
        Commands::Reboot => IpcCommand::Reboot,
    };

    let request = IpcRequest {
        target: IpcTarget::Verdantd,
        command: ipc_command,
    };

    match send_ipc_request(VERDANTD_SOCKET_PATH, &request) {
        Ok(response) => {
            if response.success {
                println!("Command succeeded: {}", response.message);
            } else {
                eprintln!("Command failed: {}", response.message);
            }
        }
        Err(e) => {
            eprintln!("Failed to send IPC request: {}", e);
        }
    }
}

