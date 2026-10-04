//! A five-field cron expression (minute hour day-of-month month day-of-week), read in the
//! machine's local time. Fields take `*`, numbers, ranges (`1-5`), lists (`0,30`) and steps
//! (`0-59/15`, `8-18/2`, or a star in place of the range). As in cron itself, when both day
//! fields are restricted a day matches if either does.
//!
//! The arithmetic is done on wall-clock time; `local` turns a result into a moment.

use std::collections::BTreeSet;

use chrono::{DateTime, Datelike, Duration, Local, NaiveDateTime, NaiveTime, TimeZone, Timelike};

/// How far `next` and `previous` look before deciding an expression never fires (30 February)
const HORIZON_DAYS: i64 = 5 * 366;

#[derive(Debug, Clone)]
pub struct Cron {
    expression: String,
    minutes: BTreeSet<u32>,
    hours: BTreeSet<u32>,
    days_of_month: BTreeSet<u32>,
    months: BTreeSet<u32>,
    days_of_week: BTreeSet<u32>,
    any_day_of_month: bool,
    any_day_of_week: bool,
}

impl Cron {
    /// Fails with a message that names the offending field
    pub fn parse(expression: &str) -> Result<Cron, String> {
        let fields: Vec<&str> = expression.split_whitespace().collect();
        let [minute, hour, day_of_month, month, day_of_week] = fields[..] else {
            return Err(format!(
                "a cron expression has five fields (minute hour day-of-month month day-of-week), got {}",
                fields.len()
            ));
        };
        Ok(Cron {
            expression: fields.join(" "),
            minutes: field("minute", minute, 0, 59)?,
            hours: field("hour", hour, 0, 23)?,
            days_of_month: field("day-of-month", day_of_month, 1, 31)?,
            months: field("month", month, 1, 12)?,
            // 7 is Sunday too
            days_of_week: field("day-of-week", day_of_week, 0, 7)?
                .into_iter()
                .map(|day| day % 7)
                .collect(),
            any_day_of_month: day_of_month.starts_with('*'),
            any_day_of_week: day_of_week.starts_with('*'),
        })
    }

    pub fn expression(&self) -> &str {
        &self.expression
    }

    /// The first moment after `after` that the expression fires, or nothing when it never does
    pub fn next(&self, after: NaiveDateTime) -> Option<NaiveDateTime> {
        let limit = after + Duration::days(HORIZON_DAYS);
        let mut d = minute_of(after) + Duration::minutes(1);
        while d <= limit {
            if !self.day_matches(d) {
                d = (d.date() + Duration::days(1)).and_time(NaiveTime::MIN);
            } else if !self.hours.contains(&d.hour()) {
                d = hour_of(d) + Duration::hours(1);
            } else if !self.minutes.contains(&d.minute()) {
                d += Duration::minutes(1);
            } else {
                return Some(d);
            }
        }
        None
    }

    /// The latest moment at or before `at` that the expression fired, or nothing when it never did
    pub fn previous(&self, at: NaiveDateTime) -> Option<NaiveDateTime> {
        let limit = at - Duration::days(HORIZON_DAYS);
        let mut d = minute_of(at);
        while d >= limit {
            if !self.day_matches(d) {
                d = d.date().and_time(NaiveTime::MIN) - Duration::minutes(1);
            } else if !self.hours.contains(&d.hour()) {
                d = hour_of(d) - Duration::minutes(1);
            } else if !self.minutes.contains(&d.minute()) {
                d -= Duration::minutes(1);
            } else {
                return Some(d);
            }
        }
        None
    }

    fn day_matches(&self, d: NaiveDateTime) -> bool {
        if !self.months.contains(&d.month()) {
            return false;
        }
        let day_of_month = self.days_of_month.contains(&d.day());
        let day_of_week = self
            .days_of_week
            .contains(&d.weekday().num_days_from_sunday());
        if self.any_day_of_month || self.any_day_of_week {
            day_of_month && day_of_week
        } else {
            day_of_month || day_of_week
        }
    }
}

/// The moment a wall-clock time stands for here. An hour that a clock change skips is
/// taken as the hour after it, which is when a clock on the wall would next show a time.
pub fn local(wall: NaiveDateTime) -> DateTime<Local> {
    Local
        .from_local_datetime(&wall)
        .earliest()
        .or_else(|| {
            Local
                .from_local_datetime(&(wall + Duration::hours(1)))
                .earliest()
        })
        .unwrap_or_else(|| Local.from_utc_datetime(&wall))
}

fn minute_of(d: NaiveDateTime) -> NaiveDateTime {
    d.date()
        .and_hms_opt(d.hour(), d.minute(), 0)
        .expect("a valid time")
}

fn hour_of(d: NaiveDateTime) -> NaiveDateTime {
    d.date().and_hms_opt(d.hour(), 0, 0).expect("a valid time")
}

