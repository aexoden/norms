// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! The project's latest release, whose version every manifest that carries one names.

use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use semver::Version;

/// The version a project has released last, and where it was found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Release {
    Version {
        version: Version,
        source: Source,
    },
    /// The latest release heading of `CHANGELOG.md` names something other than a version, such as `[v1.0.0]`.
    Invalid(String),
}

/// Where a [`Release`]'s version comes from.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Source {
    /// The first release heading of `CHANGELOG.md`.
    Changelog,
    /// The highest release tag, in a project without `CHANGELOG.md`.
    Tag,
    /// No release yet: version 0.0.0.
    Unreleased,
}

/// A Keep a Changelog release heading, such as `## [0.2.0] - 2026-11-10`, or `## [Unreleased]`.
static RELEASE_HEADING: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r"(?m)^## \[(?<version>[^\]]+)\]").unwrap()
});

impl Release {
    /// The latest release of the project at `path`, whose release tags (`vX.Y.Z`) are `tags`: the first release in its
    /// `CHANGELOG.md`, or, without one, its highest release tag. A project with neither has released nothing, which
    /// version 0.0.0 stands for.
    pub(crate) fn find(path: &Path, tags: &[String]) -> Self {
        let (heading, source) = fs::read_to_string(path.join("CHANGELOG.md")).map_or_else(
            |_| {
                (
                    highest_tag(tags).map(|version| version.to_string()),
                    Source::Tag,
                )
            },
            |changelog| (latest_in_changelog(&changelog), Source::Changelog),
        );
        let Some(heading) = heading else {
            return Self::Version {
                version: Version::new(0, 0, 0),
                source: Source::Unreleased,
            };
        };
        Version::parse(&heading).map_or_else(
            |_| Self::Invalid(heading.clone()),
            |version| Self::Version { version, source },
        )
    }

    /// Check that a manifest's `version` is this release's.
    pub(crate) fn check(&self, version: Option<&str>) -> Result<(), String> {
        let (expected, source) = match self {
            Self::Version {
                version: released,
                source,
            } => (released.to_string(), source),
            Self::Invalid(heading) => {
                return Err(format!(
                    "CHANGELOG.md's latest release, [{heading}], isn't a semantic version, such as 1.0.0, to compare \
                     the version with"
                ));
            }
        };
        let source = match source {
            Source::Changelog => "the latest release in CHANGELOG.md",
            Source::Tag => "the latest release tag",
            Source::Unreleased => "as the project has no release yet",
        };
        match version {
            Some(version) if version == expected => Ok(()),
            Some(version) => Err(format!("version is {version}, not {expected}, {source}")),
            None => Err(format!(
                "Missing version, which should be {expected}, {source}"
            )),
        }
    }
}

/// The highest version among the release tags `tags` (`vX.Y.Z`).
fn highest_tag(tags: &[String]) -> Option<Version> {
    tags.iter()
        .filter_map(|tag| Version::parse(tag.strip_prefix('v')?).ok())
        .max()
}

/// The first release heading in `changelog` after `[Unreleased]`, such as `0.2.0`, which may not be a version.
fn latest_in_changelog(changelog: &str) -> Option<String> {
    RELEASE_HEADING
        .captures_iter(changelog)
        .filter_map(|captures| captures.name("version"))
        .map(|heading| heading.as_str().trim())
        .find(|heading| !heading.eq_ignore_ascii_case("unreleased"))
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    const CHANGELOG: &str =
        "# Changelog\n\n## [Unreleased]\n\n## [0.2.0] - 2026-11-10\n\n## [0.1.0] - 2026-01-10\n";

    fn project(changelog: Option<&str>) -> TempDir {
        let dir = TempDir::new().unwrap();
        if let Some(changelog) = changelog {
            fs::write(dir.path().join("CHANGELOG.md"), changelog).unwrap();
        }
        dir
    }

    fn release(changelog: Option<&str>, tags: &[&str]) -> Release {
        let dir = project(changelog);
        let tags: Vec<String> = tags.iter().map(|&tag| tag.to_owned()).collect();
        Release::find(dir.path(), &tags)
    }

    #[test]
    fn the_changelog_names_the_latest_release() {
        assert_eq!(
            release(Some(CHANGELOG), &["v0.3.0"]),
            Release::Version {
                version: Version::new(0, 2, 0),
                source: Source::Changelog,
            }
        );
    }

    const UNRELEASED: Release = Release::Version {
        version: Version::new(0, 0, 0),
        source: Source::Unreleased,
    };

    #[test]
    fn a_changelog_without_releases_is_unreleased() {
        assert_eq!(
            release(Some("# Changelog\n\n## [Unreleased]\n"), &["v0.3.0"]),
            UNRELEASED
        );
    }

    #[test]
    fn a_release_heading_that_isnt_a_version_is_invalid() {
        let changelog =
            "# Changelog\n\n## [Unreleased]\n\n## [v1.0.0] - 2026-11-12\n\n## [0.9.0]\n";
        let invalid = release(Some(changelog), &[]);
        assert_eq!(invalid, Release::Invalid("v1.0.0".to_owned()));
        assert_eq!(
            invalid.check(Some("0.9.0")),
            Err(
                "CHANGELOG.md's latest release, [v1.0.0], isn't a semantic version, such as 1.0.0, to compare the \
                 version with"
                    .to_owned()
            )
        );
    }

    #[test]
    fn without_a_changelog_the_highest_tag_is_the_latest_release() {
        let tagged = release(None, &["v0.10.0", "v0.9.0"]);
        assert_eq!(
            tagged,
            Release::Version {
                version: Version::new(0, 10, 0),
                source: Source::Tag,
            }
        );
        assert_eq!(
            tagged.check(Some("0.9.0")),
            Err("version is 0.9.0, not 0.10.0, the latest release tag".to_owned())
        );
    }

    #[test]
    fn without_a_changelog_or_tags_nothing_is_released() {
        assert_eq!(release(None, &[]), UNRELEASED);
    }

    #[test]
    fn checks_versions_against_the_release() {
        let released = release(Some(CHANGELOG), &[]);
        assert_eq!(released.check(Some("0.2.0")), Ok(()));
        assert_eq!(
            released.check(Some("0.1.0")),
            Err("version is 0.1.0, not 0.2.0, the latest release in CHANGELOG.md".to_owned())
        );
        assert_eq!(
            UNRELEASED.check(None),
            Err(
                "Missing version, which should be 0.0.0, as the project has no release yet"
                    .to_owned()
            )
        );
    }
}
