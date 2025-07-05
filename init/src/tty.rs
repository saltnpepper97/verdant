use std::{
    collections::HashMap,
    io,
    path::Path,
    process::{Child, Command},
    thread,
    time::Duration,
};

pub struct TtyManager {
    pub children: HashMap<String, Child>,
    pub getty_path: String,
}

impl TtyManager {
    const GETTY_CANDIDATES: [&'static str; 3] = [
        "/sbin/agetty",
        "/sbin/getty",
        "/sbin/mingetty",
    ];

    fn detect_getty() -> io::Result<String> {
        for &path in &Self::GETTY_CANDIDATES {
            if Path::new(path).is_file() {
                return Ok(path.to_string());
            }
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "No suitable getty binary found",
        ))
    }

    pub fn new() -> io::Result<Self> {
        let getty_path = Self::detect_getty()?;
        Ok(Self {
            children: HashMap::new(),
            getty_path,
        })
    }

    /// Launch getty sessions and return list of tty names successfully started.
    pub fn launch_tty_sessions(&mut self, tty_sessions: &[String]) -> io::Result<Vec<String>> {
        let mut launched = Vec::new();

        for tty in tty_sessions {
            match self.spawn_getty(tty) {
                Ok(child) => {
                    self.children.insert(tty.clone(), child);
                    launched.push(tty.clone());
                }
                Err(e) => {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("Failed to spawn getty on {}: {}", tty, e),
                    ));
                }
            }
        }

        Ok(launched)
    }

    fn spawn_getty(&self, tty: &str) -> io::Result<Child> {
        let mut cmd = Command::new(&self.getty_path);
        match self.getty_path.as_str() {
            "/sbin/agetty" | "/sbin/getty" => {
                cmd.arg("-L").arg(tty).arg("115200");
            }
            "/sbin/mingetty" => {
                cmd.arg(tty);
            }
            _ => {
                cmd.arg(tty);
            }
        }
        cmd.spawn()
    }

    /// Monitor and restart crashed getty sessions.
    pub fn supervise(&mut self) {
        loop {
            let mut to_restart = Vec::new();

            for (tty, child) in &mut self.children {
                match child.try_wait() {
                    Ok(Some(_)) => {
                        to_restart.push(tty.clone());
                    }
                    Ok(None) => {}
                    Err(_) => {}
                }
            }

            for tty in to_restart {
                let _ = self.restart_tty(&tty);
            }

            thread::sleep(Duration::from_secs(1));
        }
    }

    pub fn restart_tty(&mut self, tty: &str) -> io::Result<()> {
        self.children.remove(tty);
        let child = self.spawn_getty(tty)?;
        self.children.insert(tty.to_string(), child);
        Ok(())
    }
}

