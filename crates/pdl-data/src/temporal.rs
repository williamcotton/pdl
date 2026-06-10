// Deterministic temporal parsing, flooring, and formatting shared by the
// row runtime (`pdl-exec`) and the data facade (`crate::engine`). v0.46.5
// keeps the value model string-backed: helpers parse at the function
// boundary and render back to plain strings or numbers.
//
// Everything here is pure. No wall-clock reads, no local time-zone lookup,
// no process locale, and no timezone-database names; only fixed offsets
// parsed from the input text.

use chrono::{DateTime, Datelike, Days, FixedOffset, NaiveDate, SecondsFormat, Timelike};

/// A parsed temporal value: a calendar date or a datetime with the fixed
/// offset it was written in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TemporalValue {
    Date(NaiveDate),
    DateTime(DateTime<FixedOffset>),
}

/// Calendar units accepted by `date_floor`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TemporalUnit {
    Day,
    Week,
    Month,
    Year,
}

/// Parses trimmed text as `YYYY-MM-DD` or an RFC3339 datetime. `Z` is a
/// first-class UTC designator. Unparseable text returns `None`; callers map
/// that to a null value.
pub fn parse_temporal(text: &str) -> Option<TemporalValue> {
    let text = text.trim();
    if let Ok(date) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        return Some(TemporalValue::Date(date));
    }
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(TemporalValue::DateTime)
}

pub fn parse_temporal_unit(text: &str) -> Option<TemporalUnit> {
    match text {
        "day" => Some(TemporalUnit::Day),
        "week" => Some(TemporalUnit::Week),
        "month" => Some(TemporalUnit::Month),
        "year" => Some(TemporalUnit::Year),
        _ => None,
    }
}

pub fn temporal_year(value: &TemporalValue) -> i32 {
    match value {
        TemporalValue::Date(date) => date.year(),
        TemporalValue::DateTime(datetime) => datetime.year(),
    }
}

pub fn temporal_month(value: &TemporalValue) -> u32 {
    match value {
        TemporalValue::Date(date) => date.month(),
        TemporalValue::DateTime(datetime) => datetime.month(),
    }
}

pub fn temporal_day(value: &TemporalValue) -> u32 {
    match value {
        TemporalValue::Date(date) => date.day(),
        TemporalValue::DateTime(datetime) => datetime.day(),
    }
}

/// Renders the calendar date of a parsed value as `YYYY-MM-DD`. Datetimes
/// use the date as written in their fixed offset.
pub fn normalize_date(value: &TemporalValue) -> String {
    let date = temporal_naive_date(value);
    format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day())
}

/// Renders a parsed datetime as a normalized RFC3339 string with whole
/// seconds. UTC renders as `Z`; other offsets render as `+HH:MM`/`-HH:MM`.
/// Date-only input has no time or offset and returns `None` (a null value).
pub fn normalize_datetime(value: &TemporalValue) -> Option<String> {
    match value {
        TemporalValue::Date(_) => None,
        TemporalValue::DateTime(datetime) => {
            Some(datetime.to_rfc3339_opts(SecondsFormat::Secs, true))
        }
    }
}

/// Floors a parsed value to the start of the requested unit. Datetimes keep
/// their parsed fixed offset and floor in that offset's local calendar.
pub fn floor_temporal(value: &TemporalValue, unit: TemporalUnit) -> TemporalValue {
    match value {
        TemporalValue::Date(date) => TemporalValue::Date(floor_date(*date, unit)),
        TemporalValue::DateTime(datetime) => {
            let floored = floor_date(datetime.date_naive(), unit)
                .and_hms_opt(0, 0, 0)
                .expect("midnight is a valid time of day")
                .and_local_timezone(*datetime.offset())
                .single()
                .expect("fixed offsets map local datetimes unambiguously");
            TemporalValue::DateTime(floored)
        }
    }
}

fn floor_date(date: NaiveDate, unit: TemporalUnit) -> NaiveDate {
    match unit {
        TemporalUnit::Day => date,
        // ISO weeks start on Monday, matching the `%G`/`%V`/`%u` tokens.
        TemporalUnit::Week => date
            .checked_sub_days(Days::new(u64::from(date.weekday().num_days_from_monday())))
            .expect("every supported date has a same-week Monday"),
        TemporalUnit::Month => NaiveDate::from_ymd_opt(date.year(), date.month(), 1)
            .expect("the first of a parsed month is a valid date"),
        TemporalUnit::Year => NaiveDate::from_ymd_opt(date.year(), 1, 1)
            .expect("January 1 of a parsed year is a valid date"),
    }
}

