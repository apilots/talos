//! Adapt the configured IANA calendar to the library's public rotation boundary.
//! rolling-file owns size accounting, file switching, archive naming and retention.
use crate::LogRotation;
use chrono::{DateTime, Local, NaiveDate};
use chrono_tz::Tz;
use rolling_file::{RollingCondition, RollingConditionBasic, RollingFileAppender};
use std::{fs, io, path::Path};

pub(super) struct CalendarOrSize {
    timezone: Tz,
    last_day: Option<NaiveDate>,
    size: RollingConditionBasic,
}

impl RollingCondition for CalendarOrSize {
    fn should_rollover(&mut self, now: &DateTime<Local>, current_filesize: u64) -> bool {
        let day = now.with_timezone(&self.timezone).date_naive();
        let calendar_changed = self.last_day.replace(day).is_some_and(|last| last != day);
        let size_reached = self.size.should_rollover(now, current_filesize);
        calendar_changed || size_reached
    }
}

pub(super) fn open(
    path: &Path,
    limits: &LogRotation,
    timezone: Tz,
) -> io::Result<RollingFileAppender<CalendarOrSize>> {
    let last_day = match fs::metadata(path) {
        Ok(metadata) if metadata.len() > 0 => Some(
            DateTime::<chrono::Utc>::from(metadata.modified()?)
                .with_timezone(&timezone)
                .date_naive(),
        ),
        Ok(_) => None,
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    RollingFileAppender::new(
        path,
        CalendarOrSize {
            timezone,
            last_day,
            size: RollingConditionBasic::new().max_size(limits.max_bytes),
        },
        limits.max_files - 1,
    )
}

#[cfg(test)]
#[path = "rotation_condition_tests.rs"]
mod tests;
