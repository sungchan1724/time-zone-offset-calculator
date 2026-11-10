//! POSIX TZ environment variable rules for computing UTC offsets.
//!
//! The library parses a POSIX TZ string such as `EST5EDT,M3.2.0,M11.1.0` and
//! answers one question: for a given local civil date and time, what is the
//! UTC offset in seconds east of UTC?
//!
//! Only the POSIX grammar is implemented: an optional std abbreviation, a
//! signed offset, an optional DST abbreviation and offset, and an optional
//! DST rule. The `TZ` value is treated as if the `posix/` prefix were present;
//! the `right/` and file-backed forms are out of scope.
//!
//! # Ambiguity policy
//!
//! During a fall-back transition the same local wall time occurs twice. This
//! library resolves the ambiguity by choosing the *earlier* occurrence, i.e.
//! the one still in daylight saving time. During a spring-forward gap no local
//! time exists; the library resolves it by choosing the offset in effect
//! *before* the transition (standard time). Both choices are stated plainly so
//! callers can decide whether they need a different disambiguation.

use std::fmt;

/// Errors produced while parsing a POSIX TZ string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TzError {
    /// The string is empty or contains no usable offset.
    Empty,
    /// A numeric field was malformed or out of range.
    BadNumber { field: &'static str, value: String },
    /// A rule clause (`,M...`) was malformed.
    BadRule { value: String },
    /// A quoted abbreviation was not terminated.
    UnterminatedQuote,
    /// The offset magnitude exceeds 24 hours.
    OffsetOutOfRange { seconds: i64 },
}

impl fmt::Display for TzError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TzError::Empty => write!(f, "empty POSIX TZ string"),
            TzError::BadNumber { field, value } => {
                write!(f, "bad {field} number: {value:?}")
            }
            TzError::BadRule { value } => write!(f, "bad DST rule: {value:?}"),
            TzError::UnterminatedQuote => write!(f, "unterminated quoted abbreviation"),
            TzError::OffsetOutOfRange { seconds } => {
                write!(f, "offset out of range: {seconds} seconds")
            }
        }
    }
}

impl std::error::Error for TzError {}

/// A civil date and time with no attached zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LocalDateTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl LocalDateTime {
    pub fn new(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> Self {
        LocalDateTime { year, month, day, hour, minute, second }
    }

    /// Seconds since the Unix epoch treating this value as UTC. This is a
    /// pure calendar calculation; no timezone is involved.
    pub fn to_unix_seconds(self) -> i64 {
        let days = days_from_civil(self.year, self.month, self.day);
        let secs = days * 86_400
            + self.hour as i64 * 3600
            + self.minute as i64 * 60
            + self.second as i64;
        secs
    }

    /// Inverse of [`LocalDateTime::to_unix_seconds`].
    pub fn from_unix_seconds(secs: i64) -> Self {
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        LocalDateTime {
            year,
            month,
            day,
            hour: (rem / 3600) as u32,
            minute: ((rem % 3600) / 60) as u32,
            second: (rem % 60) as u32,
        }
    }
}

/// Days from 1970-01-01 to the given civil date, using Howard Hinnant's
/// `days_from_civil` algorithm. It is exact for the proleptic Gregorian
/// calendar over the full range of `i32` years.
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = ((m as i64) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + (d as i64) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Inverse of [`days_from_civil`].
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

/// A parsed POSIX TZ specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TzSpec {
    pub std_abbr: String,
    /// Seconds east of UTC for standard time.
    pub std_offset: i64,
    pub dst_abbr: Option<String>,
    /// Seconds east of UTC for daylight saving time.
    pub dst_offset: Option<i64>,
    pub dst_rule: Option<DstRule>,
}

/// A POSIX `,M` style daylight saving rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DstRule {
    pub start: TransitionRule,
    pub end: TransitionRule,
}

