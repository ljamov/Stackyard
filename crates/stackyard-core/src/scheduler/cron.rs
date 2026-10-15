//! Schedules in the five-field form crontab uses, which most developers
//! already know: `minute hour day month weekday`, e.g. `*/5 * * * *` or
//! `30 2 * * mon-fri`, plus shortcuts like `@daily`.
//!
//! Each field takes `*`, a number, a range `a-b`, any of those with a step
//! (`*/15`, `0-30/10`), or a comma-separated list of them. Months and
//! weekdays also take their English names (`jan`, `mon`); weekday 7 is
//! Sunday, like 0. When both day and weekday are given, a day matching
//! either one counts, as in crontab.

use super::clock::Moment;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    minutes: u64,
    hours: u32,
    days: u32,
    months: u16,
    weekdays: u8,
    /// The day field starts with `*` (not restricted).
    any_day: bool,
    /// The weekday field starts with `*`.
    any_weekday: bool,
}

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];
const WEEKDAYS: [&str; 7] = ["sun", "mon", "tue", "wed", "thu", "fri", "sat"];
const WEEKDAY_NAMES: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// How far ahead [`Schedule::next_runs`] looks, in hours: just over four
/// years, so 29 February is always found.
const SEARCH_HOURS: i64 = 4 * 366 * 24 + 24;

impl Schedule {
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        let expanded = match text.to_ascii_lowercase().as_str() {
            "@yearly" | "@annually" => "0 0 1 1 *",
            "@monthly" => "0 0 1 * *",
            "@weekly" => "0 0 * * 0",
            "@daily" | "@midnight" => "0 0 * * *",
            "@hourly" => "0 * * * *",
            other if other.starts_with('@') => {
                return Err(format!(
                    "{text} is not a shortcut; use @hourly, @daily, @weekly, @monthly or @yearly"
                ))
            }
            _ => text,
        };
        let fields: Vec<&str> = expanded.split_whitespace().collect();
        let [minute, hour, day, month, weekday] = fields[..] else {
            return Err(
                "a schedule has five parts: minute hour day month weekday, e.g. `*/5 * * * *`"
                    .into(),
            );
        };
        let weekdays = field(weekday, "weekday", 0, 7, &WEEKDAYS)?;
        Ok(Self {
            minutes: field(minute, "minute", 0, 59, &[])?,
            hours: field(hour, "hour", 0, 23, &[])? as u32,
            days: field(day, "day", 1, 31, &[])? as u32,
            months: field(month, "month", 1, 12, &MONTHS)? as u16,
            // 7 is Sunday too.
            weekdays: ((weekdays | weekdays >> 7) & 0x7f) as u8,
            any_day: day.starts_with('*'),
            any_weekday: weekday.starts_with('*'),
        })
    }

    pub fn matches(&self, m: &Moment) -> bool {
        self.minutes & (1 << m.minute) != 0
            && self.hours & (1 << m.hour) != 0
            && self.day_matches(m)
    }

    fn day_matches(&self, m: &Moment) -> bool {
        let day = self.days & (1 << m.day) != 0;
        let weekday = self.weekdays & (1 << m.weekday) != 0;
        let day_ok = match (self.any_day, self.any_weekday) {
            (true, true) => true,
            (true, false) => weekday,
            (false, true) => day,
            (false, false) => day || weekday,
        };
        day_ok && self.months & (1 << m.month) != 0
    }

    /// The next `count` minutes (as seconds since 1970) after `after` that
    /// match, reading the calendar with `calendar` (e.g. [`super::clock::local`]).
    pub fn next_runs(
        &self,
        after: i64,
        count: usize,
        calendar: impl Fn(i64) -> Moment,
    ) -> Vec<i64> {
        let mut runs = Vec::new();
        let mut at = (after.div_euclid(60) + 1) * 60;
        let end = at + SEARCH_HOURS * 3600;
        while runs.len() < count && at < end {
            let m = calendar(at);
            if !self.day_matches(&m) || self.hours & (1 << m.hour) == 0 {
                // Nothing this hour: skip to the next one.
                at += i64::from(60 - m.minute) * 60;
                continue;
            }
            if self.minutes & (1 << m.minute) != 0 {
                runs.push(at);
            }
            at += 60;
        }
        runs
    }
}

