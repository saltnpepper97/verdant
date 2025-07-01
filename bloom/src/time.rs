use std::time::{Duration, Instant};

/// Tracks overall elapsed time since system start.
pub struct SystemTimer {
    start: Instant,
}

impl SystemTimer {
    /// Create a new timer starting now.
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// Returns elapsed time since timer creation.
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// Format elapsed time for console logging, e.g. `[ 00:01:23 ]`
    pub fn format_elapsed(&self) -> String {
        format_duration(self.elapsed())
    }
}

/// Measure the duration of a process or task.
/// Usage:
/// ```
/// let timer = ProcessTimer::start();
/// // do some work...
/// let elapsed = timer.elapsed();
/// println!("Process took {}", format_duration(elapsed));
/// ```
pub struct ProcessTimer {
    start: Instant,
}

impl ProcessTimer {
    /// Start timing a process.
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// Returns how long the process took so far.
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

/// Format a Duration into a string like `[ 00:01:23 ]` (mm:ss:ms)
pub fn format_duration(duration: Duration) -> String {
    let mins = duration.as_secs() / 60;
    let secs = duration.as_secs() % 60;
    let millis = duration.subsec_millis();

    format!("[ {:02}:{:02}:{:03} ]", mins, secs, millis)
}