/// Checks a `date_format` pattern against the v0.46.5 token subset:
/// `%Y`, `%m`, `%d`, `%H`, `%M`, `%S`, `%G`, `%V`, `%u`, `%j`, `%z`,
/// `%:z`, and `%%`. Returns the offending token text so callers can
/// report it.
pub fn validate_format_pattern(pattern: &str) -> Result<(), String> {
    let mut chars = pattern.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            continue;
        }
        match chars.next() {
            Some('Y' | 'm' | 'd' | 'H' | 'M' | 'S' | 'G' | 'V' | 'u' | 'j' | 'z' | '%') => {}
            Some(':') => match chars.next() {
                Some('z') => {}
                Some(other) => return Err(format!("%:{other}")),
                None => return Err("%:".to_string()),
            },
            Some(other) => return Err(format!("%{other}")),
            None => return Err("%".to_string()),
        }
    }
    Ok(())
}

/// Formats a parsed value with a validated pattern. Date-only input renders
/// time fields as `00`; offset tokens on date-only input return `None`
/// (a null value) because no offset was written. Fractional seconds are not
/// representable in the token subset and are dropped by `%S`.
pub fn format_temporal(value: &TemporalValue, pattern: &str) -> Option<String> {
    let mut output = String::with_capacity(pattern.len());
    let mut chars = pattern.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            output.push(ch);
            continue;
        }
        match chars.next() {
            Some('Y') => output.push_str(&format!("{:04}", temporal_year(value))),
            Some('m') => output.push_str(&format!("{:02}", temporal_month(value))),
            Some('d') => output.push_str(&format!("{:02}", temporal_day(value))),
            Some('H') => output.push_str(&format!("{:02}", temporal_hour(value))),
            Some('M') => output.push_str(&format!("{:02}", temporal_minute(value))),
            Some('S') => output.push_str(&format!("{:02}", temporal_second(value))),
            Some('G') => output.push_str(&format!(
                "{:04}",
                temporal_naive_date(value).iso_week().year()
            )),
            Some('V') => output.push_str(&format!(
                "{:02}",
                temporal_naive_date(value).iso_week().week()
            )),
            Some('u') => output.push_str(
                &temporal_naive_date(value)
                    .weekday()
                    .number_from_monday()
                    .to_string(),
            ),
            Some('j') => output.push_str(&format!("{:03}", temporal_naive_date(value).ordinal())),
            Some('z') => output.push_str(&offset_text(value, false)?),
            Some(':') => {
                // The validator only admits `%:z`.
                chars.next();
                output.push_str(&offset_text(value, true)?);
            }
            Some('%') => output.push('%'),
            // Unreachable after `validate_format_pattern`; keep the literal
            // so a missed validation cannot panic.
            Some(other) => {
                output.push('%');
                output.push(other);
            }
            None => output.push('%'),
        }
    }
    Some(output)
}

/// The calendar date the value was written as: the date itself, or a
/// datetime's date in its own fixed offset. ISO week-year, week number,
/// weekday, and day-of-year all derive from this date.
fn temporal_naive_date(value: &TemporalValue) -> NaiveDate {
    match value {
        TemporalValue::Date(date) => *date,
        TemporalValue::DateTime(datetime) => datetime.date_naive(),
    }
}

fn temporal_hour(value: &TemporalValue) -> u32 {
    match value {
        TemporalValue::Date(_) => 0,
        TemporalValue::DateTime(datetime) => datetime.hour(),
    }
}

fn temporal_minute(value: &TemporalValue) -> u32 {
    match value {
        TemporalValue::Date(_) => 0,
        TemporalValue::DateTime(datetime) => datetime.minute(),
    }
}

fn temporal_second(value: &TemporalValue) -> u32 {
    match value {
        TemporalValue::Date(_) => 0,
        TemporalValue::DateTime(datetime) => datetime.second(),
    }
}

