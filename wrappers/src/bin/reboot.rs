use std::process::Command;

fn main() {
    let status = Command::new("/usr/bin/verdantctl")
        .arg("reboot")
        .status()
        .expect("failed to execute verdantctl reboot");
    std::process::exit(status.code().unwrap_or(1));
}
