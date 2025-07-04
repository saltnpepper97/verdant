use std::fs::File;
use std::io::{BufRead, BufReader};

use crate::service::{Service, StartupPackage, RestartPolicy};
use bloom::status::ServiceState;
use bloom::errors::BloomError;

fn parse_quoted_args(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut chars = s.chars().peekable();
    let mut in_double_quotes = false;
    let mut in_single_quotes = false;

    while let Some(&ch) = chars.peek() {
        match ch {
            '"' if !in_single_quotes => {
                chars.next();
                if in_double_quotes {
                    in_double_quotes = false;
                    args.push(current.clone());
                    current.clear();
                } else {
                    in_double_quotes = true;
                }
            }
            '\'' if !in_double_quotes => {
                chars.next();
                if in_single_quotes {
                    in_single_quotes = false;
                    args.push(current.clone());
                    current.clear();
                } else {
                    in_single_quotes = true;
                }
            }
            ch if ch.is_whitespace() && !in_double_quotes && !in_single_quotes => {
                chars.next();
                if !current.is_empty() {
                    args.push(current.clone());
                    current.clear();
                }
            }
            _ => {
                current.push(ch);
                chars.next();
            }
        }
    }

    if !current.is_empty() {
        args.push(current);
    }

    args
}

pub fn parse_service_file(path: &str) -> Result<Vec<Service>, BloomError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut name = None;
    let mut desc = None;
    let mut cmd = None;
    let mut args = Vec::new();
    let mut startup = None;
    let mut restart = None;
    let mut tags = Vec::new();
    let mut instances = Vec::new();
    let mut stdout: Option<String> = None;
    let mut stderr: Option<String> = None;
    let mut in_instance_block = false;

    for line in reader.lines() {
        let line = line?;
        let line = line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with("instances:") {
            in_instance_block = true;
            continue;
        }

        if in_instance_block {
            if line.starts_with('-') {
                let value = line.trim_start_matches('-').trim().to_string();
                if !value.is_empty() {
                    instances.push(value);
                }
                continue;
            } else {
                in_instance_block = false;
            }
        }

        if let Some((key, val)) = line.split_once(':') {
            let key = key.trim();
            let val = val.trim();

            match key {
                "name" => name = Some(val.to_string()),
                "desc" => desc = Some(val.to_string()),
                "cmd" => cmd = Some(val.to_string()),
                "args" => args = parse_quoted_args(val),
                "startup" => startup = StartupPackage::from_str(val),
                "restart" => restart = RestartPolicy::from_str(val),
                "tags" => tags = val.split(',').map(|s| s.trim().to_string()).collect(),
                "stdout" => stdout = Some(val.to_string()),
                "stderr" => stderr = Some(val.to_string()),

                _ => return Err(BloomError::Parse(format!("Unknown key: {key}"))),
            }
        }
    }

    let name = name.ok_or_else(|| BloomError::Parse("Missing name".into()))?;
    let cmd = cmd.ok_or_else(|| BloomError::Parse("Missing cmd".into()))?;

    let base = Service {
        name,
        desc: desc.unwrap_or_default(),
        cmd,
        args,
        startup: startup.unwrap_or(StartupPackage::Custom),
        restart: restart.unwrap_or(RestartPolicy::Never),
        tags,
        instances: vec![],
        state: ServiceState::Stopped,
        stdout,
        stderr,
    };

    // If instances were defined, create one service per instance with `{}` replaced
    if !instances.is_empty() {
        let mut expanded = Vec::new();
        for inst in instances {
            let svc = Service {
                name: base.name.replace("{}", &inst),
                desc: base.desc.replace("{}", &inst),
                cmd: base.cmd.replace("{}", &inst),
                args: base.args.iter().map(|a| a.replace("{}", &inst)).collect(),
                stdout: base.stdout.as_ref().map(|s| s.replace("{}", &inst)),
                stderr: base.stderr.as_ref().map(|s| s.replace("{}", &inst)),
                instances: vec![inst.clone()],
                ..base.clone()
            };
            expanded.push(svc);
        }
        Ok(expanded)
    } else {
        Ok(vec![base])
    }
}

