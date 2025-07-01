/// Represents general status results for operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Error,
    ServiceFailed,
    NotFound,
}

/// Represents the current lifecycle state of a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

/// Commands used to control services or the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Start,
    Stop,
    Restart,
    Reload,
    Shutdown,
    Reboot,
}

/// Log levels to control verbosity of logging output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Info,
    Warn,
    Fail,
    Ok,
}