/// The values one field allows
fn field(name: &str, text: &str, min: u32, max: u32) -> Result<BTreeSet<u32>, String> {
    let malformed = || format!("the {name} field \"{text}\" is not a number, range, list or step");
    let outside = || format!("the {name} field \"{text}\" is outside {min}-{max}");
    let number = |digits: &str| -> Result<u32, String> {
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(malformed());
        }
        digits.parse().map_err(|_| outside())
    };

    let mut values = BTreeSet::new();
    for part in text.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((range, step)) => (range, Some(number(step)?)),
            None => (part, None),
        };
        let (from, to) = if range == "*" {
            (min, max)
        } else if let Some((from, to)) = range.split_once('-') {
            (number(from)?, number(to)?)
        } else {
            let from = number(range)?;
            // `5/15` means from 5 onwards
            (from, if step.is_some() { max } else { from })
        };
        let step = step.unwrap_or(1);
        if step < 1 || from < min || to > max || from > to {
            return Err(outside());
        }
        values.extend((from..=to).step_by(step as usize));
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDateTime;

    use super::Cron;

    fn at(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S").unwrap()
    }

    fn show(d: Option<NaiveDateTime>) -> Option<String> {
        d.map(|d| d.format("%Y-%m-%d %H:%M").to_string())
    }

    fn cron(expression: &str) -> Cron {
        Cron::parse(expression).unwrap()
    }

    #[test]
    fn should_find_the_next_and_previous_run_of_a_daily_schedule() {
        let cron = cron("0 3 * * *");
        assert_eq!(
            show(cron.next(at("2026-10-03T14:20:00"))).as_deref(),
            Some("2026-10-04 03:00")
        );
        assert_eq!(
            show(cron.previous(at("2026-10-03T14:20:00"))).as_deref(),
            Some("2026-10-03 03:00")
        );
        assert_eq!(
            show(cron.previous(at("2026-10-03T02:59:59"))).as_deref(),
            Some("2026-10-02 03:00")
        );
    }

    #[test]
    fn should_count_the_current_minute_as_fired_and_not_as_next() {
        let cron = cron("5 * * * *");
        assert_eq!(
            show(cron.previous(at("2026-10-03T14:05:30"))).as_deref(),
            Some("2026-10-03 14:05")
        );
        assert_eq!(
            show(cron.next(at("2026-10-03T14:05:30"))).as_deref(),
            Some("2026-10-03 15:05")
        );
    }

    #[test]
    fn should_understand_steps_ranges_and_lists() {
        assert_eq!(
            show(cron("*/15 * * * *").next(at("2026-10-03T14:46:00"))).as_deref(),
            Some("2026-10-03 15:00")
        );
        assert_eq!(
            show(cron("0 8-18/2 * * *").next(at("2026-10-03T12:30:00"))).as_deref(),
            Some("2026-10-03 14:00")
        );
        assert_eq!(
            show(cron("0,30 22 * * *").previous(at("2026-10-03T12:00:00"))).as_deref(),
            Some("2026-10-02 22:30")
        );
    }

    #[test]
    fn should_cross_month_and_year_boundaries() {
        assert_eq!(
            show(cron("0 0 1 1 *").next(at("2026-10-03T12:00:00"))).as_deref(),
            Some("2027-01-01 00:00")
        );
        assert_eq!(
            show(cron("0 0 1 * *").previous(at("2026-10-01T00:00:00"))).as_deref(),
            Some("2026-10-01 00:00")
        );
        assert_eq!(
            show(cron("30 4 31 * *").next(at("2026-10-31T05:00:00"))).as_deref(),
            Some("2026-12-31 04:30")
        );
    }

    #[test]
    fn should_treat_day_of_week_like_cron_does() {
        // 2026-10-03 is a Saturday
        assert_eq!(
            show(cron("0 17 * * 5").next(at("2026-10-03T12:00:00"))).as_deref(),
            Some("2026-10-09 17:00")
        );
        assert_eq!(
            show(cron("0 17 * * 7").next(at("2026-10-03T12:00:00"))).as_deref(),
            Some("2026-10-04 17:00")
        );
        // both day fields restricted: either may match
        assert_eq!(
            show(cron("0 0 15 * 1").next(at("2026-10-03T12:00:00"))).as_deref(),
            Some("2026-10-05 00:00")
        );
    }

    #[test]
    fn should_give_up_on_an_expression_that_never_fires() {
        assert_eq!(cron("0 0 30 2 *").next(at("2026-10-03T12:00:00")), None);
        assert_eq!(cron("0 0 30 2 *").previous(at("2026-10-03T12:00:00")), None);
    }

    #[test]
    fn should_reject_what_is_not_a_cron_expression() {
        for bad in [
            "",
            "daily",
            "0 3 * *",
            "0 3 * * * *",
            "60 3 * * *",
            "0 24 * * *",
            "0 3 0 * *",
            "0 3 * 13 *",
            "0 3 * * 8",
            "5-1 * * * *",
            "*/0 * * * *",
            "a * * * *",
        ] {
            assert!(Cron::parse(bad).is_err(), "{bad:?} should not parse");
        }
    }
}
