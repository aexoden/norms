// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! The release age that each package manager enforces (STANDARDS.md "Release Age").

use jiff::{SignedDuration, Span, SpanRelativeTo};

/// How old a release must be before a project adopts it.
const RELEASE_AGE: SignedDuration = SignedDuration::from_hours(72);

/// Check that a duration, such as `"3 days"`, is the release age.
///
/// jiff, which uv uses for its own durations, parses it, so both friendly forms (`"3 days"`, `"72 hours"`) and ISO
/// 8601 (`"P3D"`) are understood. Days count as 24 hours. Months and years have no fixed length, so they fail, as do
/// values that aren't durations at all, such as Cargo's `"0"`, which turns the release age off.
pub(crate) fn check_release_age(value: &str) -> Result<(), String> {
    let duration = value
        .parse::<Span>()
        .ok()
        .and_then(|span| span.to_duration(SpanRelativeTo::days_are_24_hours()).ok());

    if duration == Some(RELEASE_AGE) {
        Ok(())
    } else {
        Err(format!("\"{value}\" isn't 3 days"))
    }
}

/// Check that a TOML value is a string holding the release age, as [`check_release_age`] does.
pub(crate) fn check_release_age_value(value: &toml::Value) -> Result<(), String> {
    value
        .as_str()
        .ok_or_else(|| format!("{value} isn't a string"))
        .and_then(check_release_age)
}

/// Check that a number of minutes, as pnpm gives it, is the release age.
pub(crate) fn check_release_age_minutes(minutes: f64) -> Result<(), String> {
    #[expect(
        clippy::cast_precision_loss,
        reason = "the release age in minutes is small"
    )]
    let expected = (RELEASE_AGE.as_secs() / 60) as f64;

    if (minutes - expected).abs() < f64::EPSILON {
        Ok(())
    } else {
        Err(format!("{minutes} minutes isn't 3 days (4320)"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_three_days_in_any_form() {
        for value in [
            "3 days",
            "3 day",
            "3days",
            "3d",
            "72 hours",
            "4320 minutes",
            "2 days 24 hours",
            "P3D",
            "PT72H",
        ] {
            assert_eq!(check_release_age(value), Ok(()), "{value}");
        }
    }

    #[test]
    fn rejects_other_ages() {
        for value in [
            "7 days",
            "1 day",
            "0",
            "",
            "3",
            "1 week",
            "3 days ago",
            "1 month",
            "2026-01-01",
            "3 dayz",
        ] {
            assert!(check_release_age(value).is_err(), "{value}");
        }
        assert_eq!(
            check_release_age("7 days"),
            Err("\"7 days\" isn't 3 days".to_owned())
        );
        assert_eq!(check_release_age("0"), Err("\"0\" isn't 3 days".to_owned()));
    }

    #[test]
    fn values_must_be_strings() {
        assert_eq!(
            check_release_age_value(&toml::Value::from("3 days")),
            Ok(())
        );
        assert_eq!(
            check_release_age_value(&toml::Value::from(0)),
            Err("0 isn't a string".to_owned())
        );
    }

    #[test]
    fn checks_minutes() {
        assert_eq!(check_release_age_minutes(4320.0), Ok(()));
        for minutes in [0.0, 1440.0, 4320.5, 10_080.0, -4320.0, f64::MAX, f64::NAN] {
            assert!(check_release_age_minutes(minutes).is_err(), "{minutes}");
        }
    }
}
