use serde::{Serialize, Deserialize};

pub const SOCKET_PATH: &str = "/run/verdantd.sock";

#[derive(Serialize, Deserialize)]
pub enum Request {
    StartService { name: String },
    StopService { name: String },
    RestartService { name: String },
    ReloadService { name: String },
    ReloadAllServices,
    Shutdown,
    Reboot,
    EnableModule { name: String },
    DisableModule { name: String },
    Status,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Response {
    Ok,
    Error(String),
    StatusInfo(String),
}


