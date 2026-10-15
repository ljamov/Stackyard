//! Wall-clock time on this computer, which is what schedules are written
//! in: "every day at 02:00" means 02:00 where the user is.

use std::time::{SystemTime, UNIX_EPOCH};

/// A minute on the calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moment {
    pub year: i32,
    /// 1 to 12.
    pub month: u32,
    /// 1 to 31.
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    /// 0 is Sunday, 6 is Saturday.
    pub weekday: u32,
}

/// Seconds since 1970, UTC.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// The calendar minute `at` falls in, in UTC.
pub fn utc(at: i64) -> Moment {
    let days = at.div_euclid(86_400);
    let secs = at.rem_euclid(86_400);
    let (year, month, day) = civil(days);
    Moment {
        year,
        month,
        day,
        hour: (secs / 3600) as u32,
        minute: (secs % 3600 / 60) as u32,
        weekday: (days + 4).rem_euclid(7) as u32, // 1970-01-01 was a Thursday
    }
}

/// Year, month and day for a count of days since 1970-01-01, in the
/// proleptic Gregorian calendar. Works in 400-year cycles, which repeat
/// exactly, counting years from March so the leap day falls last.
fn civil(days: i64) -> (i32, u32, u32) {
    let from_march_0000 = days + 719_468;
    let cycle = from_march_0000.div_euclid(146_097);
    let day_of_cycle = from_march_0000.rem_euclid(146_097);
    let year_of_cycle =
        (day_of_cycle - day_of_cycle / 1460 + day_of_cycle / 36_524 - day_of_cycle / 146_096) / 365;
    let day_of_year =
        day_of_cycle - (365 * year_of_cycle + year_of_cycle / 4 - year_of_cycle / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_from_march + 2) / 5 + 1) as u32;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    } as u32;
    let year = year_of_cycle + cycle * 400 + i64::from(month <= 2);
    (year as i32, month, day)
}

/// The calendar minute `at` falls in, in this computer's time zone.
#[cfg(unix)]
pub fn local(at: i64) -> Moment {
    let time = at as libc::time_t;
    // SAFETY: `tm` is plain data that localtime_r fills in; both pointers
    // are valid for the call.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&time, &mut tm) }.is_null() {
        return utc(at);
    }
    Moment {
        year: tm.tm_year + 1900,
        month: (tm.tm_mon + 1) as u32,
        day: tm.tm_mday as u32,
        hour: tm.tm_hour as u32,
        minute: tm.tm_min as u32,
        weekday: tm.tm_wday as u32,
    }
}

/// The calendar minute `at` falls in, in this computer's time zone.
#[cfg(windows)]
pub fn local(at: i64) -> Moment {
    use windows_sys::Win32::Foundation::SYSTEMTIME;
    use windows_sys::Win32::System::Time::SystemTimeToTzSpecificLocalTime;

    let u = utc(at);
    let given = SYSTEMTIME {
        wYear: u.year as u16,
        wMonth: u.month as u16,
        wDayOfWeek: u.weekday as u16,
        wDay: u.day as u16,
        wHour: u.hour as u16,
        wMinute: u.minute as u16,
        wSecond: 0,
        wMilliseconds: 0,
    };
    // SAFETY: both SYSTEMTIMEs are valid for the call; a null time zone
    // means the computer's current one, with its daylight saving rules.
    let mut out: SYSTEMTIME = unsafe { std::mem::zeroed() };
    if unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &given, &mut out) } == 0 {
        return u;
    }
    Moment {
        year: i32::from(out.wYear),
        month: u32::from(out.wMonth),
        day: u32::from(out.wDay),
        hour: u32::from(out.wHour),
        minute: u32::from(out.wMinute),
        weekday: u32::from(out.wDayOfWeek),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_dates() {
        let m = |year, month, day, hour, minute, weekday| Moment {
            year,
            month,
            day,
            hour,
            minute,
            weekday,
        };
        assert_eq!(utc(0), m(1970, 1, 1, 0, 0, 4));
        // 2026-10-14 22:05:09, a Wednesday.
        assert_eq!(utc(1_792_015_509), m(2026, 10, 14, 22, 5, 3));
        // A leap day, and the day after.
        assert_eq!(utc(951_782_400), m(2000, 2, 29, 0, 0, 2));
        assert_eq!(utc(951_868_799), m(2000, 2, 29, 23, 59, 2));
        assert_eq!(utc(951_868_800), m(2000, 3, 1, 0, 0, 3));
        assert_eq!(utc(-1), m(1969, 12, 31, 23, 59, 3));
        assert_eq!(utc(4_102_444_800), m(2100, 1, 1, 0, 0, 5));
    }

    #[test]
    fn every_day_follows_the_one_before() {
        let mut previous = utc(0);
        for day in 1..(400 * 366) {
            let next = utc(day * 86_400);
            assert_eq!(next.weekday, (previous.weekday + 1) % 7);
            let new_month = next.day == 1;
            assert!(
                new_month || next.day == previous.day + 1,
                "{previous:?} {next:?}"
            );
            previous = next;
        }
    }

    #[test]
    fn local_time_is_a_real_minute() {
        let m = local(now());
        assert!((1..=12).contains(&m.month) && (1..=31).contains(&m.day));
        assert!(m.hour < 24 && m.minute < 60 && m.weekday < 7);
        // No time zone is more than 14 hours from UTC.
        let u = utc(now());
        let minutes =
            |m: &Moment| i64::from(m.day) * 1440 + i64::from(m.hour) * 60 + i64::from(m.minute);
        if m.month == u.month {
            assert!((minutes(&m) - minutes(&u)).abs() <= 14 * 60);
        }
    }
}
