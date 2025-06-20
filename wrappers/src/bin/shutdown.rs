use std::process::Command;

fn main() {
    let status = Command::new("/usr/bin/verdantctl")
        .arg("shutdown")
        .status()
        .expect("failed to execute verdantctl shutdown");
    std::process::exit(status.code().unwrap_or(1));
}
