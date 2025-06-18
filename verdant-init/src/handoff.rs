use std::process::{Command, Stdio};
use std::os::unix::process::CommandExt;
use common::{print_step, status_ok, status_fail};

/// Forks and execs verdantd, replaces the current process image.
pub fn handoff_to_verdantd() -> ! {
    print_step("Starting verdantd", &status_ok());

    // Replace current process image with verdantd (exec)
    Command::new("/sbin/verdantd")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .exec();

    // If exec fails
    print_step("Failed to exec /sbin/verdantd", &status_fail());
    std::process::exit(1);
}