/// A plain-English reading of common schedules, e.g. "Every day at 02:30";
/// `None` for the rest, which are shown as written.
pub fn describe(text: &str) -> Option<String> {
    let schedule = Schedule::parse(text).ok()?;
    let expanded = match text.trim().to_ascii_lowercase().as_str() {
        "@yearly" | "@annually" => return Some("Every year on 1 January at 00:00".into()),
        "@monthly" => return Some("On the 1st of every month at 00:00".into()),
        "@weekly" => return Some("Every Sunday at 00:00".into()),
        "@daily" | "@midnight" => return Some("Every day at 00:00".into()),
        "@hourly" => return Some("Every hour, on the hour".into()),
        other => other.to_string(),
    };
    let fields: Vec<&str> = expanded.split_whitespace().collect();
    let [minute, hour, day, month, weekday] = fields[..] else {
        return None;
    };
    let number = |s: &str| s.parse::<u32>().ok();
    let step = |s: &str| s.strip_prefix("*/").and_then(number);
    if month != "*" {
        return None;
    }
    let daily = day == "*" && weekday == "*";
    match (number(minute), hour) {
        (None, "*") if daily && minute == "*" => Some("Every minute".into()),
        (None, "*") if daily => step(minute).map(|n| format!("Every {n} minutes")),
        (Some(0), "*") if daily => Some("Every hour, on the hour".into()),
        (Some(m), "*") if daily => Some(format!("Every hour at {m} minutes past")),
        (Some(m), _) => {
            if let Some(n) = step(hour).filter(|_| daily) {
                return Some(format!("Every {n} hours at {m:02} minutes past"));
            }
            let h = number(hour)?;
            let time = format!("{h:02}:{m:02}");
            if daily {
                return Some(format!("Every day at {time}"));
            }
            if day == "*" {
                let days: Vec<u32> = (0..7)
                    .filter(|d| schedule.weekdays & (1 << d) != 0)
                    .collect();
                let names = match days[..] {
                    [1, 2, 3, 4, 5] => "weekday".to_string(),
                    [0, 6] => "Saturday and Sunday".to_string(),
                    _ => {
                        let names: Vec<&str> =
                            days.iter().map(|d| WEEKDAY_NAMES[*d as usize]).collect();
                        names.join(", ")
                    }
                };
                return Some(format!("Every {names} at {time}"));
            }
            let d = number(day).filter(|_| weekday == "*")?;
            let suffix = match d {
                1 | 21 | 31 => "st",
                2 | 22 => "nd",
                3 | 23 => "rd",
                _ => "th",
            };
            Some(format!("On the {d}{suffix} of every month at {time}"))
        }
        _ => None,
    }
}

/// One field as a bit set: bit `n` is set when value `n` matches.
fn field(text: &str, what: &str, min: u32, max: u32, names: &[&str]) -> Result<u64, String> {
    let value = |s: &str| -> Result<u32, String> {
        let lower = s.to_ascii_lowercase();
        if let Some(i) = names.iter().position(|n| *n == lower) {
            // Month names count from 1, weekday names from 0.
            return Ok(i as u32 + min);
        }
        let n: u32 = s
            .parse()
            .map_err(|_| format!("{what}: `{s}` is not a number"))?;
        if n < min || n > max {
            return Err(format!("{what}: {n} is outside {min}-{max}"));
        }
        Ok(n)
    };
    let mut bits = 0u64;
    for item in text.split(',') {
        let (range, step) = match item.split_once('/') {
            Some((range, step)) => {
                let step: u32 = step.parse().ok().filter(|s| *s > 0).ok_or_else(|| {
                    format!("{what}: `{step}` is not a step; use a number above 0")
                })?;
                (range, step)
            }
            None => (item, 1),
        };
        let (from, to) = match range {
            "*" => (min, max),
            _ => match range.split_once('-') {
                Some((a, b)) => (value(a)?, value(b)?),
                // `5/15` means from 5 to the end, every 15.
                None if step > 1 => (value(range)?, max),
                None => {
                    let n = value(range)?;
                    (n, n)
                }
            },
        };
        if from > to {
            return Err(format!("{what}: {range} runs backwards"));
        }
        for n in (from..=to).step_by(step as usize) {
            bits |= 1 << n;
        }
    }
    Ok(bits)
}

#[cfg(test)]
mod tests {
    use super::super::clock::utc;
    use super::*;

    fn moment(month: u32, day: u32, hour: u32, minute: u32, weekday: u32) -> Moment {
        Moment {
            year: 2026,
            month,
            day,
            hour,
            minute,
            weekday,
        }
    }

    #[test]
    fn parses_fields() {
        let s = Schedule::parse("*/15 9-17 * * mon-fri").unwrap();
        assert!(s.matches(&moment(10, 14, 9, 45, 3)));
        assert!(!s.matches(&moment(10, 14, 9, 46, 3)));
        assert!(!s.matches(&moment(10, 14, 18, 0, 3)));
        assert!(!s.matches(&moment(10, 18, 10, 0, 0))); // Sunday

        let s = Schedule::parse("0,30 2 1 JAN,jul *").unwrap();
        assert!(s.matches(&moment(7, 1, 2, 30, 3)));
        assert!(!s.matches(&moment(8, 1, 2, 30, 6)));

        // 7 is Sunday too; ranges with steps; `n/step` runs to the end.
        assert!(Schedule::parse("0 0 * * 7")
            .unwrap()
            .matches(&moment(10, 18, 0, 0, 0)));
        let s = Schedule::parse("10-40/10 5/6 * * *").unwrap();
        assert!(s.matches(&moment(1, 1, 11, 40, 0)));
        assert!(s.matches(&moment(1, 1, 23, 10, 0)));
        assert!(!s.matches(&moment(1, 1, 12, 10, 0)));
        assert!(!s.matches(&moment(1, 1, 11, 50, 0)));
    }

