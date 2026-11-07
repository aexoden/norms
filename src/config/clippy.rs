// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `clippy.toml`.

use serde::Deserialize;

/// The names Clippy reads its configuration from, in the order it prefers them within one directory.
pub(crate) const CLIPPY_TOML: [&str; 2] = [".clippy.toml", "clippy.toml"];

/// The limit on an absolute path's segments that the standard sets.
const ABSOLUTE_PATHS_MAX_SEGMENTS: i64 = 3;

/// Top-level `clippy.toml` structure. Its fields are read leniently, so that one of the wrong type fails only its
/// check.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct ClippyToml {
    pub absolute_paths_max_segments: Option<toml::Value>,
    pub allow_expect_in_tests: Option<toml::Value>,
    pub allow_panic_in_tests: Option<toml::Value>,
    pub allow_unwrap_in_tests: Option<toml::Value>,
}

impl ClippyToml {
    /// Check that `unwrap`, `expect` and `panic` are allowed in tests. The error names each setting that isn't `true`.
    pub(crate) fn check_allowed_in_tests(&self) -> Result<(), String> {
        let unset: Vec<&str> = [
            ("allow-expect-in-tests", &self.allow_expect_in_tests),
            ("allow-panic-in-tests", &self.allow_panic_in_tests),
            ("allow-unwrap-in-tests", &self.allow_unwrap_in_tests),
        ]
        .into_iter()
        .filter(|(_, value)| value.as_ref().and_then(toml::Value::as_bool) != Some(true))
        .map(|(name, _)| name)
        .collect();

        if unset.is_empty() {
            Ok(())
        } else {
            Err(format!("Not set to true: {}", unset.join(", ")))
        }
    }

    /// Check that absolute paths are limited to the standard's number of segments.
    pub(crate) fn check_absolute_paths_max_segments(&self) -> Result<(), String> {
        match &self.absolute_paths_max_segments {
            Some(value) if value.as_integer() == Some(ABSOLUTE_PATHS_MAX_SEGMENTS) => Ok(()),
            Some(value) => Err(format!(
                "absolute-paths-max-segments is {value}, not {ABSOLUTE_PATHS_MAX_SEGMENTS}"
            )),
            None => Err(format!(
                "absolute-paths-max-segments isn't set, so Clippy allows 2 rather than {ABSOLUTE_PATHS_MAX_SEGMENTS}"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clippy(toml: &str) -> ClippyToml {
        toml::from_str(toml).unwrap()
    }

    #[test]
    fn tests_may_unwrap_expect_and_panic() {
        assert_eq!(
            clippy("allow-expect-in-tests = true\nallow-panic-in-tests = true\nallow-unwrap-in-tests = true\n")
                .check_allowed_in_tests(),
            Ok(())
        );
        assert_eq!(
            clippy("allow-expect-in-tests = true\nallow-panic-in-tests = false\nallow-unwrap-in-tests = \"yes\"\n")
                .check_allowed_in_tests(),
            Err("Not set to true: allow-panic-in-tests, allow-unwrap-in-tests".to_owned())
        );
        assert_eq!(
            clippy("").check_allowed_in_tests(),
            Err(
                "Not set to true: allow-expect-in-tests, allow-panic-in-tests, allow-unwrap-in-tests"
                    .to_owned()
            )
        );
    }

    #[test]
    fn absolute_paths_have_three_segments() {
        assert_eq!(
            clippy("absolute-paths-max-segments = 3\n").check_absolute_paths_max_segments(),
            Ok(())
        );
        for (toml, error) in [
            (
                "absolute-paths-max-segments = 2\n",
                "absolute-paths-max-segments is 2, not 3",
            ),
            (
                "absolute-paths-max-segments = \"3\"\n",
                "absolute-paths-max-segments is \"3\", not 3",
            ),
            (
                "",
                "absolute-paths-max-segments isn't set, so Clippy allows 2 rather than 3",
            ),
        ] {
            assert_eq!(
                clippy(toml).check_absolute_paths_max_segments(),
                Err(error.to_owned()),
                "{toml}"
            );
        }
    }
}
