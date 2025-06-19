use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::io::{self, Error, ErrorKind};

#[derive(Debug)]
pub enum RestartPolicy {
    Always,
    OnFailure,
    Never,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        RestartPolicy::Never
    }
}

#[derive(Debug, Default)]
pub struct ServiceConfig {
    pub name: String,
    pub description: Option<String>,
    pub exec: String,
    pub args: Option<Vec<String>>,
    pub restart: RestartPolicy,
    pub requires: Vec<String>,
    pub after: Vec<String>,
}


impl ServiceConfig {
    pub fn from_file<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let content = fs::read_to_string(path)?;
        let mut map = HashMap::new();

        for (i, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let parts: Vec<&str> = line.splitn(2, '=').collect();
            if parts.len() != 2 {
                return Err(Error::new(ErrorKind::InvalidData, format!("Invalid line {}: {}", i + 1, line)));
            }

            map.insert(parts[0].trim().to_lowercase(), parts[1].trim().to_string());
        }

        let name = map.remove("name").ok_or_else(|| Error::new(ErrorKind::InvalidData, "Missing 'name'"))?;
        let exec = map.remove("exec").ok_or_else(|| Error::new(ErrorKind::InvalidData, "Missing 'exec'"))?;
        let description = map.remove("description");
        let args = map.remove("args").map(|a| {
            shell_words::split(&a).map_err(|e| {
                Error::new(ErrorKind::InvalidData, format!("Failed to parse args: {}", e))
            })
        }).transpose()?;
        let requires = map.remove("requires").map_or_else(Vec::new, |r| r.split(',').map(|s| s.trim().to_string()).collect());
        let after = map.remove("after").map_or_else(Vec::new, |r| {
            r.split(',').map(|s| s.trim().to_string()).collect()
        });
        let restart = match map.remove("restart").as_deref() {
            Some("always") => RestartPolicy::Always,
            Some("on-failure") => RestartPolicy::OnFailure,
            Some("never") | None => RestartPolicy::Never,
            Some(other) => return Err(Error::new(ErrorKind::InvalidData, format!("Unknown restart policy: {}", other))),
        };

        Ok(ServiceConfig {
            name,
            description,
            exec,
            args,
            restart,
            requires,
            after,
        })
    }
}
