use std::os::unix::net::UnixStream;
use std::io::{Write, Read};

use clap::{Parser, Subcommand};
use ipc_protocol::{Request, Response, SOCKET_PATH};
use serde_json;

#[derive(Parser)]
#[command(name = "verdantctl")]
#[command(about = "Command-line tool to interact with verdantd")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Start { name: String },
    Stop { name: String },
    Restart { name: String },
    Reload { name: String },
    ReloadAll,
    Shutdown,
    Reboot,
    EnableModule { name: String },
    DisableModule { name: String },
    Status,
}

fn send_request(req: &Request) -> std::io::Result<Response> {
    let mut stream = UnixStream::connect(SOCKET_PATH)?;

    let req_json = serde_json::to_string(req)?;
    stream.write_all(req_json.as_bytes())?;
    stream.write_all(b"\n")?;

    let mut response_buf = String::new();
    stream.read_to_string(&mut response_buf)?;

    let response: Response = serde_json::from_str(&response_buf)?;
    Ok(response)
}

fn main() {
    let cli = Cli::parse();

    let (request, action_description) = match &cli.command {
        Command::Start { name } => (
            Request::StartService { name: name.clone() },
            format!("Starting service '{}'", name),
        ),
        Command::Stop { name } => (
            Request::StopService { name: name.clone() },
            format!("Stopping service '{}'", name),
        ),
        Command::Restart { name } => (
            Request::RestartService { name: name.clone() },
            format!("Restarting service '{}'", name),
        ),
        Command::Reload { name } => (
            Request::ReloadService { name: name.clone() },
            format!("Reloading service '{}'", name),
        ),
        Command::ReloadAll => (
            Request::ReloadAllServices,
            "Reloading all services".into(),
        ),
        Command::Shutdown => (
            Request::Shutdown,
            "Shutting down system".into(),
        ),
        Command::Reboot => (
            Request::Reboot,
            "Rebooting system".into(),
        ),
        Command::EnableModule { name } => (
            Request::EnableModule { name: name.clone() },
            format!("Enabling module '{}'", name),
        ),
        Command::DisableModule { name } => (
            Request::DisableModule { name: name.clone() },
            format!("Disabling module '{}'", name),
        ),
        Command::Status => (
            Request::Status,
            "Fetching system status".into(),
        ),
    };

    match send_request(&request) {
        Ok(Response::Ok) => println!("Success: {}", action_description),
        Ok(Response::StatusInfo(info)) => println!("{}", info),
        Ok(Response::Error(e)) => eprintln!("Failed to {}: {}", action_description.to_lowercase(), e),
        Err(e) => eprintln!("Failed to communicate with verdantd: {}", e),
    }
}