    #[test]
    fn day_or_weekday_like_crontab() {
        // The 13th, or any Friday.
        let s = Schedule::parse("0 0 13 * fri").unwrap();
        assert!(s.matches(&moment(10, 13, 0, 0, 2)));
        assert!(s.matches(&moment(10, 16, 0, 0, 5)));
        assert!(!s.matches(&moment(10, 14, 0, 0, 3)));
        // With `*` for day only the weekday counts.
        let s = Schedule::parse("0 0 * * fri").unwrap();
        assert!(!s.matches(&moment(10, 13, 0, 0, 2)));
    }

    #[test]
    fn shortcuts() {
        let daily = Schedule::parse("@daily").unwrap();
        assert_eq!(daily, Schedule::parse("0 0 * * *").unwrap());
        assert_eq!(
            Schedule::parse("@WEEKLY").unwrap(),
            Schedule::parse("0 0 * * sun").unwrap()
        );
        assert!(Schedule::parse("@reboot").is_err());
    }

    #[test]
    fn explains_mistakes() {
        let err = |s| Schedule::parse(s).unwrap_err();
        assert!(err("* * * *").contains("five parts"));
        assert_eq!(err("60 * * * *"), "minute: 60 is outside 0-59");
        assert_eq!(err("* * 0 * *"), "day: 0 is outside 1-31");
        assert_eq!(err("* * * foo *"), "month: `foo` is not a number");
        assert!(err("*/0 * * * *").contains("not a step"));
        assert_eq!(err("* 5-2 * * *"), "hour: 5-2 runs backwards");
        assert!(err("").contains("five parts"));
    }

    #[test]
    fn finds_the_next_runs() {
        // 2026-10-14 22:05:09 UTC, a Wednesday.
        let now = 1_792_015_509;
        let every5 = Schedule::parse("*/5 * * * *").unwrap();
        let runs = every5.next_runs(now, 3, utc);
        assert_eq!(
            runs,
            [now - 9 + 5 * 60, now - 9 + 10 * 60, now - 9 + 15 * 60]
        );
        assert_eq!(utc(runs[0]).minute, 10);

        // Next Monday at 09:00: 2026-10-19.
        let monday = Schedule::parse("0 9 * * mon").unwrap();
        let run = utc(monday.next_runs(now, 1, utc)[0]);
        assert_eq!((run.month, run.day, run.hour, run.minute), (10, 19, 9, 0));

        // 29 February: 2028.
        let leap = Schedule::parse("0 0 29 2 *").unwrap();
        let run = utc(leap.next_runs(now, 1, utc)[0]);
        assert_eq!((run.year, run.month, run.day), (2028, 2, 29));

        // A day that never comes.
        assert!(Schedule::parse("0 0 31 2 *")
            .unwrap()
            .next_runs(now, 1, utc)
            .is_empty());
    }

    #[test]
    fn half_hour_time_zones() {
        // UTC+05:30: local hours start at :30 past the UTC hour.
        let india = |at: i64| utc(at + 5 * 3600 + 1800);
        let s = Schedule::parse("0 3 * * *").unwrap();
        let run = s.next_runs(1_792_015_509, 1, india)[0];
        let local = india(run);
        assert_eq!((local.hour, local.minute), (3, 0));
        let u = utc(run);
        assert_eq!((u.hour, u.minute), (21, 30));
    }

    #[test]
    fn plain_english() {
        let d = |s| describe(s).unwrap_or_else(|| format!("({s})"));
        assert_eq!(d("* * * * *"), "Every minute");
        assert_eq!(d("*/5 * * * *"), "Every 5 minutes");
        assert_eq!(d("0 * * * *"), "Every hour, on the hour");
        assert_eq!(d("15 * * * *"), "Every hour at 15 minutes past");
        assert_eq!(d("0 */6 * * *"), "Every 6 hours at 00 minutes past");
        assert_eq!(d("30 2 * * *"), "Every day at 02:30");
        assert_eq!(d("0 9 * * 1-5"), "Every weekday at 09:00");
        assert_eq!(d("0 9 * * sat,sun"), "Every Saturday and Sunday at 09:00");
        assert_eq!(d("0 9 * * mon,thu"), "Every Monday, Thursday at 09:00");
        assert_eq!(d("0 4 1 * *"), "On the 1st of every month at 04:00");
        assert_eq!(d("0 4 22 * *"), "On the 22nd of every month at 04:00");
        assert_eq!(d("@daily"), "Every day at 00:00");
        assert_eq!(d("0 0 1 1 *"), "(0 0 1 1 *)");
        assert_eq!(d("bad"), "(bad)");
    }
}
