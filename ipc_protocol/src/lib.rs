use serde::{Serialize, Deserialize};

pub const SOCKET_PATH: &str = "/run/verdantd.sock";

#[derive(Serialize, Deserialize, Debug)]
pub enum Request {
    EnableModule { name: String },
    DisableModule { name: String },
    StartService { name: String },
    StopService { name: String },
    RestartService { name: String },
    Status,
    Shutdown,
    Reboot,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Response {
    Ok,
    Error(String),
    StatusInfo(String),
}

