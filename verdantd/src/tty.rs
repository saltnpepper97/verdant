use std::process::{Command, Stdio};
use std::thread;
use std::path::Path;

const TTY_BIN_CANDIDATES: &[&str] = &[
    "/sbin/agetty",
    "/bin/agetty",
    "/usr/bin/agetty",
    "/usr/sbin/agetty",
    "/sbin/getty",
    "/bin/getty",
    "/usr/bin/getty",
    "/usr/sbin/getty",
    "/sbin/mingetty",
    "/bin/mingetty",
    "/usr/bin/mingetty",
    "/usr/sbin/mingetty",
];


/// Tries to find a working getty/agetty binary.
fn find_getty_binary() -> Option<String> {
    for path in TTY_BIN_CANDIDATES {
        if Path::new(path).exists() {
            return Some(path.to_string());
        }
    }
    None
}

/// Spawns a getty on the specified tty (e.g. "tty1").
pub fn spawn_tty(tty: &str) -> Result<(), String> {
    let getty = find_getty_binary().ok_or("No getty/agetty binary found")?;

    let tty_path = format!("/dev/{}", tty);
    if !Path::new(&tty_path).exists() {
        return Err(format!("TTY device not found: {}", tty_path));
    }

    println!("[verdantd] Launching getty: {} on {}", getty, tty);

    let getty_path = getty.clone();
    let tty_string = tty.to_owned();

    thread::spawn(move || {
        loop {
            let mut cmd = Command::new(&getty_path);

            // All getty variants prefer just "tty1", not "/dev/tty1"
            cmd.arg("38400").arg(&tty_string);

            cmd.stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());

            match cmd.spawn() {
                Ok(mut child) => {
                    let _ = child.wait();
                }
                Err(e) => {
                    eprintln!("[verdantd] Failed to spawn getty on {}: {}", tty_string, e);
                    break;
                }
            }

            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    });

    Ok(())
}

