use std::io::{self, Write};

// Colour constants - unified verdant theme
pub const RESET: &str = "\x1b[0m";
pub const GREEN_BOLD: &str = "\x1b[1;32m";
pub const GREEN_DIM: &str = "\x1b[0;32m";
pub const RED_BOLD: &str = "\x1b[1;31m";
pub const YELLOW_BOLD: &str = "\x1b[1;33m";

// Status tags with unified green shades
pub fn status_ok() -> String {
    format!("{GREEN_BOLD}[OK]{RESET}")
}

pub fn status_fail() -> String {
    format!("{RED_BOLD}[FAIL]{RESET}")
}

pub fn tag_info() -> String {
    format!("{GREEN_DIM}[INFO]{RESET}")
}

pub fn tag_boot() -> String {
    format!("{GREEN_BOLD}[BOOT]{RESET}")
}

pub fn tag_shutdown() -> String {
    format!("{GREEN_DIM}[SHUTDOWN]{RESET}")
}

pub fn tag_restart() -> String {
    format!("{GREEN_BOLD}[RESTART]{RESET}")
}

// Symbols for add, remove, change with yellow
pub fn symbol_add() -> String {
    format!("{YELLOW_BOLD}[+]{RESET}")
}

pub fn symbol_remove() -> String {
    format!("{YELLOW_BOLD}[-]{RESET}")
}

pub fn symbol_change() -> String {
    format!("{YELLOW_BOLD}[*]{RESET}")
}

// Printing functions
pub fn print_error(message: &str) {
    eprintln!("{} {}", status_fail(), message);
}

pub fn print_info(message: &str) {
    println!("{} {}", tag_info(), message);
    io::stdout().flush().unwrap();
}

pub fn print_boot_info(message: &str) {
    println!("{} {}", tag_boot(), message);
    io::stdout().flush().unwrap();
}

pub fn print_shutdown(message: &str) {
    println!("{} {}", tag_shutdown(), message);
    io::stdout().flush().unwrap();
}

pub fn print_restart(message: &str) {
    println!("{} {}", tag_restart(), message);
    io::stdout().flush().unwrap();
}

pub fn print_add(message: &str) {
    println!("{} {}", symbol_add(), message);
    io::stdout().flush().unwrap();
}

pub fn print_remove(message: &str) {
    println!("{} {}", symbol_remove(), message);
    io::stdout().flush().unwrap();
}

pub fn print_change(message: &str) {
    println!("{} {}", symbol_change(), message);
    io::stdout().flush().unwrap();
}

// Banner
pub fn verdant_banner() {
    println!("{GREEN_BOLD}>>> Verdant Init Starting...{RESET}");
}

