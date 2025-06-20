use std::process::Command;

fn main() {
    let status = Command::new("/usr/bin/vctl")
        .arg("reboot")
        .status()
        .expect("failed to execute vctl reboot");
    std::process::exit(status.code().unwrap_or(1));
}
