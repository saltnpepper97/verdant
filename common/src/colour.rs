pub const RESET: &str = "\x1b[0m";
pub const GREEN: &str = "\x1b[1;32m";
pub const RED: &str = "\x1b[1;31m";
pub const CYAN: &str = "\x1b[1;36m";
pub const YELLOW: &str = "\x1b[1;33m";
pub const MAGENTA: &str = "\x1b[1;35m";

pub fn status_ok() -> &'static str {
    concat!("\x1b[1;32m", "⟦ OK ⟧", "\x1b[0m")
}

pub fn status_fail() -> &'static str {
    concat!("\x1b[1;31m", "⟦FAIL⟧", "\x1b[0m")
}

pub fn tag(tag: &str, color: &str) -> String {
    format!("{color}[{tag}]{RESET}")
}

pub fn tag_boot() -> &'static str {
    concat!("\x1b[1;35m", "[BOOT]", "\x1b[0m")
}

pub fn verdant_banner() {
    println!("{GREEN}🌿 Verdant Init Starting...{RESET}");
}

