use std::process::{Command, Stdio};
use std::collections::HashMap;
use std::io::{self, Write};
use std::fs;
use common::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleRequirement {
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

fn load_module(module: &str, is_last: bool) -> Result<(), ()> {
    io::stdout().flush().unwrap();

    let status = Command::new("modprobe")
        .arg(module)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    match status {
        Ok(s) if s.success() => {
            if is_last {
                print_substep_last(&format!("Module {} loaded", module), &status_ok());
            } else {
                print_substep(&format!("Module {} loaded", module), &status_ok());
            }
            Ok(())
        }
        Ok(_) | Err(_) => {
            Err(())
        }
    }
}

pub fn merge_module_configs() -> Result<HashMap<String, ModuleRequirement>, ()> {
    let mut modules: HashMap<String, ModuleRequirement> = HashMap::new();

    let merge_file = |path: &str, modules: &mut HashMap<String, ModuleRequirement>| -> Result<(), ()> {
        let contents = fs::read_to_string(path).map_err(|e| {
            print_substep(&format!("Failed to read {}: {}", path, e), &status_fail());
            ()
        })?;

        for line in contents.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let (req, module) = parse_module_line(line);
            match req {
                ModuleRequirement::Disabled => {
                    modules.insert(module.to_string(), ModuleRequirement::Disabled);
                }
                _ => {
                    let entry = modules.entry(module.to_string()).or_insert(req);
                    if *entry != ModuleRequirement::Required && req == ModuleRequirement::Required {
                        *entry = ModuleRequirement::Required;
                    }
                }
            }
        }
        Ok(())
    };

    merge_file("/etc/verdant/default.conf", &mut modules)?;

    let modules_dir = "/etc/verdant/modules.d";
    if let Ok(entries) = fs::read_dir(modules_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("conf") {
                merge_file(path.to_str().unwrap(), &mut modules)?;
            }
        }
    }

    Ok(modules)
}

pub fn load_modules_from_map(modules: &HashMap<String, ModuleRequirement>) -> Result<(), ()> {
    print_info_step("Loading all kernel modules according to merged configs");
    io::stdout().flush().unwrap();

    let len = modules.len();
    let mut count = 0;

    for (module, requirement) in modules {
        count += 1;

        if *requirement == ModuleRequirement::Disabled {
            if count == len {
                print_substep_last(&format!("Skipping disabled module {}", module), &status_skip());
            } else {
                print_substep(&format!("Skipping disabled module {}", module), &status_skip());
            }
            continue;
        }

        // For the last module, pass is_last = true
        let is_last = count == len;

        let load_result = load_module(module, is_last);

        match load_result {
            Ok(()) => {}
            Err(()) => {
                if *requirement == ModuleRequirement::Required {
                    if is_last {
                        print_substep_last(&format!("Required module {} failed to load, aborting.", module), &status_fail());
                    } else {
                        print_substep(&format!("Required module {} failed to load, aborting.", module), &status_fail());
                    }
                    return Err(());
                } else {
                    if is_last {
                        print_substep_last(&format!("Optional module {} failed to load, continuing.", module), &status_fail());
                    } else {
                        print_substep(&format!("Optional module {} failed to load, continuing.", module), &status_fail());
                    }
                }
            }
        }
    }

    Ok(())
}

