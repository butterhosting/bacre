//! When a schedule fires: standard cron, read by croner, in the machine's local time.
//!
//! Five fields (minute hour day-of-month month day-of-week), with names (`MON-FRI`, `JAN`),
//! shortcuts (`@daily`) and croner's extras (`L`, `#`). Weekdays are POSIX: 0 to 6 from
//! Sunday, 7 is Sunday too. As in cron itself, when both day fields are restricted a day
//! matches if either does. Seconds and years are refused, so a six-field expression is
//! never quietly read as something else.

use chrono::{DateTime, Local, NaiveDateTime, TimeZone};
use croner::parser::{CronParser, Seconds, Year};

#[derive(Debug, Clone)]
pub struct Cron {
    expression: String,
    cron: croner::Cron,
}

impl Cron {
    pub fn parse(expression: &str) -> Result<Cron, String> {
        let expression = expression.split_whitespace().collect::<Vec<_>>().join(" ");
        let parser = CronParser::builder()
            .seconds(Seconds::Disallowed)
            .year(Year::Disallowed)
            .build();
        let cron = parser
            .parse(&expression)
            .map_err(|e| format!("\"{expression}\" is not a cron expression: {e}"))?;
        Ok(Cron { expression, cron })
    }

    pub fn expression(&self) -> &str {
        &self.expression
    }

    /// The first moment after `after` that the expression fires, or nothing when it never does
    pub fn next(&self, after: &DateTime<Local>) -> Option<DateTime<Local>> {
        self.cron.find_next_occurrence(after, false).ok()
    }

