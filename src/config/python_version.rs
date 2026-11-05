// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Model for `.python-version`, the Python version uv installs and runs for a project.

use std::str::FromStr as _;

use pep508_rs::pep440_rs::Version;

/// The file that pins a Python component's development version.
pub(crate) const PYTHON_VERSION: &str = ".python-version";

/// Parse the contents of `.python-version`: its first line that is neither blank nor a `#` comment, as uv reads it.
///
/// uv also accepts requests such as `cpython@3.15` or `3.15t`, but the standards pin a plain version, so anything
/// else is an error.
pub(crate) fn parse_python_version(text: &str) -> Result<Version, String> {
    let line = text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))
        .ok_or_else(|| "Names no version".to_owned())?;
    Version::from_str(line).map_err(|error| format!("Not a plain version, {line}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_first_version_line() {
        assert_eq!(
            parse_python_version("3.15.2\n"),
            Ok(Version::new([3, 15, 2]))
        );
        assert_eq!(
            parse_python_version("# pinned\n\n  3.14 \n3.13\n"),
            Ok(Version::new([3, 14]))
        );
        assert_eq!(
            parse_python_version("# nothing\n\n"),
            Err("Names no version".to_owned())
        );
        assert!(
            parse_python_version("cpython@3.15\n")
                .unwrap_err()
                .starts_with("Not a plain version, cpython@3.15: ")
        );
    }
}
