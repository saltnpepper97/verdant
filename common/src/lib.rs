pub const RESET: &str = "\x1b[0m";
pub const GREEN_BOLD: &str = "\x1b[1;32m";
pub const GREEN_DIM: &str = "\x1b[0;32m";
pub const RED_BOLD: &str = "\x1b[1;31m";
pub const YELLOW_BOLD: &str = "\x1b[1;33m";
pub const CYAN_BOLD: &str = "\x1b[1;36m";

/// Status tags
pub fn status_ok() -> String {
    format!("{GREEN_BOLD}[ OK ]{RESET}")
}

pub fn status_fail() -> String {
    format!("{RED_BOLD}[FAIL]{RESET}")
}

pub fn status_skip() -> String {
    format!("{YELLOW_BOLD}[SKIP]{RESET}")
}

/// Printing functions
pub fn print_step(label: &str, status: &str) {
    println!("• {:<60}{}", label, status);
}

pub fn print_info_step(label: &str) {
    println!("• {:<60}", label);
}

pub fn print_substep(label: &str, status: &str) {
    // For middle substeps, use ├─
    println!("  ├─ {:<57}{}", label, status);
}

pub fn print_substep_last(label: &str, status: &str) {
    // For last substep, use └─
    println!("  └─ {:<57}{}", label, status);
}

pub fn print_boot_step(label: &str) {
    print_step(label, &status_ok());
}

pub fn print_shutdown_step(label: &str) {
    print_step(label, &status_ok());
}

pub fn print_restart_step(label: &str) {
    print_step(label, &status_ok());
}

/// Verdantctl
pub fn print_success(message: &str) {
    println!("{}✔{} {}", GREEN_BOLD, RESET, message);
}

pub fn print_error(message: &str) {
    eprintln!("{}✘{} {}", RED_BOLD, RESET, message);
}

pub fn print_info(message: &str) {
    println!("{}i{} {}", CYAN_BOLD, RESET, message);
}

// Banner
pub fn verdant_banner(os_name: &str) {
    println!(
        "{green}>> Verdant Init (PID 1) starting {cyan}{os}{green}{reset}",
        green = GREEN_BOLD,
        cyan = CYAN_BOLD,
        os = os_name,
        reset = RESET
    );
}

