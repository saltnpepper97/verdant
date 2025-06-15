<<<<<<< HEAD
fn main() {
    println!("Hello, world!");
}
=======
use clap::{Parser, Subcommand};
use std::path::Path;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

use ipc_protocol::{Request, Response, SOCKET_PATH};

#[derive(Parser)]
#[command(name = "verdantctl")]
#[command(about = "Control the verdantd daemon")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start a service
    Start { name: String },

    /// Stop a service
    Stop { name: String },

    /// Enable a kernel module
    EnableModule { name: String },

    /// Disable a kernel module
    DisableModule { name: String },

    /// Query system/service status
    Status,

    /// Shutdown the system
    Shutdown,

    /// Reboot the system
    Reboot,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if !Path::new(SOCKET_PATH).exists() {
        eprintln!("Error: IPC socket not found at {}", SOCKET_PATH);
        std::process::exit(1);
    }

    let mut stream = UnixStream::connect(SOCKET_PATH).await?;

    // Build request from CLI input
    let request = match cli.command {
        Command::Start { name } => Request::StartService { name },
        Command::Stop { name } => Request::StopService { name },
        Command::EnableModule { name } => Request::EnableModule { name },
        Command::DisableModule { name } => Request::DisableModule { name },
        Command::Status => Request::Status,
        Command::Shutdown => Request::Shutdown,
        Command::Reboot => Request::Reboot,
    };

    // Send serialized request
    let req_json = serde_json::to_string(&request)?;
    stream.write_all(req_json.as_bytes()).await?;
    stream.write_all(b"\n").await?;
    stream.flush().await?;

    // Receive and parse response
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;

    let response: Response = serde_json::from_str(line.trim_end())?;

    // Handle response
    match response {
        Response::Ok => println!("✔ Success"),
        Response::Error(err) => eprintln!("✘ Error: {}", err),
        Response::StatusInfo(info) => println!("{}", info),
    }

    Ok(())
}


>>>>>>> 3b07a92 (Lot's of changes)
