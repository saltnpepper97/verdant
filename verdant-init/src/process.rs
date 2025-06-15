use std::process::{Command, exit};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::{fork, ForkResult};
use common::utils::*;

pub fn start_verdantd() {
    println!("{} Starting verdantd service supervisor...", tag_boot());

    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            let _err = Command::new("/usr/bin/verdantd")
                .spawn()
                .expect("failed to exec verdantd");
            exit(0);
        }
        Ok(ForkResult::Parent { child }) => {
            println!("{} started with PID {}", status_ok(), child);
            loop {
                match waitpid(None, Some(WaitPidFlag::WNOHANG)) {
                    Ok(WaitStatus::StillAlive) => {
                        std::thread::sleep(std::time::Duration::from_secs(1));
                    }
                    Ok(status) => {
                        println!("Child exited: {:?}", status);
                    }
                    Err(e) => {
                        eprintln!("waitpid error: {:?}", e);
                        exit(1);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("fork failed: {:?}", e);
            exit(1);
        }
    }
}
