use common::colour::*;
use std::io::{self, Write};

/// Print an error message with the "fail" status prefix
pub fn print_error(msg: &str) {
    println!("{} {}", status_fail(), msg);
    io::stdout().flush().unwrap();
}

/// Print an info message with the "boot" tag prefix
pub fn print_info(msg: &str) {
    println!("{} {}", tag_boot(), msg);
    io::stdout().flush().unwrap();
}
