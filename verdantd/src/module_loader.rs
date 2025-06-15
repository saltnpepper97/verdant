use std::process::{Command, exit, Stdio};
use std::collections::HashSet;
use std::io::{self, Write};
use std::fs;
use common::utils::*;

#[derive(Debug, PartialEq, Eq)]
enum ModuleRequirement {
    Required,
    Optional,
    Disabled,
}

fn parse_module_line(line: &str) -> (ModuleRequirement, &str) {
    let trimmed = line.trim();
    if trimmed.starts_with('!') {
        (ModuleRequirement::Required, trimmed[1..].trim())
    } else if trimmed.starts_with('-') {
        (ModuleRequirement::Disabled, trimmed[1..].trim())
    } else {
        (ModuleRequirement::Optional, trimmed)
    }
}

fn load_module(module: &str) -> Result<(), ()> {
    print_boot_info(&format!("Loading module {}...", module));
    io::stdout().flush().unwrap();

    let status = Command::new("modprobe")
        .arg(module)
        .stdout(Stdio::null())  // suppress stdout
        .stderr(Stdio::null())  // suppress stderr
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("{} Module {} loaded successfully", status_ok(), module);
            Ok(())
        }
        Ok(_) | Err(_) => {
            Err(())
        }
    }
}

pub fn load_modules_from_file(
    path: &str,
    disabled: &HashSet<String>,
    fail_on_required_error: bool,
) -> Result<(), ()> {
    print_boot_info(&format!("Reading modules from {}", path));
    io::stdout().flush().unwrap();

    let contents = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            print_error(&format!("Failed to read {}: {}", path, e));
            return Err(());
        }
    };

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let (requirement, module) = parse_module_line(line);

        if requirement == ModuleRequirement::Disabled {
            // Disabled modules are just skipped
            continue;
        }

        // Skip loading default modules that are disabled by overrides
        if disabled.contains(module) && fail_on_required_error {
            print_info(&format!("Skipping disabled module {}", module));
            continue;
        }

        let result = load_module(module);

        match (requirement, result) {
            (ModuleRequirement::Required, Err(_)) => {
                print_error(&format!("Required module {} failed to load, aborting.", module));
                return Err(());
            }
            (_, Err(_)) => {
                print_error(&format!("Optional module {} failed to load, continuing.", module));
            }
            (_, Ok(())) => {}
        }
    }

    Ok(())
}

pub fn load_all_modules() {
    // First collect all disabled modules from override configs (modules.d)
    let modules_dir = "/etc/verdant/modules.d";
    let mut disabled_modules = HashSet::new();

    if let Ok(entries) = fs::read_dir(modules_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("conf") {
                if let Ok(contents) = fs::read_to_string(&path) {
                    for line in contents.lines() {
                        let line = line.trim();
                        let (req, module) = parse_module_line(line);
                        if req == ModuleRequirement::Disabled {
                            disabled_modules.insert(module.to_string());
                        }
                    }
                }
            }
        }
    }

    // Load defaults, skipping disabled modules, fail if required module fails
    if let Err(_) = load_modules_from_file("/etc/verdant/default.conf", &disabled_modules, true) {
        exit(1);
    }

    print_info(&format!("Loading additional module configs from {}", modules_dir));
    io::stdout().flush().unwrap();

    // Load overrides, but here no disables — just load required/optional modules
    if let Ok(entries) = fs::read_dir(modules_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("conf") {
                // Load modules from override file, ignoring disabled lines
                if let Err(_) = load_modules_from_file(path.to_str().unwrap(), &HashSet::new(), false) {
                    exit(1);
                }
            }
        }
    }
}
