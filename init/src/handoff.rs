use std::process::{Command, Stdio};
use std::os::unix::process::CommandExt;
use common::{print_info_step, print_step, status_fail};

/// Replaces verdant-init with verdantd
pub fn handoff_to_verdantd() -> ! {
    print_info_step("Handing off to /usr/sbin/verdantd");

    let _ = Command::new("/usr/sbin/verdantd")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .exec();

    // Only runs if exec fails
    print_step("Failed to exec /usr/sbin/verdantd", &status_fail());
    std::process::exit(1);
}

