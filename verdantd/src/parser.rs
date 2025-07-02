use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;

use bloom::errors::BloomError;
use bloom::log::{ConsoleLogger, FileLogger};
use bloom::status::LogLevel;
use bloom::time::ProcessTimer;

use crate::service_file::{ServiceFile, RestartPolicy};

/// Replace `{key}` placeholders in content using vars map
fn apply_template(content: &str, vars: &HashMap<String, String>) -> String {
    let mut result = content.to_string();
    for (key, val) in vars {
        let placeholder = format!("{{{}}}", key);
        result = result.replace(&placeholder, val);
    }
    result
}

/// Parse .vs service file at `path`, applying template variables from `vars`
pub fn parse_service_file(
    path: &Path,
    vars: &HashMap<String, String>,
    console_logger: &mut dyn ConsoleLogger,
    file_logger: &mut dyn FileLogger,
) -> Result<ServiceFile, BloomError> {
    let timer = ProcessTimer::start();

    // Read entire file content
    let raw_content = fs::read_to_string(path).map_err(BloomError::Io)?;

    // Apply template substitutions
    let content = apply_template(&raw_content, vars);

    // Use BufReader on substituted content bytes
    let reader = BufReader::new(content.as_bytes());

    let mut service = ServiceFile::new(String::new(), String::new());

    let mut current_key: Option<String> = None;

    for line_res in reader.lines() {
        let line = line_res.map_err(BloomError::Io)?;
        let line = line.trim_end();

        // Skip empty lines or comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Handle list values by indentation (2+ spaces or tab)
        if line.starts_with("  ") || line.starts_with('\t') {
            if let Some(ref key) = current_key {
                let val = match key.as_str() {
                    "instances" => {
                        // Explicit type annotation on closure param to satisfy compiler
                        line.trim_start_matches(|c: char| c == '-' || c.is_whitespace()).trim_end()
                    }
                    _ => {
                        line.trim_start_matches('-').trim()
                    }
                };

                match key.as_str() {
                    "env" => {
                        if let Some(ref mut env_vec) = service.env {
                            env_vec.push(val.to_string());
                        } else {
                            service.env = Some(vec![val.to_string()]);
                        }
                    }
                    "dependencies" => {
                        if let Some(ref mut dep_vec) = service.dependencies {
                            dep_vec.push(val.to_string());
                        } else {
                            service.dependencies = Some(vec![val.to_string()]);
                        }
                    }
                    "tags" => {
                        if let Some(ref mut tag_vec) = service.tags {
                            tag_vec.push(val.to_string());
                        } else {
                            service.tags = Some(vec![val.to_string()]);
                        }
                    }
                    "instances" => {
                        if let Some(ref mut inst_vec) = service.instances {
                            inst_vec.push(val.to_string());
                        } else {
                            service.instances = Some(vec![val.to_string()]);
                        }
                    }
                    _ => {
                        // Unknown list key with indentation, ignore or log if you want
                    }
                }
                continue;
            } else {
                // Indented line but no current key set — skip or error
                continue;
            }
        }

        // Parse key: value lines
        if let Some((key, val)) = line.split_once(':') {
            let key = key.trim();
            let val = val.trim();

            current_key = Some(key.to_string());

            match key {
                "name" => service.name = val.to_string(),
                "desc" => service.desc = if val.is_empty() { None } else { Some(val.to_string()) },
                "cmd" => service.cmd = val.to_string(),
                "args" => {
                    if val.is_empty() {
                        service.args = None;
                    } else {
                        let args_vec = val.split_whitespace().map(|s| s.to_string()).collect();
                        service.args = Some(args_vec);
                    }
                }
                "pre-cmd" => service.pre_cmd = if val.is_empty() { None } else { Some(val.to_string()) },
                "post-cmd" => service.post_cmd = if val.is_empty() { None } else { Some(val.to_string()) },
                "package" => service.package = if val.is_empty() { None } else { Some(val.to_string()) },
                "startup-package" => service.startup_package = if val.is_empty() { None } else { Some(val.to_string()) },
                "user" => service.user = if val.is_empty() { None } else { Some(val.to_string()) },
                "group" => service.group = if val.is_empty() { None } else { Some(val.to_string()) },
                "working-dir" => service.working_dir = if val.is_empty() { None } else { Some(val.to_string()) },
                "restart" => {
                    let policy = match val {
                        "on-failure" => RestartPolicy::OnFailure,
                        "always" => RestartPolicy::Always,
                        "never" => RestartPolicy::Never,
                        _ => RestartPolicy::Never,
                    };
                    service.restart = policy;
                }
                "restart-delay" => {
                    if let Ok(delay) = val.parse() {
                        service.restart_delay = Some(delay);
                    }
                }
                "stop-cmd" => service.stop_cmd = if val.is_empty() { None } else { Some(val.to_string()) },
                "timeout-start" => {
                    if let Ok(t) = val.parse() {
                        service.timeout_start = Some(t);
                    }
                }
                "timeout-stop" => {
                    if let Ok(t) = val.parse() {
                        service.timeout_stop = Some(t);
                    }
                }
                "priority" => {
                    if let Ok(p) = val.parse() {
                        service.priority = Some(p);
                    }
                }
                "stdout-log" => service.stdout_log = if val.is_empty() { None } else { Some(val.to_string()) },
                "stderr-log" => service.stderr_log = if val.is_empty() { None } else { Some(val.to_string()) },
                "umask" => service.umask = if val.is_empty() { None } else { Some(val.to_string()) },
                "nice" => {
                    if let Ok(n) = val.parse() {
                        service.nice = Some(n);
                    }
                }
                "env" => service.env = Some(Vec::new()),
                "dependencies" => service.dependencies = Some(Vec::new()),
                "tags" => service.tags = Some(Vec::new()),
                _ => {
                    // Unknown keys ignored or log if needed
                }
            }
        } else {
            // Invalid line without colon — ignore or log if needed
        }
    }

    if service.name.is_empty() {
        let msg = format!("Service file {:?} missing required 'name'", path);
        log_error(console_logger, file_logger, &timer, LogLevel::Fail, &msg);
        return Err(BloomError::Custom(msg));
    }
    if service.cmd.is_empty() {
        let msg = format!("Service file {:?} missing required 'cmd'", path);
        log_error(console_logger, file_logger, &timer, LogLevel::Fail, &msg);
        return Err(BloomError::Custom(msg));
    }

    if service.restart_delay.is_none() {
        service.restart_delay = Some(0);
    }
    if service.timeout_start.is_none() {
        service.timeout_start = Some(10);
    }
    if service.timeout_stop.is_none() {
        service.timeout_stop = Some(5);
    }
    if service.umask.is_none() {
        service.umask = Some("022".to_string());
    }
    if service.nice.is_none() {
        service.nice = Some(0);
    }
    if service.priority.is_none() {
        service.priority = Some(50);
    }
    if service.stdout_log.is_none() {
        service.stdout_log = Some(format!("/var/log/verdant/services/{}.out.log", service.name));
    }
    if service.stderr_log.is_none() {
        service.stderr_log = Some(format!("/var/log/verdant/services/{}.err.log", service.name));
    }

    Ok(service)
}

fn _log_success(
    console_logger: &mut dyn ConsoleLogger,
    file_logger: &mut dyn FileLogger,
    timer: &ProcessTimer,
    level: LogLevel,
    msg: &str,
) {
    let elapsed = timer.elapsed();
    console_logger.message(level, msg, elapsed);
    file_logger.log(level, msg);
}

fn log_error(
    console_logger: &mut dyn ConsoleLogger,
    file_logger: &mut dyn FileLogger,
    timer: &ProcessTimer,
    level: LogLevel,
    msg: &str,
) {
    let elapsed = timer.elapsed();
    console_logger.message(level, msg, elapsed);
    file_logger.log(level, msg);
}

