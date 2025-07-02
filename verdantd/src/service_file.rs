#[derive(Debug, Clone, PartialEq)]
pub enum RestartPolicy {
    OnFailure,
    Always,
    Never,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        RestartPolicy::Never
    }
}

/// Represents a parsed .vs service file
#[derive(Debug, Clone)]
pub struct ServiceFile {
    pub name: String,
    pub desc: Option<String>,

    pub cmd: String,
    pub args: Option<Vec<String>>,

    pub pre_cmd: Option<String>,
    pub post_cmd: Option<String>,

    pub package: Option<String>,
    pub startup_package: Option<String>,

    pub env: Option<Vec<String>>, // e.g. ["TERM=linux", "PATH=/usr/bin"]
    pub user: Option<String>,
    pub group: Option<String>,
    pub working_dir: Option<String>,

    pub restart: RestartPolicy,
    pub restart_delay: Option<u64>, // seconds

    pub stop_cmd: Option<String>,

    pub timeout_start: Option<u64>, // seconds
    pub timeout_stop: Option<u64>,  // seconds

    pub dependencies: Option<Vec<String>>,
    pub priority: Option<i32>,

    pub stdout_log: Option<String>,
    pub stderr_log: Option<String>,

    pub umask: Option<String>, // usually a string representing octal like "022"
    pub nice: Option<i32>,

    pub tags: Option<Vec<String>>,
}

impl ServiceFile {
    pub fn new(name: String, cmd: String) -> Self {
        Self {
            name,
            desc: None,
            cmd,
            args: None,
            pre_cmd: None,
            post_cmd: None,
            package: None,
            startup_package: None,
            env: None,
            user: None,
            group: None,
            working_dir: None,
            restart: RestartPolicy::Never,
            restart_delay: None,
            stop_cmd: None,
            timeout_start: None,
            timeout_stop: None,
            dependencies: None,
            priority: None,
            stdout_log: None,
            stderr_log: None,
            umask: None,
            nice: None,
            tags: None,
        }
    }
}