    /// The latest moment at or before `at` that the expression fired, or nothing when it never did
    pub fn previous(&self, at: &DateTime<Local>) -> Option<DateTime<Local>> {
        self.cron.find_previous_occurrence(at, true).ok()
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
                .from_local_datetime(&(wall + chrono::Duration::hours(1)))
                .earliest()
        })
        .unwrap_or_else(|| Local.from_utc_datetime(&wall))
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Local, NaiveDateTime};

    use super::Cron;

    /// A time on the clock here
    fn local(text: &str) -> DateTime<Local> {
        super::local(NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").unwrap())
    }

    /// A result as it shows on the clock here
    fn wall(d: Option<DateTime<Local>>) -> Option<String> {
        d.map(|d| d.naive_local().format("%Y-%m-%d %H:%M").to_string())
    }

    fn cron(expression: &str) -> Cron {
        Cron::parse(expression).unwrap()
    }

    #[test]
    fn should_find_the_next_and_previous_run_of_a_daily_schedule() {
        let cron = cron("0 3 * * *");
        assert_eq!(
            wall(cron.next(&local("2026-10-03 14:20:00"))).as_deref(),
            Some("2026-10-04 03:00")
        );
        assert_eq!(
            wall(cron.previous(&local("2026-10-03 14:20:00"))).as_deref(),
            Some("2026-10-03 03:00")
        );
        assert_eq!(
            wall(cron.previous(&local("2026-10-03 02:59:59"))).as_deref(),
            Some("2026-10-02 03:00")
        );
    }

    #[test]
    fn should_count_the_current_minute_as_fired_and_not_as_next() {
        let cron = cron("5 * * * *");
        assert_eq!(
            wall(cron.previous(&local("2026-10-03 14:05:30"))).as_deref(),
            Some("2026-10-03 14:05")
        );
        assert_eq!(
            wall(cron.next(&local("2026-10-03 14:05:30"))).as_deref(),
            Some("2026-10-03 15:05")
        );
    }

    #[test]
    fn should_understand_steps_ranges_lists_and_names() {
        assert_eq!(
            wall(cron("*/15 * * * *").next(&local("2026-10-03 14:46:00"))).as_deref(),
            Some("2026-10-03 15:00")
        );
        assert_eq!(
            wall(cron("0 8-18/2 * * *").next(&local("2026-10-03 12:30:00"))).as_deref(),
            Some("2026-10-03 14:00")
        );
        assert_eq!(
            wall(cron("0,30 22 * * *").previous(&local("2026-10-03 12:00:00"))).as_deref(),
            Some("2026-10-02 22:30")
        );
        // 2026-10-03 is a Saturday
        assert_eq!(
            wall(cron("0 9 * * MON-FRI").next(&local("2026-10-03 12:00:00"))).as_deref(),
            Some("2026-10-05 09:00")
        );
        assert_eq!(
            wall(cron("@daily").next(&local("2026-10-03 12:00:00"))).as_deref(),
            Some("2026-10-04 00:00")
        );
    }

    #[test]
    fn should_cross_month_and_year_boundaries() {
        assert_eq!(
            wall(cron("0 0 1 1 *").next(&local("2026-10-03 12:00:00"))).as_deref(),
            Some("2027-01-01 00:00")
        );
        assert_eq!(
            wall(cron("0 0 1 * *").previous(&local("2026-10-01 00:00:00"))).as_deref(),
            Some("2026-10-01 00:00")
        );
        assert_eq!(
            wall(cron("30 4 31 * *").next(&local("2026-10-31 05:00:00"))).as_deref(),
            Some("2026-12-31 04:30")
        );
    }

    #[test]
    fn should_treat_day_of_week_like_cron_does() {
        assert_eq!(
            wall(cron("0 17 * * 5").next(&local("2026-10-03 12:00:00"))).as_deref(),
            Some("2026-10-09 17:00")
        );
        assert_eq!(
            wall(cron("0 17 * * 7").next(&local("2026-10-03 12:00:00"))).as_deref(),
            Some("2026-10-04 17:00")
        );
        // both day fields restricted: either may match
        assert_eq!(
            wall(cron("0 0 15 * 1").next(&local("2026-10-03 12:00:00"))).as_deref(),
            Some("2026-10-05 00:00")
        );
    }

    #[test]
    fn should_give_up_on_an_expression_that_never_fires() {
        assert_eq!(cron("0 0 30 2 *").next(&local("2026-10-03 12:00:00")), None);
        assert_eq!(
            cron("0 0 30 2 *").previous(&local("2026-10-03 12:00:00")),
            None
        );
    }

    #[test]
    fn should_reject_what_is_not_a_five_field_cron_expression() {
        for bad in [
            "",
            "daily",
            "0 3 * *",
            "0 3 * * * *",
            "0 3 * * * 2027",
            "60 3 * * *",
            "0 24 * * *",
            "0 3 0 * *",
            "0 3 * 13 *",
            "0 3 * * 8",
            "*/0 * * * *",
            "a * * * *",
        ] {
            assert!(Cron::parse(bad).is_err(), "{bad:?} should not parse");
        }
        assert!(
            Cron::parse("60 3 * * *")
                .unwrap_err()
                .starts_with("\"60 3 * * *\" is not a cron expression: ")
        );
    }

    #[test]
    fn should_keep_the_expression_as_written_less_the_extra_spaces() {
        assert_eq!(cron("  5 *  * * * ").expression(), "5 * * * *");
    }

    /// Both clock changes of 2026 in the Netherlands. They only mean something there, so they
    /// check the zone first: run with TZ=Europe/Amsterdam to be sure they run.
    mod daylight_saving {
        use chrono::{DateTime, Local};

        use super::{cron, local};

        fn amsterdam() -> bool {
            let offset = |text| local(text).offset().local_minus_utc();
            offset("2026-07-01 12:00:00") == 7200 && offset("2026-01-01 12:00:00") == 3600
        }

        fn at(text: &str) -> DateTime<Local> {
            DateTime::parse_from_rfc3339(text)
                .unwrap()
                .with_timezone(&Local)
        }

        #[test]
        fn should_run_a_time_the_spring_change_skips_as_soon_as_the_clock_has_jumped() {
            if !amsterdam() {
                return;
            }
            // 02:00 becomes 03:00 on 29 March: there is no 02:30
            let cron = cron("30 2 * * *");
            assert_eq!(
                cron.next(&at("2026-03-29T01:00:00+01:00")),
                Some(at("2026-03-29T03:00:00+02:00"))
            );
            assert_eq!(
                cron.previous(&at("2026-03-29T12:00:00+02:00")),
                Some(at("2026-03-29T03:00:00+02:00"))
            );
            assert_eq!(
                cron.next(&at("2026-03-29T03:00:00+02:00")),
                Some(at("2026-03-30T02:30:00+02:00"))
            );
        }

        #[test]
        fn should_run_once_through_the_hour_the_autumn_change_repeats() {
            if !amsterdam() {
                return;
            }
            // 03:00 becomes 02:00 on 25 October: daily at 03:00 runs once that day
            let cron = cron("0 3 * * *");
            assert_eq!(
                cron.next(&at("2026-10-25T01:00:00+02:00")),
                Some(at("2026-10-25T03:00:00+01:00"))
            );
            assert_eq!(
                cron.next(&at("2026-10-25T03:00:00+01:00")),
                Some(at("2026-10-26T03:00:00+01:00"))
            );
            assert_eq!(
                cron.previous(&at("2026-10-25T12:00:00+01:00")),
                Some(at("2026-10-25T03:00:00+01:00"))
            );
        }
    }
}