fn offset_text(value: &TemporalValue, colon: bool) -> Option<String> {
    let TemporalValue::DateTime(datetime) = value else {
        return None;
    };
    let seconds = datetime.offset().local_minus_utc();
    let sign = if seconds < 0 { '-' } else { '+' };
    let minutes = seconds.abs() / 60;
    let (hours, minutes) = (minutes / 60, minutes % 60);
    Some(if colon {
        format!("{sign}{hours:02}:{minutes:02}")
    } else {
        format!("{sign}{hours:02}{minutes:02}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporal_scalar_functions_parse_accepted_forms() {
        assert!(matches!(
            parse_temporal("2024-01-15"),
            Some(TemporalValue::Date(_))
        ));
        for text in [
            "2025-02-17T14:20:59Z",
            "2025-02-17T14:20:59+00:00",
            "2024-01-15T10:22:33-05:00",
            "2024-01-15T10:22:33.123456-05:00",
            " 2024-01-15 ",
        ] {
            assert!(parse_temporal(text).is_some(), "{text} must parse");
        }
        for text in ["", "yesterday", "02/03/2025", "2024-13-01", "2024-01-32"] {
            assert!(parse_temporal(text).is_none(), "{text} must not parse");
        }
    }

    #[test]
    fn temporal_scalar_functions_z_and_utc_offset_agree() {
        let z = parse_temporal("2025-02-17T14:20:59Z").expect("Z form parses");
        let offset = parse_temporal("2025-02-17T14:20:59+00:00").expect("+00:00 form parses");
        assert_eq!(temporal_year(&z), temporal_year(&offset));
        assert_eq!(temporal_month(&z), temporal_month(&offset));
        assert_eq!(temporal_day(&z), temporal_day(&offset));
        assert_eq!(normalize_datetime(&z), normalize_datetime(&offset));
        assert_eq!(
            normalize_datetime(&z).as_deref(),
            Some("2025-02-17T14:20:59Z")
        );
    }

    #[test]
    fn temporal_scalar_functions_floor_preserves_offset() {
        let parsed = parse_temporal("2024-01-15T10:22:33-05:00").expect("offset form parses");
        let floored = floor_temporal(&parsed, TemporalUnit::Month);
        assert_eq!(
            normalize_datetime(&floored).as_deref(),
            Some("2024-01-01T00:00:00-05:00")
        );
        let date = parse_temporal("2024-01-15").expect("date parses");
        assert_eq!(
            normalize_date(&floor_temporal(&date, TemporalUnit::Year)),
            "2024-01-01"
        );
    }

    /// Week flooring lands on the ISO Monday of the value's week, matching
    /// the `%G`/`%V`/`%u` token calendar, including across calendar-year
    /// boundaries.
    #[test]
    fn temporal_scalar_functions_floor_week_is_iso_monday() {
        for (input, expected) in [
            // Monday floors to itself.
            ("2025-02-17", "2025-02-17"),
            // Sunday floors back six days.
            ("2025-02-23", "2025-02-17"),
            // Wednesday 2025-01-01 floors into the previous calendar year.
            ("2025-01-01", "2024-12-30"),
        ] {
            let parsed = parse_temporal(input).expect("date parses");
            assert_eq!(
                normalize_date(&floor_temporal(&parsed, TemporalUnit::Week)),
                expected,
                "{input}"
            );
        }
        // Datetimes floor in their own offset's calendar and keep the offset.
        let datetime = parse_temporal("2025-02-23T10:22:33-05:00").expect("datetime parses");
        assert_eq!(
            normalize_datetime(&floor_temporal(&datetime, TemporalUnit::Week)).as_deref(),
            Some("2025-02-17T00:00:00-05:00")
        );
    }

    #[test]
    fn temporal_scalar_functions_format_token_subset() {
        let datetime = parse_temporal("2024-01-15T10:22:33.5-05:00").expect("datetime parses");
        assert_eq!(
            format_temporal(&datetime, "%Y-%m-%d %H:%M:%S %z %:z 100%%").as_deref(),
            Some("2024-01-15 10:22:33 -0500 -05:00 100%")
        );
        let date = parse_temporal("2024-01-15").expect("date parses");
        assert_eq!(format_temporal(&date, "%Y-%m").as_deref(), Some("2024-01"));
        assert_eq!(
            format_temporal(&date, "%H:%M:%S").as_deref(),
            Some("00:00:00")
        );
        assert_eq!(format_temporal(&date, "%z"), None);
        assert_eq!(format_temporal(&date, "%:z"), None);
    }

    /// ISO week tokens follow the first-Thursday rule, so dates near the
    /// calendar year boundary can belong to the previous or next ISO
    /// week-year.
    #[test]
    fn temporal_scalar_functions_iso_week_tokens() {
        for (input, expected) in [
            // Friday 2027-01-01 falls in week 53 of ISO year 2026.
            ("2027-01-01", "2026-W53-5 001"),
            // Monday 2024-12-30 falls in week 1 of ISO year 2025.
            ("2024-12-30", "2025-W01-1 365"),
            ("2025-02-17", "2025-W08-1 048"),
        ] {
            let parsed = parse_temporal(input).expect("date parses");
            assert_eq!(
                format_temporal(&parsed, "%G-W%V-%u %j").as_deref(),
                Some(expected),
                "{input}"
            );
        }
        // Datetimes use the calendar date as written in their offset.
        let datetime = parse_temporal("2024-12-30T23:59:59-05:00").expect("datetime parses");
        assert_eq!(
            format_temporal(&datetime, "%G-W%V").as_deref(),
            Some("2025-W01")
        );
    }

    #[test]
    fn temporal_scalar_functions_validate_pattern_tokens() {
        assert_eq!(validate_format_pattern("%Y-%m-%dT%H:%M:%S%:z"), Ok(()));
        assert_eq!(validate_format_pattern("%G-W%V-%u %j"), Ok(()));
        assert_eq!(validate_format_pattern("%%"), Ok(()));
        assert_eq!(validate_format_pattern("%B"), Err("%B".to_string()));
        assert_eq!(validate_format_pattern("%:m"), Err("%:m".to_string()));
        assert_eq!(validate_format_pattern("trailing %"), Err("%".to_string()));
    }
}