/// One transition, expressed as month, week, and weekday.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransitionRule {
    /// Month 1..=12.
    pub month: u32,
    /// Week 1..=5; 5 means "last <weekday> of the month".
    pub week: u32,
    /// Weekday 0..=6, Sunday = 0.
    pub weekday: u32,
    /// Seconds after local midnight.
    pub time: i64,
}

impl TzSpec {
    /// Parse a POSIX TZ string. The `posix/` prefix is not accepted; pass the
    /// rule text directly. A leading `:` is accepted and ignored.
    pub fn parse(s: &str) -> Result<TzSpec, TzError> {
        let s = s.trim();
        let s = s.strip_prefix(':').unwrap_or(s);
        if s.is_empty() {
            return Err(TzError::Empty);
        }
        let bytes = s.as_bytes();
        let mut i = 0usize;

        let std_abbr = read_abbr(s, &mut i)?;
        if std_abbr.is_empty() {
            return Err(TzError::Empty);
        }

        let std_offset = read_offset(s, &mut i, "std offset")?;

        if i >= bytes.len() {
            return Ok(TzSpec {
                std_abbr,
                std_offset,
                dst_abbr: None,
                dst_offset: None,
                dst_rule: None,
            });
        }

        let dst_abbr = read_abbr(s, &mut i)?;
        if dst_abbr.is_empty() {
            return Err(TzError::BadRule { value: s.to_string() });
        }

        let dst_offset = if i < bytes.len() && bytes[i] != b',' {
            Some(read_offset(s, &mut i, "dst offset")?)
        } else {
            // POSIX defaults the DST offset to standard time + 1 hour.
            Some(std_offset + 3600)
        };

        let dst_rule = if i < bytes.len() {
            if bytes[i] != b',' {
                return Err(TzError::BadRule { value: s[i..].to_string() });
            }
            Some(read_rule(s, &mut i)?)
        } else {
            // POSIX default when a DST abbreviation is present but no rule:
            // US rules as of 1987. We keep that default.
            Some(DstRule {
                start: TransitionRule { month: 4, week: 1, weekday: 0, time: 2 * 3600 },
                end: TransitionRule { month: 10, week: 5, weekday: 0, time: 2 * 3600 },
            })
        };

        if i != bytes.len() {
            return Err(TzError::BadRule { value: s[i..].to_string() });
        }

        Ok(TzSpec {
            std_abbr,
            std_offset,
            dst_abbr: Some(dst_abbr),
            dst_offset,
            dst_rule,
        })
    }

    /// True if this spec has a daylight saving component.
    pub fn has_dst(&self) -> bool {
        self.dst_abbr.is_some() && self.dst_offset.is_some() && self.dst_rule.is_some()
    }

    /// UTC offset in seconds east of UTC for the given local wall time.
    ///
    /// The returned value is the offset that, added to UTC, yields this local
    /// time. Ambiguities are resolved per the module-level policy.
    pub fn offset_for_local(&self, local: LocalDateTime) -> i64 {
        let Some(rule) = self.dst_rule else {
            return self.std_offset;
        };
        let Some(dst_offset) = self.dst_offset else {
            return self.std_offset;
        };

        // Compute transition instants in UTC. The rule times are local wall
        // times, and the offset in effect just before each transition differs
        // between the two transitions, so we use the appropriate offset for
        // each.
        let start_utc = rule_instant_utc(rule.start, local.year, self.std_offset);
        let end_utc = rule_instant_utc(rule.end, local.year, dst_offset);

        // Convert the query into a UTC instant under each candidate offset and
        // ask whether that instant falls in DST.
        let as_std = local.to_unix_seconds() - self.std_offset;
        let as_dst = local.to_unix_seconds() - dst_offset;

        let in_dst = |utc: i64| -> bool {
            if start_utc <= end_utc {
                utc >= start_utc && utc < end_utc
            } else {
                // Southern hemisphere: DST wraps the year boundary.
                utc >= start_utc || utc < end_utc
            }
        };

        if in_dst(as_dst) {
            dst_offset
        } else if in_dst(as_std) {
            self.std_offset
        } else {
            // Neither interpretation lands in DST. This is the spring-forward
            // gap: the local time does not exist. Per policy, report the
            // offset in effect before the transition, which is standard time.
            self.std_offset
        }
    }

