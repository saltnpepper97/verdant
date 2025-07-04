use bloom::status::ServiceState;

#[derive(Debug, Clone)]
pub struct Service {
    pub name: String,
    pub desc: String,
    pub cmd: String,
    pub args: Vec<String>,
    pub startup: StartupPackage,
    pub restart: RestartPolicy,
    pub tags: Vec<String>,
    pub instances: Vec<String>,
    pub state: ServiceState,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupPackage {
    Base,
    Network,
    System,
    User,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartPolicy {
    Never,
    Always,
    OnFailure,
}

impl StartupPackage {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "base" => Some(Self::Base),
            "network" => Some(Self::Network),
            "system" => Some(Self::System),
            "user" => Some(Self::User),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            StartupPackage::Base => "base",
            StartupPackage::Network => "network",
            StartupPackage::System => "system",
            StartupPackage::User => "user",
            StartupPackage::Custom => "custom",
        }
    }
}

impl RestartPolicy {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "never" => Some(Self::Never),
            "always" => Some(Self::Always),
            "on-failure" => Some(Self::OnFailure),
            _ => None,
        }
    }
}

