use std::process::Command;

fn main() {
    let status = Command::new("/usr/bin/vctl")
        .arg("shutdown")
        .status()
        .expect("failed to execute vctl shutdown");
    std::process::exit(status.code().unwrap_or(1));
}