    /// UTC offset for a UTC instant, in seconds east of UTC.
    pub fn offset_for_utc(&self, utc_secs: i64) -> i64 {
        let Some(rule) = self.dst_rule else {
            return self.std_offset;
        };
        let Some(dst_offset) = self.dst_offset else {
            return self.std_offset;
        };
        let (year, _, _) = civil_from_days(utc_secs.div_euclid(86_400));
        let start_utc = rule_instant_utc(rule.start, year, self.std_offset);
        let end_utc = rule_instant_utc(rule.end, year, dst_offset);
        let is_dst = if start_utc <= end_utc {
            utc_secs >= start_utc && utc_secs < end_utc
        } else {
            utc_secs >= start_utc || utc_secs < end_utc
        };
        if is_dst {
            dst_offset
        } else {
            self.std_offset
        }
    }
}

/// Convert a transition rule into a UTC instant for the given year. The rule
/// time is a local wall time; `prior_offset` is the offset in effect just
/// before the transition, which is what the wall clock was showing.
fn rule_instant_utc(rule: TransitionRule, year: i32, prior_offset: i64) -> i64 {
    let day = nth_weekday_of_month(year, rule.month, rule.week, rule.weekday);
    let local = LocalDateTime::new(year, rule.month, day, 0, 0, 0);
    local.to_unix_seconds() + rule.time - prior_offset
}

