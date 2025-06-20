use std::sync::mpsc::Sender;
use std::thread;
use signal_hook::consts::signal::*;
use signal_hook::iterator::Signals;
use libc::SIGPWR;

use crate::runtime::SystemAction;

pub fn spawn_signal_listener(tx: Sender<SystemAction>) {
    // Only listen to shutdown-related signals
    let signals = Signals::new(&[SIGTERM, SIGINT, SIGPWR]);

    match signals {
        Ok(mut signals) => {
            thread::spawn(move || {
                for signal in signals.forever() {
                    match signal {
                        SIGTERM | SIGINT | SIGPWR => {
                            // Send shutdown signal to main thread
                            let _ = tx.send(SystemAction::Shutdown);
                        }
                        _ => (),
                    }
                }
            });
        }
        Err(e) => {
            eprintln!("Failed to register signal handlers: {}", e);
        }
    }
}
