//! Timestamped stderr logger. launchd sends stderr to /var/log/macdetour.log.
//! The userspace TCP stack is chatty at info level, so it is held to warnings.

use log::{Level, LevelFilter, Log, Metadata, Record};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

struct Logger;

impl Log for Logger {
    fn enabled(&self, meta: &Metadata<'_>) -> bool {
        let quiet = meta.target().starts_with("tun2proxy") || meta.target().starts_with("ipstack");
        meta.level() <= if quiet { Level::Warn } else { Level::Info }
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let mut err = std::io::stderr().lock();
        let _ = writeln!(err, "{} {:<5} {}", timestamp(secs), record.level(), record.args());
    }

    fn flush(&self) {}
}

/// `YYYY-MM-DDTHH:MM:SSZ` without pulling in a date crate.
pub fn timestamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

pub fn init() {
    if log::set_boxed_logger(Box::new(Logger)).is_ok() {
        log::set_max_level(LevelFilter::Info);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats_utc_timestamps() {
        assert_eq!(super::timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(super::timestamp(1_760_112_000), "2025-10-10T16:00:00Z");
        assert_eq!(super::timestamp(951_782_400), "2000-02-29T00:00:00Z");
    }
}
