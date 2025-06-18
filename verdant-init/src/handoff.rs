use std::process::{Command, Stdio};
use common::{print_step, status_ok, status_fail};

/// Launches verdantd as a subprocess. Does not replace current image.
pub fn handoff_to_verdantd() {
    match Command::new("/sbin/verdantd")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => {
            print_step("Handoff to /sbin/verdantd successful", &status_ok());
            std::process::exit(0);
        }
        Err(err) => {
            print_step(&format!("Failed to exec /sbin/verdantd: {}", err), &status_fail());
            std::process::exit(1);
        }
    }
}

