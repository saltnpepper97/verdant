use std::os::unix::net::UnixStream;
use std::io::{Write, BufRead, BufReader};

use clap::{Parser, Subcommand};
use ipc_protocol::{Request, Response, SOCKET_PATH};
use serde_json;

#[derive(Parser)]
#[command(name = "vctl")]
#[command(about = "Command-line tool to interact with verdant")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[command(about = "Start a service by name")]
    Start { name: String },

    #[command(about = "Stop a service by name")]
    Stop { name: String },

    #[command(about = "Restart a service by name")]
    Restart { name: String },

    #[command(about = "Reload a service's configuration by name")]
    Reload { name: String },

    #[command(about = "Reload all services")]
    ReloadAll,

    #[command(about = "Shutdown the system")]
    Shutdown,

    #[command(about = "Reboot the system")]
    Reboot,

    #[command(about = "Enable a module by name")]
    Enable { name: String },

    #[command(about = "Disable a module by name")]
    Disable { name: String },

    #[command(about = "Show system and service status")]
    Status,
}

fn send_request(req: &Request) -> std::io::Result<Response> {
    let mut stream = UnixStream::connect(SOCKET_PATH)?;

    let req_json = serde_json::to_string(req)?;
    stream.write_all(req_json.as_bytes())?;
    stream.write_all(b"\n")?;

    // Important: shutdown the write half so the server knows no more data is coming
    stream.shutdown(std::net::Shutdown::Write)?;

    let mut reader = BufReader::new(&stream);
    let mut response_buf = String::new();
    reader.read_line(&mut response_buf)?;

    let response: Response = serde_json::from_str(&response_buf)?;
    Ok(response)
}

fn main() {
    let cli = Cli::parse();

    let request = match &cli.command {
        Command::Start { name } => Request::Start { name: name.clone() },
        Command::Stop { name } => Request::Stop { name: name.clone() },
        Command::Restart { name } => Request::Restart { name: name.clone() },
        Command::Reload { name } => Request::Reload { name: name.clone() },
        Command::ReloadAll => Request::ReloadAll,
        Command::Shutdown => Request::Shutdown,
        Command::Reboot => Request::Reboot,
        Command::Enable { name } => Request::Enable { name: name.clone() },
        Command::Disable { name } => Request::Disable { name: name.clone() },
        Command::Status => Request::Status,
    };

    match send_request(&request) {
        Ok(Response::Success { message }) => println!("{}", message),
        Ok(Response::Error { message }) => eprintln!("Error: {}", message),
        Ok(Response::StatusInfo(info)) => println!("{}", info),
        Err(e) => eprintln!("Failed to communicate with verdantd: {}", e),
    }
}