/// Day of month for the `week`-th `weekday` of `month` in `year`.
/// `week == 5` means the last such weekday in the month.
fn nth_weekday_of_month(year: i32, month: u32, week: u32, weekday: u32) -> u32 {
    let first = days_from_civil(year, month, 1);
    // Weekday of the first of the month, Sunday = 0.
    let first_wd = (first + 4).rem_euclid(7) as u32;
    let mut day = 1 + (weekday + 7 - first_wd) % 7;
    if week == 5 {
        while day + 7 <= days_in_month(year, month) {
            day += 7;
        }
    } else {
        day += (week - 1) * 7;
    }
    day
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Read an abbreviation: either a run of ASCII letters, or a quoted string
/// of letters, digits, `+`, and `-`.
fn read_abbr(s: &str, i: &mut usize) -> Result<String, TzError> {
    let bytes = s.as_bytes();
    if *i >= bytes.len() {
        return Ok(String::new());
    }
    if bytes[*i] == b'<' {
        let start = *i + 1;
        let mut j = start;
        while j < bytes.len() && bytes[j] != b'>' {
            j += 1;
        }
        if j >= bytes.len() {
            return Err(TzError::UnterminatedQuote);
        }
        let inner = &s[start..j];
        if inner.is_empty() {
            return Err(TzError::UnterminatedQuote);
        }
        *i = j + 1;
        Ok(inner.to_string())
    } else {
        let start = *i;
        while *i < bytes.len() && bytes[*i].is_ascii_alphabetic() {
            *i += 1;
        }
        Ok(s[start..*i].to_string())
    }
}

/// Read a signed offset of the form `[+-]hh[:mm[:ss]]`. POSIX defines the
/// value as the time you add to local time to get UTC, so the offset east of
/// UTC is the negation.
fn read_offset(s: &str, i: &mut usize, field: &'static str) -> Result<i64, TzError> {
    let bytes = s.as_bytes();
    let start = *i;
    let mut sign = 1i64;
    if *i < bytes.len() && (bytes[*i] == b'+' || bytes[*i] == b'-') {
        if bytes[*i] == b'-' {
            sign = -1;
        }
        *i += 1;
    }
    let h_start = *i;
    while *i < bytes.len() && bytes[*i].is_ascii_digit() {
        *i += 1;
    }
    if *i == h_start {
        return Err(TzError::BadNumber { field, value: s[start..].to_string() });
    }
    let hours: i64 = s[h_start..*i]
        .parse()
        .map_err(|_| TzError::BadNumber { field, value: s[h_start..*i].to_string() })?;
    let mut minutes = 0i64;
    let mut seconds = 0i64;
    if *i < bytes.len() && bytes[*i] == b':' {
        *i += 1;
        let m_start = *i;
        while *i < bytes.len() && bytes[*i].is_ascii_digit() {
            *i += 1;
        }
        if *i == m_start {
            return Err(TzError::BadNumber { field, value: s[start..].to_string() });
        }
        minutes = s[m_start..*i]
            .parse()
            .map_err(|_| TzError::BadNumber { field, value: s[m_start..*i].to_string() })?;
        if *i < bytes.len() && bytes[*i] == b':' {
            *i += 1;
            let s_start = *i;
            while *i < bytes.len() && bytes[*i].is_ascii_digit() {
                *i += 1;
            }
            if *i == s_start {
                return Err(TzError::BadNumber { field, value: s[start..].to_string() });
            }
            seconds = s[s_start..*i]
                .parse()
                .map_err(|_| TzError::BadNumber { field, value: s[s_start..*i].to_string() })?;
        }
    }
    let total = sign * (hours * 3600 + minutes * 60 + seconds);
    if total.abs() > 24 * 3600 {
        return Err(TzError::OffsetOutOfRange { seconds: total });
    }
    // POSIX offset is added to local time to get UTC, so east-of-UTC is the
    // negation of the parsed value.
    Ok(-total)
}

/// Parse `,start,end` where each side is `Mm.w.d[/time]`.
fn read_rule(s: &str, i: &mut usize) -> Result<DstRule, TzError> {
    let bytes = s.as_bytes();
    if *i >= bytes.len() || bytes[*i] != b',' {
        return Err(TzError::BadRule { value: s[*i..].to_string() });
    }
    *i += 1;
    let start = read_transition(s, i)?;
    if *i >= bytes.len() || bytes[*i] != b',' {
        return Err(TzError::BadRule { value: s[*i..].to_string() });
    }
    *i += 1;
    let end = read_transition(s, i)?;
    Ok(DstRule { start, end })
}

fn read_transition(s: &str, i: &mut usize) -> Result<TransitionRule, TzError> {
    let bytes = s.as_bytes();
    if *i >= bytes.len() || bytes[*i] != b'M' {
        return Err(TzError::BadRule { value: s[*i..].to_string() });
    }
    *i += 1;
    let month = read_u32(s, i, 1, 12, "month")?;
    if *i >= bytes.len() || bytes[*i] != b'.' {
        return Err(TzError::BadRule { value: s[*i..].to_string() });
    }
    *i += 1;
    let week = read_u32(s, i, 1, 5, "week")?;
    if *i >= bytes.len() || bytes[*i] != b'.' {
        return Err(TzError::BadRule { value: s[*i..].to_string() });
    }
    *i += 1;
    let weekday = read_u32(s, i, 0, 6, "weekday")?;
    let mut time = 2 * 3600i64;
    if *i < bytes.len() && bytes[*i] == b'/' {
        *i += 1;
        time = read_signed_time(s, i)?;
    }
    Ok(TransitionRule { month, week, weekday, time })
}

fn read_u32(
    s: &str,
    i: &mut usize,
    min: u32,
    max: u32,
    field: &'static str,
) -> Result<u32, TzError> {
    let bytes = s.as_bytes();
    let start = *i;
    while *i < bytes.len() && bytes[*i].is_ascii_digit() {
        *i += 1;
    }
    if *i == start {
        return Err(TzError::BadRule { value: s[start..].to_string() });
    }
    let v: u32 = s[start..*i]
        .parse()
        .map_err(|_| TzError::BadRule { value: s[start..*i].to_string() })?;
    if v < min || v > max {
        return Err(TzError::BadNumber { field, value: v.to_string() });
    }
    Ok(v)
}

/// Time component after `/`: `[+-]hh[:mm[:ss]]` with no negation of meaning.
fn read_signed_time(s: &str, i: &mut usize) -> Result<i64, TzError> {
    let bytes = s.as_bytes();
    let start = *i;
    let mut sign = 1i64;
    if *i < bytes.len() && (bytes[*i] == b'+' || bytes[*i] == b'-') {
        if bytes[*i] == b'-' {
            sign = -1;
        }
        *i += 1;
    }
    let h_start = *i;
    while *i < bytes.len() && bytes[*i].is_ascii_digit() {
        *i += 1;
    }
    if *i == h_start {
        return Err(TzError::BadRule { value: s[start..].to_string() });
    }
    let hours: i64 = s[h_start..*i]
        .parse()
        .map_err(|_| TzError::BadRule { value: s[h_start..*i].to_string() })?;
    let mut minutes = 0i64;
    let mut seconds = 0i64;
    if *i < bytes.len() && bytes[*i] == b':' {
        *i += 1;
        let m_start = *i;
        while *i < bytes.len() && bytes[*i].is_ascii_digit() {
            *i += 1;
        }
        if *i == m_start {
            return Err(TzError::BadRule { value: s[start..].to_string() });
        }
        minutes = s[m_start..*i]
            .parse()
            .map_err(|_| TzError::BadRule { value: s[m_start..*i].to_string() })?;
        if *i < bytes.len() && bytes[*i] == b':' {
            *i += 1;
            let s_start = *i;
            while *i < bytes.len() && bytes[*i].is_ascii_digit() {
                *i += 1;
            }
            if *i == s_start {
                return Err(TzError::BadRule { value: s[start..].to_string() });
            }
            seconds = s[s_start..*i]
                .parse()
                .map_err(|_| TzError::BadRule { value: s[s_start..*i].to_string() })?;
        }
    }
    Ok(sign * (hours * 3600 + minutes * 60 + seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(s: &str) -> TzSpec {
        TzSpec::parse(s).expect("parse should succeed")
    }

    #[test]
    fn fixed_offset_only() {
        let tz = spec("UTC0");
        assert_eq!(tz.std_abbr, "UTC");
        assert_eq!(tz.std_offset, 0);
        assert!(!tz.has_dst());
        let t = LocalDateTime::new(2024, 1, 1, 0, 0, 0);
        assert_eq!(tz.offset_for_local(t), 0);
    }

    #[test]
    fn east_offsets_are_positive() {
        // IST-5:30 means UTC+05:30, i.e. 19800 seconds east.
        let tz = spec("IST-5:30");
        assert_eq!(tz.std_offset, 5 * 3600 + 30 * 60);
    }

    #[test]
    fn west_offsets_are_negative() {
        // EST5 means UTC-05:00.
        let tz = spec("EST5");
        assert_eq!(tz.std_offset, -5 * 3600);
    }

    #[test]
    fn us_eastern_dst_boundaries() {
        let tz = spec("EST5EDT,M3.2.0,M11.1.0");
        assert!(tz.has_dst());
        assert_eq!(tz.dst_offset, Some(-4 * 3600));

        // Just before the spring-forward transition: standard time.
        let before = LocalDateTime::new(2024, 3, 10, 1, 59, 59);
        assert_eq!(tz.offset_for_local(before), -5 * 3600);

        // Just after: daylight time.
        let after = LocalDateTime::new(2024, 3, 10, 3, 0, 0);
        assert_eq!(tz.offset_for_local(after), -4 * 3600);

        // Mid-summer.
        let summer = LocalDateTime::new(2024, 7, 1, 12, 0, 0);
        assert_eq!(tz.offset_for_local(summer), -4 * 3600);

        // Mid-winter.
        let winter = LocalDateTime::new(2024, 1, 15, 12, 0, 0);
        assert_eq!(tz.offset_for_local(winter), -5 * 3600);
    }

    #[test]
    fn spring_forward_gap_uses_standard_time() {
        let tz = spec("EST5EDT,M3.2.0,M11.1.0");
        // 2024-03-10 02:30 does not exist locally. Policy: standard time.
        let gap = LocalDateTime::new(2024, 3, 10, 2, 30, 0);
        assert_eq!(tz.offset_for_local(gap), -5 * 3600);
    }

    #[test]
    fn fall_back_ambiguity_uses_earlier_occurrence() {
        let tz = spec("EST5EDT,M3.2.0,M11.1.0");
        // 2024-11-03 01:30 occurs twice. Policy: earlier, i.e. DST.
        let ambiguous = LocalDateTime::new(2024, 11, 3, 1, 30, 0);
        assert_eq!(tz.offset_for_local(ambiguous), -4 * 3600);
    }

    #[test]
    fn southern_hemisphere_wraps_year() {
        // Australia/Sydney style: DST starts first Sunday in October and
        // ends first Sunday in April.
        let tz = spec("AEST-10AEDT,M10.1.0,M4.1.0/3");
        let jan = LocalDateTime::new(2024, 1, 15, 12, 0, 0);
        let jul = LocalDateTime::new(2024, 7, 15, 12, 0, 0);
        assert_eq!(tz.offset_for_local(jan), 11 * 3600);
        assert_eq!(tz.offset_for_local(jul), 10 * 3600);
    }

    #[test]
    fn last_weekday_rule() {
        // November's last Sunday in 2024 is the 24th.
        assert_eq!(nth_weekday_of_month(2024, 11, 5, 0), 24);
        // March's second Sunday in 2024 is the 10th.
        assert_eq!(nth_weekday_of_month(2024, 3, 2, 0), 10);
    }

    #[test]
    fn quoted_abbreviation() {
        let tz = spec("<+0530>-5:30");
        assert_eq!(tz.std_abbr, "+0530");
        assert_eq!(tz.std_offset, 5 * 3600 + 30 * 60);
    }

    #[test]
    fn explicit_dst_offset() {
        // DST offset given explicitly as 4:30 east.
        let tz = spec("AAA-5BBB-4:30,M3.2.0,M11.1.0");
        assert_eq!(tz.dst_offset, Some(4 * 3600 + 30 * 60));
    }

    #[test]
    fn default_dst_offset_is_std_plus_one_hour() {
        let tz = spec("EST5EDT,M3.2.0,M11.1.0");
        assert_eq!(tz.dst_offset, Some(tz.std_offset + 3600));
    }

    #[test]
    fn offset_for_utc_matches_offset_for_local() {
        let tz = spec("EST5EDT,M3.2.0,M11.1.0");
        let local = LocalDateTime::new(2024, 7, 1, 12, 0, 0);
        let off = tz.offset_for_local(local);
        let utc = local.to_unix_seconds() - off;
        assert_eq!(tz.offset_for_utc(utc), off);
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(TzSpec::parse(""), Err(TzError::Empty));
        assert!(matches!(TzSpec::parse("EST"), Err(TzError::BadNumber { .. })));
        assert!(matches!(
            TzSpec::parse("EST5EDT,M13.2.0,M11.1.0"),
            Err(TzError::BadNumber { .. })
        ));
        assert!(matches!(
            TzSpec::parse("EST5EDT,M3.2.0"),
            Err(TzError::BadRule { .. })
        ));
    }

    #[test]
    fn round_trips_unix_seconds() {
        let cases = [
            LocalDateTime::new(1970, 1, 1, 0, 0, 0),
            LocalDateTime::new(2000, 2, 29, 23, 59, 59),
            LocalDateTime::new(1969, 12, 31, 23, 59, 59),
            LocalDateTime::new(2024, 3, 10, 2, 30, 0),
        ];
        for c in cases {
            let secs = c.to_unix_seconds();
            assert_eq!(LocalDateTime::from_unix_seconds(secs), c);
        }
    }
}
