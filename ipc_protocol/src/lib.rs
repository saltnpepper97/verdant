use serde::{Serialize, Deserialize};

pub const SOCKET_PATH: &str = "/run/verdantd.sock";

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", content = "data")]
pub enum Request {
    Start { name: String },
    Stop { name: String },
    Restart { name: String },
    Reload { name: String },
    ReloadAll,
    Shutdown,
    Reboot,
    Enable { name: String },
    Disable { name: String },
    Status,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Response {
    Success { message: String },
    Error { message: String },
    StatusInfo(String),
}

