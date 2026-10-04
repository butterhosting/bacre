//! restic's `forget` rules, for the backends that prune themselves. Newest first, a snapshot is
//! kept when it is the first one seen in a bucket (an hour, a day, …) while that rule still has
//! buckets to fill. As in restic, the oldest snapshot is also kept while any rule has some left.

use chrono::{Datelike, NaiveDateTime, Timelike};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Retention {
    pub keep_last: u32,
    pub keep_hourly: u32,
    pub keep_daily: u32,
    pub keep_weekly: u32,
    pub keep_monthly: u32,
}

impl Retention {
    pub fn keeps_anything(&self) -> bool {
        self.keep_last + self.keep_hourly + self.keep_daily + self.keep_weekly + self.keep_monthly
            > 0
    }

    /// Which of the snapshots to keep, in the order given; times are on the local clock
    pub fn keep(&self, times: &[NaiveDateTime]) -> Vec<bool> {
        let mut keep = vec![false; times.len()];
        if !self.keeps_anything() {
            // a policy that keeps nothing is refused when the bacre.yaml is read; never delete everything
            return vec![true; times.len()];
        }
        let mut newest_first: Vec<usize> = (0..times.len()).collect();
        newest_first.sort_by(|a, b| times[*b].cmp(&times[*a]));

        type Bucket = fn(usize, NaiveDateTime) -> i64;
        let rules: [(u32, Bucket); 5] = [
            (self.keep_last, |nr, _| nr as i64),
            (self.keep_hourly, |_, t| {
                i64::from(t.year()) * 1_000_000
                    + i64::from(t.month() * 10_000 + t.day() * 100 + t.hour())
            }),
            (self.keep_daily, |_, t| {
                i64::from(t.year()) * 10_000 + i64::from(t.month() * 100 + t.day())
            }),
            (self.keep_weekly, |_, t| {
                let week = t.iso_week();
                i64::from(week.year()) * 100 + i64::from(week.week())
            }),
            (self.keep_monthly, |_, t| {
                i64::from(t.year()) * 100 + i64::from(t.month())
            }),
        ];
        let mut left: Vec<u32> = rules.iter().map(|(count, _)| *count).collect();
        let mut last: Vec<Option<i64>> = vec![None; rules.len()];

        for (nr, &index) in newest_first.iter().enumerate() {
            let oldest = nr == newest_first.len() - 1;
            for (rule, (_, bucket)) in rules.iter().enumerate() {
                if left[rule] == 0 {
                    continue;
                }
                let value = bucket(nr, times[index]);
                if last[rule] != Some(value) || oldest {
                    keep[index] = true;
                    last[rule] = Some(value);
                    left[rule] -= 1;
                }
            }
        }
        keep
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, NaiveDateTime};

    use super::Retention;

    fn at(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").unwrap()
    }

    /// Hourly snapshots, newest first, from `newest` back `count` hours
    fn hourly(newest: &str, count: i64) -> Vec<NaiveDateTime> {
        (0..count)
            .map(|hours| at(newest) - Duration::hours(hours))
            .collect()
    }

    fn kept(retention: Retention, times: &[NaiveDateTime]) -> Vec<String> {
        let keep = retention.keep(times);
        times
            .iter()
            .zip(keep)
            .filter(|(_, keep)| *keep)
            .map(|(time, _)| time.format("%m-%d %H:%M").to_string())
            .collect()
    }

    #[test]
    fn should_keep_the_newest_ones_by_count() {
        let times = hourly("2026-10-04 12:05", 10);
        assert_eq!(
            kept(
                Retention {
                    keep_last: 3,
                    ..Default::default()
                },
                &times
            ),
            vec!["10-04 12:05", "10-04 11:05", "10-04 10:05"]
        );
    }

    #[test]
    fn should_keep_one_per_day_and_the_oldest_while_days_are_left() {
        // three days of hourly snapshots, and room for five days
        let times = hourly("2026-10-04 12:05", 60);
        assert_eq!(
            kept(
                Retention {
                    keep_daily: 5,
                    ..Default::default()
                },
                &times
            ),
            vec!["10-04 12:05", "10-03 23:05", "10-02 23:05", "10-02 01:05"]
        );
    }

    #[test]
    fn should_combine_rules_without_counting_a_snapshot_twice_against_the_same_rule() {
        let times = hourly("2026-10-04 12:05", 60);
        assert_eq!(
            kept(
                Retention {
                    keep_last: 2,
                    keep_hourly: 4,
                    keep_daily: 2,
                    ..Default::default()
                },
                &times
            ),
            vec![
                "10-04 12:05",
                "10-04 11:05",
                "10-04 10:05",
                "10-04 09:05",
                "10-03 23:05"
            ]
        );
    }

    #[test]
    fn should_bucket_weeks_by_iso_week_and_months_by_month() {
        let times: Vec<NaiveDateTime> = (0..70)
            .map(|days| at("2026-10-04 03:01") - Duration::days(days))
            .collect();
        // 2026-10-04 is a Sunday: the last day of ISO week 40
        assert_eq!(
            kept(
                Retention {
                    keep_weekly: 2,
                    ..Default::default()
                },
                &times
            ),
            vec!["10-04 03:01", "09-27 03:01"]
        );
        assert_eq!(
            kept(
                Retention {
                    keep_monthly: 3,
                    ..Default::default()
                },
                &times
            ),
            vec!["10-04 03:01", "09-30 03:01", "08-31 03:01"]
        );
    }

    #[test]
    fn should_not_care_in_which_order_the_snapshots_come() {
        let mut times = hourly("2026-10-04 12:05", 5);
        times.reverse();
        let retention = Retention {
            keep_last: 2,
            ..Default::default()
        };
        assert_eq!(
            retention.keep(&times),
            vec![false, false, false, true, true]
        );
    }

    #[test]
    fn should_delete_nothing_under_a_policy_that_keeps_nothing() {
        let times = hourly("2026-10-04 12:05", 3);
        assert_eq!(Retention::default().keep(&times), vec![true, true, true]);
    }
}
