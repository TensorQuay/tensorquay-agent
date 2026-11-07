//! Retry-After delays from seconds or any HTTP-date representation in RFC 9110 section 5.6.7.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

const WEEKDAYS: &[&str] = &["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const LONG_WEEKDAYS: &[&str] = &[
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

pub(super) fn parse(value: &str, now: SystemTime) -> Option<Duration> {
    let value = value.trim();
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        return value.parse::<u64>().ok().map(Duration::from_secs);
    }
    let now_seconds = signed_seconds(now)?;
    let date = date(value, now_seconds)?;
    let seconds = date.seconds_since_epoch(date.year);
    let duration = Duration::from_secs(seconds.unsigned_abs());
    let when = if seconds < 0 {
        UNIX_EPOCH.checked_sub(duration)?
    } else {
        UNIX_EPOCH.checked_add(duration)?
    };
    Some(when.duration_since(now).unwrap_or_default())
}

struct Date {
    year: u32,
    month: usize,
    day: u32,
    time: u32,
}

impl Date {
    fn seconds_since_epoch(&self, year: u32) -> i64 {
        let days = days_before_year(year) - days_before_year(1970)
            + i64::from(month_days(year)[..self.month].iter().sum::<u32>())
            + i64::from(self.day)
            - 1;
        days * 86_400 + i64::from(self.time)
    }
}

fn date(value: &str, now_seconds: i64) -> Option<Date> {
    let fields: Vec<_> = value.split_whitespace().collect();
    let (day, month, year, time, short_year) = match fields.as_slice() {
        [weekday, day, month, year, time, "GMT"]
            if WEEKDAYS.contains(&weekday.strip_suffix(',')?) =>
        {
            (*day, *month, *year, *time, false)
        }
        [weekday, date, time, "GMT"] if LONG_WEEKDAYS.contains(&weekday.strip_suffix(',')?) => {
            let parts: Vec<_> = date.split('-').collect();
            let [day, month, year] = parts.as_slice() else {
                return None;
            };
            (*day, *month, *year, *time, true)
        }
        [weekday, month, day, time, year] if WEEKDAYS.contains(weekday) => {
            (*day, *month, *year, *time, false)
        }
        _ => return None,
    };
    if year.len() != if short_year { 2 } else { 4 }
        || !year.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let mut year = year.parse::<u32>().ok()?;
    if short_year {
        year += current_year(now_seconds)? / 100 * 100;
    }
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|value| *value == month)?;
    let mut date = Date {
        year,
        month,
        day: day.parse().ok()?,
        time: time_of_day(time)?,
    };
    if short_year && year >= 50 && date.seconds_since_epoch(year - 50) > now_seconds {
        date.year = year.checked_sub(100)?;
    }
    if !(1..=9999).contains(&date.year) || !(1..=month_days(date.year)[month]).contains(&date.day) {
        return None;
    }
    Some(date)
}

fn time_of_day(value: &str) -> Option<u32> {
    let parts: Vec<_> = value
        .split(':')
        .map(str::parse::<u32>)
        .collect::<Result<_, _>>()
        .ok()?;
    let [hour, minute, second] = parts.as_slice() else {
        return None;
    };
    if *hour > 23
        || *minute > 59
        || *second > 60
        || (*second == 60 && (*hour != 23 || *minute != 59))
    {
        return None;
    }
    Some(hour * 3600 + minute * 60 + second)
}

fn month_days(year: u32) -> [u32; 12] {
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ]
}

fn days_before_year(year: u32) -> i64 {
    let prior = i64::from(year) - 1;
    prior * 365 + prior / 4 - prior / 100 + prior / 400
}

fn signed_seconds(now: SystemTime) -> Option<i64> {
    match now.duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_secs()).ok(),
        Err(error) => Some(
            -i64::try_from(error.duration().as_secs()).ok()?
                - i64::from(error.duration().subsec_nanos() != 0),
        ),
    }
}

fn current_year(seconds: i64) -> Option<u32> {
    let epoch_days = days_before_year(1970);
    let start = |year| (days_before_year(year) - epoch_days) * 86_400;
    if seconds < start(1) || seconds >= start(10_000) {
        return None;
    }
    let (mut low, mut high) = (1, 10_000);
    while low + 1 < high {
        let middle = low + (high - low) / 2;
        if start(middle) <= seconds {
            low = middle;
        } else {
            high = middle;
        }
    }
    Some(low)
}

#[cfg(test)]
mod tests {
    use super::parse;
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn llm_e_008_retry_after_seconds_and_calendar_dates() {
        let now = UNIX_EPOCH + Duration::from_secs(1);
        for (value, seconds) in [
            (" 120 ", 120),
            ("Thu, 01 Jan 1970 00:00:00 GMT", 0),
            ("Thu, 01 Jan 1970 00:01:01 GMT", 60),
            ("Wed, 31 Dec 1969 23:59:59 GMT", 0),
            ("Tue, 29 Feb 2000 12:34:56 GMT", 951_827_695),
        ] {
            assert_eq!(
                parse(value, now),
                Some(Duration::from_secs(seconds)),
                "{value}"
            );
        }
        for invalid in [
            "",
            "-1",
            "+1",
            "nonsense",
            "Thu 01 Jan 1970 00:00:00 GMT",
            "Thu, 01 Jan 1970 00:00:00 UTC",
            "Thu, x Jan 1970 00:00:00 GMT",
            "Thu, 01 Bog 1970 00:00:00 GMT",
            "Thu, 01 Jan x 00:00:00 GMT",
            "Thu, 01 Jan 0000 00:00:00 GMT",
            "Thu, 01 Jan 10000 00:00:00 GMT",
            "Mon, 29 Feb 2100 00:00:00 GMT",
            "Thu, 01 Jan 1970 24:00:00 GMT",
            "Thu, 01 Jan 1970 00:60:00 GMT",
            "Thu, 01 Jan 1970 00:00:60 GMT",
            "Thu, 01 Jan 1970 00:00 GMT",
            "Thu, 01 Jan 1970 x:00:00 GMT",
        ] {
            assert_eq!(parse(invalid, now), None, "{invalid}");
        }
    }

    #[test]
    fn llm_e_008_retry_after_accepts_legacy_dates_and_resolves_two_digit_years() {
        let now = UNIX_EPOCH + Duration::from_secs(1);
        for value in [
            "Thursday, 01-Jan-70 00:01:01 GMT",
            "Thu Jan  1 00:01:01 1970",
        ] {
            assert_eq!(parse(value, now), Some(Duration::from_mins(1)), "{value}");
        }
        let now = UNIX_EPOCH + Duration::from_hours(490_896); // 2026-01-01 UTC.
        assert_eq!(
            parse("Thursday, 02-Jan-76 00:00:00 GMT", now),
            Some(Duration::ZERO)
        );
        assert_eq!(
            parse("Thursday, 01-Jan-76 00:00:00 GMT", now),
            parse("Thu, 01 Jan 2076 00:00:00 GMT", now)
        );
        assert_eq!(
            parse("Sat, 31 Dec 2016 23:59:60 GMT", UNIX_EPOCH),
            Some(Duration::from_hours(412_008)) // 2017-01-01 UTC.
        );
        for invalid in [
            "Thursday, 01-Jan-1970 00:00:00 GMT",
            "Thursday, broken 00:00:00 GMT",
            "Thursday, 01-Jan-xx 00:00:00 GMT",
            "Thu Jan 1 00:00:00 invalid",
            "Noday, 01 Jan 1970 00:00:00 GMT",
        ] {
            assert_eq!(parse(invalid, now), None, "{invalid}");
        }
    }
}
