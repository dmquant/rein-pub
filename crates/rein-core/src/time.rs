//! Timestamps (RFC 3339, normalized to UTC) and logical time.
//!
//! Canonical form per decision C1 (invariant 7): UTC only, `Z` suffix,
//! fractional seconds trimmed of trailing zeros. Offsets are accepted on
//! parse and normalized away. Leap seconds (`:60`) are rejected — stated
//! limit, not an oversight. There is no `now()` anywhere: time is a value.

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TimeError {
    #[error("timestamp `{0}` is not RFC 3339 (`YYYY-MM-DDTHH:MM:SS[.frac](Z|±HH:MM)`)")]
    Malformed(String),
    #[error("timestamp `{0}` has an out-of-range field")]
    OutOfRange(String),
    #[error("timestamp `{0}`: leap second `:60` is rejected in this schema")]
    LeapSecond(String),
    #[error("timestamp `{0}`: fractional seconds beyond 9 digits")]
    FractionTooLong(String),
}

/// An instant, stored as civil UTC fields. Ordered chronologically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp {
    days: i64,  // days since 1970-01-01
    secs: u32,  // seconds within day
    nanos: u32, // subsecond
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

// Howard Hinnant's civil-days algorithms.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = i64::from((153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1);
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

impl Timestamp {
    pub fn from_civil_utc(
        y: i64,
        mo: u32,
        d: u32,
        h: u32,
        mi: u32,
        s: u32,
        nanos: u32,
    ) -> Result<Self, TimeError> {
        let repr = format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}");
        if !(1..=12).contains(&mo) || d < 1 || d > days_in_month(y, mo) || h > 23 || mi > 59 {
            return Err(TimeError::OutOfRange(repr));
        }
        if s == 60 {
            return Err(TimeError::LeapSecond(repr));
        }
        if s > 59 || nanos > 999_999_999 {
            return Err(TimeError::OutOfRange(repr));
        }
        Ok(Self {
            days: days_from_civil(y, mo, d),
            secs: h * 3600 + mi * 60 + s,
            nanos,
        })
    }

    /// Parse RFC 3339, normalizing any offset to UTC.
    pub fn parse(s: &str) -> Result<Self, TimeError> {
        let b = s.as_bytes();
        let mal = || TimeError::Malformed(s.to_string());
        let digits = |r: std::ops::Range<usize>| -> Result<u32, TimeError> {
            let seg = b.get(r).ok_or_else(mal)?;
            if !seg.iter().all(u8::is_ascii_digit) {
                return Err(mal());
            }
            Ok(seg.iter().fold(0u32, |a, c| a * 10 + u32::from(c - b'0')))
        };
        let sep = |i: usize, c: u8| -> Result<(), TimeError> {
            if b.get(i) == Some(&c) {
                Ok(())
            } else {
                Err(mal())
            }
        };

        let y = i64::from(digits(0..4)?);
        sep(4, b'-')?;
        let mo = digits(5..7)?;
        sep(7, b'-')?;
        let d = digits(8..10)?;
        match b.get(10) {
            Some(b'T') | Some(b't') => {}
            _ => return Err(mal()),
        }
        let h = digits(11..13)?;
        sep(13, b':')?;
        let mi = digits(14..16)?;
        sep(16, b':')?;
        let sec = digits(17..19)?;

        let mut i = 19;
        let mut nanos: u32 = 0;
        if b.get(i) == Some(&b'.') {
            let start = i + 1;
            let mut end = start;
            while b.get(end).is_some_and(u8::is_ascii_digit) {
                end += 1;
            }
            if end == start {
                return Err(mal());
            }
            if end - start > 9 {
                return Err(TimeError::FractionTooLong(s.to_string()));
            }
            let frac = digits(start..end)?;
            nanos = frac * 10u32.pow(9 - (end - start) as u32);
            i = end;
        }

        let offset_secs: i64 = match b.get(i) {
            Some(b'Z') | Some(b'z') => {
                if i + 1 != b.len() {
                    return Err(mal());
                }
                0
            }
            Some(sign @ (b'+' | b'-')) => {
                if i + 6 != b.len() {
                    return Err(mal());
                }
                let oh = digits(i + 1..i + 3)?;
                sep(i + 3, b':')?;
                let om = digits(i + 4..i + 6)?;
                if oh > 23 || om > 59 {
                    return Err(TimeError::OutOfRange(s.to_string()));
                }
                let mag = i64::from(oh) * 3600 + i64::from(om) * 60;
                if *sign == b'+' {
                    mag
                } else {
                    -mag
                }
            }
            _ => return Err(mal()),
        };

        let t = Self::from_civil_utc(y, mo, d, h, mi, sec, nanos).map_err(|e| match e {
            TimeError::OutOfRange(_) => TimeError::OutOfRange(s.to_string()),
            TimeError::LeapSecond(_) => TimeError::LeapSecond(s.to_string()),
            other => other,
        })?;
        let total = t.days * 86_400 + i64::from(t.secs) - offset_secs;
        Ok(Self {
            days: total.div_euclid(86_400),
            secs: total.rem_euclid(86_400) as u32,
            nanos: t.nanos,
        })
    }

    /// Canonical RFC 3339 UTC rendering: `Z` suffix, fraction trimmed.
    pub fn canonical(&self) -> String {
        let (y, mo, d) = civil_from_days(self.days);
        let (h, mi, s) = (self.secs / 3600, self.secs % 3600 / 60, self.secs % 60);
        let mut out = format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}");
        if self.nanos > 0 {
            let frac = format!("{:09}", self.nanos);
            out.push('.');
            out.push_str(frac.trim_end_matches('0'));
        }
        out.push('Z');
        out
    }
}

impl std::fmt::Display for Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.canonical())
    }
}

impl Serialize for Timestamp {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.canonical())
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Logical milliseconds for event ordering and deadlines. Injected, never read
/// from a wall clock (invariant 10's honest limit lives at the runtime, not here).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct LogicalMs(pub u64);
