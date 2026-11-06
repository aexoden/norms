// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `pyproject.toml`.
//!
//! Only the subset of fields inspected by checks is modeled. All structs use `#[serde(default)]` so that missing keys
//! deserialize to `None` / empty rather than causing a parse error.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;
use std::str::FromStr as _;

use pep508_rs::pep440_rs::{
    Operator, Version, VersionSpecifier, VersionSpecifiers, release_specifier_to_range,
    release_specifiers_to_ranges,
};
use pep508_rs::{PackageName, Pep508ErrorSource, Requirement, VerbatimUrl, VersionOrUrl};
use serde::Deserialize;

use super::python_version::PYTHON_VERSION;

/// Top-level `pyproject.toml` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct PyprojectToml {
    pub project: Option<ProjectSection>,
    pub tool: Option<ToolSection>,
    /// Each group's requirements and `include-group` tables. Kept unparsed, so that a malformed group fails only the
    /// checks reading it.
    pub dependency_groups: Option<BTreeMap<String, toml::Value>>,
}

/// The `[project]` table.
///
/// The fields that only the dependency checks read are kept unparsed, so that a malformed one fails only those checks.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct ProjectSection {
    pub name: Option<String>,
    pub description: Option<String>,
    pub requires_python: Option<String>,
    pub license: Option<toml::Value>,
    pub classifiers: Option<toml::Value>,
    /// The fields that the build backend fills in, rather than this table.
    pub dynamic: Option<toml::Value>,
    pub dependencies: Option<toml::Value>,
    pub optional_dependencies: Option<toml::Value>,
    /// Console entry points: commands installed with the package.
    pub scripts: Option<toml::Value>,
    /// GUI entry points.
    pub gui_scripts: Option<toml::Value>,
}

/// The `[tool]` table (we only model the tools we inspect).
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct ToolSection {
    pub ruff: Option<RuffConfig>,
    pub mypy: Option<MypyConfig>,
    pub uv: Option<UvConfig>,
}

impl PyprojectToml {
    /// The oldest Python release that `requires-python` allows, read as uv reads it: as release versions only, without
    /// pre-releases or epochs. This is the lower bound as written if it is allowed and has at most three segments
    /// (`>=3.12` gives 3.12), or else the next micro release that is allowed: `>3.12` gives 3.12.1, and
    /// `>=3.12, !=3.12.0, !=3.12.1` gives 3.12.2. Exclusions apply, so `>=3.12, !=3.12.*` gives 3.13.
    pub(crate) fn min_python(&self) -> Result<Version, String> {
        let (requires, specifiers) = self.requires_python()?;
        let allowed = release_specifiers_to_ranges(specifiers);
        for (lower, _) in allowed.iter() {
            let (Bound::Included(lower) | Bound::Excluded(lower)) = lower else {
                return Err(format!("requires-python ({requires}) sets no lower bound"));
            };
            // The first release in this segment is its lower bound, if allowed and a release of up to three segments,
            // or else the next micro release.
            let mut release = lower.release().to_vec();
            release.truncate(3);
            let first = Version::new(&release);
            if allowed.contains(&first) {
                return Ok(first);
            }
            release.resize(3, 0);
            if let Some(micro) = release.last_mut() {
                *micro = micro.saturating_add(1);
            }
            let next = Version::new(&release);
            if allowed.contains(&next) {
                return Ok(next);
            }
        }
        Err(format!("requires-python ({requires}) allows no version"))
    }

    /// `requires-python`, as written and parsed.
    fn requires_python(&self) -> Result<(&str, VersionSpecifiers), String> {
        let Some(requires) = self
            .project
            .as_ref()
            .and_then(|project| project.requires_python.as_deref())
        else {
            return Err("pyproject.toml sets no requires-python".to_owned());
        };
        let specifiers = VersionSpecifiers::from_str(requires)
            .map_err(|error| format!("Invalid requires-python: {error}"))?;
        Ok((requires, specifiers))
    }

    /// Whether the project is a library: a package, with a `[project]` table and without `[tool.uv] package = false`,
    /// that installs no command.
    pub(crate) fn is_library(&self) -> bool {
        let unpackaged = self
            .tool
            .as_ref()
            .and_then(|tool| tool.uv.as_ref())
            .and_then(|uv| uv.package.as_ref())
            .is_some_and(|package| package.as_bool() == Some(false));
        self.project.is_some() && !unpackaged && !self.is_application()
    }

    /// Whether the project is an application rather than a library: whether it installs a command, through a
    /// non-empty `[project.scripts]` or `[project.gui-scripts]`.
    pub(crate) fn is_application(&self) -> bool {
        let installs = |scripts: Option<&toml::Value>| {
            scripts
                .and_then(toml::Value::as_table)
                .is_some_and(|table| !table.is_empty())
        };

        self.project
            .as_ref()
            .is_some_and(|p| installs(p.scripts.as_ref()) || installs(p.gui_scripts.as_ref()))
    }

    /// Check that `requires-python` agrees with `pinned`, the version in `.python-version`: an application's is
    /// exactly `==` that version, and a library's range allows it.
    pub(crate) fn check_requires_python_pin(&self, pinned: &Version) -> Result<(), String> {
        let (requires, specifiers) = self.requires_python()?;
        if self.is_application() {
            match &*specifiers {
                [specifier]
                    if *specifier.operator() == Operator::Equal
                        && specifier.version() == pinned =>
                {
                    Ok(())
                }
                _ => Err(format!(
                    "An application's requires-python ({requires}) isn't =={pinned}, the version in \
                     {PYTHON_VERSION}"
                )),
            }
        } else if release_specifiers_to_ranges(specifiers).contains(&pinned.clone().only_release())
        {
            Ok(())
        } else {
            Err(format!(
                "requires-python ({requires}) doesn't allow {pinned}, the version in {PYTHON_VERSION}"
            ))
        }
    }

    /// Check that the `Programming Language :: Python :: 3.x` classifiers list exactly the minor versions
    /// `requires-python` allows, from its lower bound up to `pinned`, the version in `.python-version`.
    ///
    /// A minor version is allowed if `requires-python` admits any of its releases, so `==3.15.2` allows only 3.15 and
    /// `>=3.12, !=3.13.*` skips 3.13. Without a pin, the newest version is the upper bound of `requires-python`, if it
    /// is within Python 3 (such as `<3.14` or `<=3.14`, but not `<4`). Other classifiers are ignored, including
    /// `Programming Language :: Python :: 3` and `3 :: Only`.
    pub(crate) fn check_python_classifiers(&self, pinned: Option<&Version>) -> Result<(), String> {
        let project = self.project.as_ref();
        let dynamic = project
            .and_then(|p| p.dynamic.as_ref())
            .and_then(toml::Value::as_array);
        if dynamic
            .into_iter()
            .flatten()
            .any(|field| field.as_str() == Some("classifiers"))
        {
            return Err("project.classifiers is dynamic, so it can't be checked".to_owned());
        }
        let listed: BTreeSet<u64> = match project.and_then(|p| p.classifiers.as_ref()) {
            None => BTreeSet::new(),
            Some(classifiers) => classifiers
                .as_array()
                .ok_or_else(|| "project.classifiers is not a list".to_owned())?
                .iter()
                .filter_map(toml::Value::as_str)
                .filter_map(|classifier| {
                    classifier
                        .strip_prefix("Programming Language :: Python :: 3.")?
                        .parse()
                        .ok()
                })
                .collect(),
        };

        let (requires, specifiers) = self.requires_python()?;
        let allowed = release_specifiers_to_ranges(specifiers);
        let oldest = match self.min_python()?.release() {
            [3, minor, ..] => *minor,
            [3] | [0..3, ..] => 0,
            // Above Python 3, so no minor version is expected.
            _ => u64::MAX,
        };
        let Some((_, upper)) = allowed.bounding_range() else {
            return Err(format!("requires-python ({requires}) allows no version"));
        };
        let (newest, source) = match (pinned, upper) {
            (Some(version), _) => match version.release() {
                [3, minor, ..] => (*minor, format!("{version} in {PYTHON_VERSION}")),
                _ => {
                    return Err(format!(
                        "{PYTHON_VERSION} pins {version}, which isn't Python 3"
                    ));
                }
            },
            (None, Bound::Included(version) | Bound::Excluded(version))
                if version.release().first() == Some(&3) =>
            {
                (
                    version.release().get(1).copied().unwrap_or_default(),
                    "its upper bound".to_owned(),
                )
            }
            (None, _) => {
                return Err(format!(
                    "No {PYTHON_VERSION} to give the newest supported Python, and requires-python \
                     ({requires}) has no upper bound within Python 3"
                ));
            }
        };

        let expected: BTreeSet<u64> = (oldest..=newest)
            .filter(|minor| {
                let series = VersionSpecifier::equals_star_version(Version::new([3, *minor]));
                !allowed.is_disjoint(&release_specifier_to_range(series))
            })
            .collect();
        if expected.is_empty() {
            return Err(format!(
                "requires-python ({requires}) allows no Python 3 version up to {source}"
            ));
        }
        if expected == listed {
            return Ok(());
        }
        let mut problems = Vec::new();
        let missing = versions(expected.difference(&listed));
        if !missing.is_empty() {
            problems.push(format!("missing {missing}"));
        }
        let unexpected = versions(listed.difference(&expected));
        if !unexpected.is_empty() {
            problems.push(format!("unexpected {unexpected}"));
        }
        Err(format!(
            "Expected classifiers for Python {}, from requires-python ({requires}) up to {source}: {}",
            versions(&expected),
            problems.join("; ")
        ))
    }

    /// Check that every dependency of the project, its extras and its dependency groups is pinned exactly, with a
    /// single `==` (or `===`) specifier without a wildcard.
    ///
    /// Dependencies that don't come from an index have no version to pin, so these pass: those given by a URL, and
    /// those whose `[tool.uv.sources]` entry is a path, Git repository, URL or workspace member. Dependencies the build
    /// backend fills in (`dynamic`) can't be checked, so they fail. The error lists each problem, by its table, with
    /// the reason.
    pub(crate) fn check_dependencies_exact(&self) -> Result<(), String> {
        let unindexed = self.unindexed_sources();
        let mut unpinned = Vec::new();
        let mut lists: Vec<(String, &toml::Value)> = Vec::new();

        if let Some(project) = &self.project {
            let dynamic = project.dynamic.as_ref().and_then(toml::Value::as_array);
            for field in dynamic
                .into_iter()
                .flatten()
                .filter_map(toml::Value::as_str)
            {
                if field == "dependencies" || field == "optional-dependencies" {
                    unpinned.push(format!("project.{field} (dynamic, so it can't be checked)"));
                }
            }

            if let Some(dependencies) = &project.dependencies {
                lists.push(("project.dependencies".to_owned(), dependencies));
            }
            if let Some(extras) = &project.optional_dependencies {
                match extras.as_table() {
                    Some(extras) => lists.extend(extras.iter().map(|(extra, list)| {
                        (format!("project.optional-dependencies.{extra}"), list)
                    })),
                    None => unpinned.push("project.optional-dependencies (not a table)".to_owned()),
                }
            }
        }
        lists.extend(
            self.dependency_groups
                .iter()
                .flatten()
                .map(|(group, list)| (format!("dependency-groups.{group}"), list)),
        );

        for (table, list) in lists {
            let Some(entries) = list.as_array() else {
                unpinned.push(format!("{table} (not a list)"));
                continue;
            };

            for entry in entries {
                if let Some(requirement) = entry.as_str() {
                    if let Err(reason) = check_requirement_exact(requirement, &unindexed) {
                        unpinned.push(format!("{table} \"{requirement}\" ({reason})"));
                    }
                } else if !(table.starts_with("dependency-groups.")
                    && entry.get("include-group").is_some())
                {
                    // A group's `{ include-group = "..." }` includes another group, which is checked itself.
                    unpinned.push(format!("{table} {entry} (not a requirement)"));
                }
            }
        }

        if unpinned.is_empty() {
            Ok(())
        } else {
            Err(format!("Not pinned with ==: {}", unpinned.join(", ")))
        }
    }

    /// The packages whose `[tool.uv.sources]` entry, or any of its alternatives, isn't an index.
    fn unindexed_sources(&self) -> Vec<PackageName> {
        let Some(sources) = self
            .tool
            .as_ref()
            .and_then(|t| t.uv.as_ref())
            .and_then(|uv| uv.sources.as_ref())
            .and_then(toml::Value::as_table)
        else {
            return Vec::new();
        };

        sources
            .iter()
            .filter(|(_, source)| {
                let entries: Vec<&toml::Value> = source
                    .as_array()
                    .map_or_else(|| vec![*source], |entries| entries.iter().collect());
                entries.iter().any(|entry| {
                    ["path", "git", "url", "workspace"]
                        .iter()
                        .any(|key| entry.get(key).is_some())
                })
            })
            .filter_map(|(name, _)| PackageName::from_str(name).ok())
            .collect()
    }
}

/// Python 3 minor versions as a list, such as `3.12, 3.14`.
fn versions<'a>(minors: impl IntoIterator<Item = &'a u64>) -> String {
    minors
        .into_iter()
        .map(|minor| format!("3.{minor}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Check that a PEP 508 requirement pins an exact version, unless the package comes from one of the `unindexed`
/// sources or a URL.
fn check_requirement_exact(requirement: &str, unindexed: &[PackageName]) -> Result<(), String> {
    let requirement = match Requirement::<VerbatimUrl>::from_str(requirement) {
        Ok(requirement) => requirement,
        // A URL that doesn't parse, such as a relative path uv accepts, still names no index.
        Err(error) if matches!(error.message, Pep508ErrorSource::UrlError(_)) => return Ok(()),
        Err(error) => return Err(format!("invalid: {}", error.message)),
    };

    if unindexed.contains(&requirement.name) {
        return Ok(());
    }

    match &requirement.version_or_url {
        Some(VersionOrUrl::Url(_)) => Ok(()),
        Some(VersionOrUrl::VersionSpecifier(specifiers))
            if matches!(
                &**specifiers,
                [specifier] if matches!(specifier.operator(), Operator::Equal | Operator::ExactEqual)
            ) =>
        {
            Ok(())
        }
        Some(VersionOrUrl::VersionSpecifier(_)) => Err("not a single == version"),
        None => Err("no version"),
    }
    .map_err(str::to_owned)
}

//
// Ruff
//

/// `[tool.ruff]` configuration.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct RuffConfig {
    pub line_length: Option<u32>,
    pub format: Option<RuffFormatConfig>,
    pub lint: Option<RuffLintConfig>,
}

/// `[tool.ruff.format]` configuration.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct RuffFormatConfig {
    pub line_ending: Option<String>,
}

/// `[tool.ruff.lint]` configuration.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct RuffLintConfig {
    pub preview: Option<bool>,
    pub select: Option<Vec<String>>,
    pub ignore: Option<Vec<String>>,
    pub pydocstyle: Option<RuffLintPydocstyleConfig>,
    pub isort: Option<RuffLintIsortConfig>,
    pub flake8_copyright: Option<RuffLintCopyrightConfig>,
}

impl RuffLintConfig {
    /// Whether the `select` list includes `"ALL"`.
    pub(crate) fn selects_all(&self) -> bool {
        self.select
            .as_ref()
            .is_some_and(|s| s.iter().any(|r| r == "ALL"))
    }

    /// Whether any rules are selected at all.
    pub(crate) fn has_rules(&self) -> bool {
        self.select.as_ref().is_some_and(|s| !s.is_empty())
    }

    /// Whether an ignore list is present and non-empty.
    pub(crate) fn has_ignores(&self) -> bool {
        self.ignore.as_ref().is_some_and(|i| !i.is_empty())
    }
}

/// `[tool.ruff.lint.pydocstyle]`.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct RuffLintPydocstyleConfig {
    pub convention: Option<String>,
}

/// `[tool.ruff.lint.isort]`.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct RuffLintIsortConfig {
    pub lines_between_types: Option<u32>,
}

/// `[tool.ruff.lint.flake8-copyright]`.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct RuffLintCopyrightConfig {
    pub notice_rgx: Option<String>,
}

//
// uv
//

/// `[tool.uv]` configuration.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct UvConfig {
    /// The newest release uv may resolve: a duration back from now, or a fixed date. Kept unparsed, so that a
    /// malformed value fails only the check reading it.
    pub exclude_newer: Option<toml::Value>,
    /// Whether uv builds and installs the project as a package. Kept unparsed, so that a malformed value fails only
    /// the check reading it.
    pub package: Option<toml::Value>,
    /// Where packages come from, when not from the default index. Kept unparsed, so that a malformed entry fails
    /// only the checks reading it.
    pub sources: Option<toml::Value>,
}

//
// Mypy
//

/// `[tool.mypy]` configuration.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct MypyConfig {
    pub strict: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(pyproject: &str) -> PyprojectToml {
        toml::from_str(pyproject).unwrap()
    }

    #[test]
    fn libraries_are_packages_without_commands() {
        assert!(parse("[project]\nname = 'lib'\n").is_library());
        assert!(
            !parse("[project]\nname = 'app'\n\n[project.scripts]\napp = 'app:main'\n").is_library()
        );
        assert!(!parse("[tool.ruff]\nline-length = 120\n").is_library());
        assert!(!parse("[project]\nname = 'service'\n\n[tool.uv]\npackage = false\n").is_library());
        assert!(parse("[project]\nname = 'lib'\n\n[tool.uv]\npackage = true\n").is_library());
    }

    /// Check a `pyproject.toml` whose classifiers list the 3.x `minors`, among other Python classifiers.
    fn check_classifiers(
        requires: &str,
        minors: &[u64],
        pinned: Option<&Version>,
    ) -> Result<(), String> {
        let classifiers: Vec<_> = minors
            .iter()
            .map(|minor| format!("\"Programming Language :: Python :: 3.{minor}\""))
            .chain([
                "\"Programming Language :: Python :: 3\"".to_owned(),
                "\"Programming Language :: Python :: 3 :: Only\"".to_owned(),
                "\"Typing :: Typed\"".to_owned(),
            ])
            .collect();
        parse(&format!(
            "[project]\nrequires-python = \"{requires}\"\nclassifiers = [{}]\n",
            classifiers.join(", ")
        ))
        .check_python_classifiers(pinned)
    }

    #[test]
    fn classifiers_match_requires_python() {
        let pin = Version::new([3, 15, 2]);
        for (requires, minors) in [
            ("==3.15.2", &[15][..]),
            ("===3.15.2", &[15]),
            ("==3.15.*", &[15]),
            (">=3.12", &[12, 13, 14, 15]),
            (">3.12", &[12, 13, 14, 15]),
            (">=3.12.0rc1", &[12, 13, 14, 15]),
            ("~=3.13", &[13, 14, 15]),
            ("~=3.13.1", &[13]),
            (">=3.12, !=3.13.*", &[12, 14, 15]),
            (">=3.11, <3.14", &[11, 12, 13]),
            (">=3.11, <=3.14", &[11, 12, 13, 14]),
            (">=2.7", &(0..=15).collect::<Vec<_>>()),
        ] {
            assert_eq!(
                check_classifiers(requires, minors, Some(&pin)),
                Ok(()),
                "{requires}"
            );
        }
        // Without a pin, an upper bound within Python 3 gives the newest version.
        for (requires, minors) in [
            (">=3.11, <3.14", &[11, 12, 13][..]),
            (">=3.11, <=3.14", &[11, 12, 13, 14]),
            ("==3.15.2", &[15]),
        ] {
            assert_eq!(
                check_classifiers(requires, minors, None),
                Ok(()),
                "{requires}"
            );
        }
        for requires in [">=3.12", ">=3.12, <4", "~=3.13"] {
            assert_eq!(
                check_classifiers(requires, &[13], None),
                Err(format!(
                    "No .python-version to give the newest supported Python, and requires-python \
                     ({requires}) has no upper bound within Python 3"
                )),
                "{requires}"
            );
        }
    }

    #[test]
    fn classifier_mismatches_are_reported() {
        let pin = Version::new([3, 15, 2]);
        assert_eq!(
            check_classifiers(">=3.12, !=3.13.*", &[11, 12, 13, 14], Some(&pin)),
            Err(
                "Expected classifiers for Python 3.12, 3.14, 3.15, from requires-python \
                 (>=3.12, !=3.13.*) up to 3.15.2 in .python-version: missing 3.15; unexpected \
                 3.11, 3.13"
                    .to_owned()
            )
        );
        assert_eq!(
            check_classifiers("==3.15.2", &[], None),
            Err(
                "Expected classifiers for Python 3.15, from requires-python (==3.15.2) up to its \
                 upper bound: missing 3.15"
                    .to_owned()
            )
        );
        assert_eq!(
            parse("[project]\nrequires-python = \">=3.14\"\n").check_python_classifiers(Some(&pin)),
            Err(
                "Expected classifiers for Python 3.14, 3.15, from requires-python (>=3.14) up to \
                 3.15.2 in .python-version: missing 3.14, 3.15"
                    .to_owned()
            )
        );
        for requires in [">=3.16", ">=4"] {
            assert_eq!(
                check_classifiers(requires, &[16], Some(&pin)),
                Err(format!(
                    "requires-python ({requires}) allows no Python 3 version up to 3.15.2 in \
                     .python-version"
                )),
                "{requires}"
            );
        }
        assert_eq!(
            check_classifiers(">=3.12", &[12], Some(&Version::new([4, 0]))),
            Err(".python-version pins 4.0, which isn't Python 3".to_owned())
        );
        assert_eq!(
            check_classifiers("<3.14", &[13], Some(&pin)),
            Err("requires-python (<3.14) sets no lower bound".to_owned())
        );
        assert_eq!(
            check_classifiers(">=3.14, <3.12", &[], Some(&pin)),
            Err("requires-python (>=3.14, <3.12) allows no version".to_owned())
        );
        assert_eq!(
            parse("[project]\nclassifiers = []\n").check_python_classifiers(Some(&pin)),
            Err("pyproject.toml sets no requires-python".to_owned())
        );
        assert_eq!(
            parse("[project]\nrequires-python = \">=3.12\"\nclassifiers = \"3.12\"\n")
                .check_python_classifiers(Some(&pin)),
            Err("project.classifiers is not a list".to_owned())
        );
        assert_eq!(
            parse("[project]\nrequires-python = \">=3.12\"\ndynamic = [\"classifiers\"]\n")
                .check_python_classifiers(Some(&pin)),
            Err("project.classifiers is dynamic, so it can't be checked".to_owned())
        );
    }

    #[test]
    fn requires_python_agrees_with_the_pin() {
        let pin = Version::new([3, 15, 2]);
        let application = |requires: &str| {
            parse(&format!(
                "[project]\nrequires-python = \"{requires}\"\n[project.scripts]\ndemo = \"demo:main\"\n"
            ))
            .check_requires_python_pin(&pin)
        };
        let library = |requires: &str| {
            parse(&format!("[project]\nrequires-python = \"{requires}\"\n"))
                .check_requires_python_pin(&pin)
        };

        for requires in ["==3.15.2", "== 3.15.2", "==3.15.2.0"] {
            assert_eq!(application(requires), Ok(()), "{requires}");
        }
        for requires in [
            "==3.15.1",
            "==3.15.*",
            "===3.15.2",
            ">=3.15",
            "==3.15.2, <4",
        ] {
            assert_eq!(
                application(requires),
                Err(format!(
                    "An application's requires-python ({requires}) isn't ==3.15.2, the version in \
                     .python-version"
                )),
                "{requires}"
            );
        }
        for requires in [
            ">=3.12",
            ">=3.15.2",
            "==3.15.*",
            ">=3.12, <3.16",
            "~=3.15.0",
        ] {
            assert_eq!(library(requires), Ok(()), "{requires}");
        }
        for requires in [
            ">=3.12, <3.15",
            ">3.15.2",
            ">=3.12, !=3.15.*",
            ">=3.12, !=3.15.2",
            "==3.14.*",
        ] {
            assert_eq!(
                library(requires),
                Err(format!(
                    "requires-python ({requires}) doesn't allow 3.15.2, the version in .python-version"
                )),
                "{requires}"
            );
        }
        assert_eq!(
            parse("[project]\n").check_requires_python_pin(&pin),
            Err("pyproject.toml sets no requires-python".to_owned())
        );
    }

    #[test]
    fn min_python_is_the_lower_bound() {
        for (requires, min) in [
            (">=3.12", "3.12"),
            (">=3.10, <4", "3.10"),
            ("~=3.11.2", "3.11.2"),
            ("==3.15.0", "3.15.0"),
            ("==3.13.*", "3.13"),
            (">=3.10,>=3.12,!=3.12.1", "3.12"),
            (">=3.12, !=3.12.*", "3.13"),
            (">=3.12, !=3.12.0", "3.12.1"),
            (">3.12", "3.12.1"),
            (">3.11.4, <4", "3.11.5"),
            (">=3.12.0rc1", "3.12.0"),
            ("===3.15.2", "3.15.2"),
            (">=2.7", "2.7"),
            (">=3.12, !=3.12.0, !=3.12.1", "3.12.2"),
            (">3.12, !=3.12.1", "3.12.2"),
            (">=3.12, !=3.12", "3.12.1"),
            (">3", "3.0.1"),
            (">3.12.0.5", "3.12.1"),
            (">=3.12.0.5", "3.12.1"),
            (">=1!3.12", "3.12"),
        ] {
            let pyproject = parse(&format!("[project]\nrequires-python = \"{requires}\"\n"));
            assert_eq!(
                pyproject.min_python(),
                Ok(min.parse().unwrap()),
                "{requires}"
            );
        }
        assert_eq!(
            parse("[project]\n").min_python(),
            Err("pyproject.toml sets no requires-python".to_owned())
        );
        assert_eq!(
            parse("[project]\nrequires-python = \"<4\"\n").min_python(),
            Err("requires-python (<4) sets no lower bound".to_owned())
        );
        for requires in [
            ">=3.14, <3.12",
            ">3.12, <3.12.1",
            ">3.12.18446744073709551615, <3.13",
        ] {
            assert_eq!(
                parse(&format!("[project]\nrequires-python = \"{requires}\"\n")).min_python(),
                Err(format!("requires-python ({requires}) allows no version")),
                "{requires}"
            );
        }
        assert!(
            parse("[project]\nrequires-python = \">=three\"\n")
                .min_python()
                .is_err_and(|error| error.starts_with("Invalid requires-python: "))
        );
    }

    #[test]
    fn applications_install_a_command() {
        assert!(parse("[project.scripts]\ndemo = \"demo.main:main\"\n").is_application());
        assert!(parse("[project.gui-scripts]\ndemo = \"demo.main:main\"\n").is_application());
        assert!(!parse("[project]\nname = \"demo\"\n").is_application());
        assert!(!parse("[project.scripts]\n").is_application());
        assert!(!parse("").is_application());
    }

    #[test]
    fn pinned_pyproject_passes() {
        let pyproject = parse(
            r#"
            [project]
            dependencies = [
              "pydantic[email]==2.12.0",
              "tomli==2.3.0; python_version < '3.11'",
              "legacy===1.0",
              "short==2.12",
              "direct @ https://example.com/direct-1.0.tar.gz",
              "local",
              "forked",
              "Some.Pkg",
              "member",
              "remote",
              "relative @ ../relative",
            ]

            [project.optional-dependencies]
            cli = ["click==8.3.0"]

            [dependency-groups]
            dev = ["ruff==0.17.3", { include-group = "test" }]
            test = ["pytest==9.2.0"]

            [tool.uv.sources]
            local = { path = "../local", editable = true }
            forked = [{ git = "https://example.com/forked.git", marker = "sys_platform == 'linux'" }]
            some_pkg = { path = "../some-pkg" }
            member = { workspace = true }
            remote = { url = "https://example.com/remote-1.0.tar.gz" }
            "#,
        );

        assert_eq!(pyproject.check_dependencies_exact(), Ok(()));
    }

    #[test]
    fn lists_every_unpinned_dependency() {
        let pyproject = parse(
            r#"
            [project]
            dependencies = ["pydantic>=2.12", "Indexed", "wild==2.*", "two==1.0.0,!=1.0.1", "not a requirement!"]

            [project.optional-dependencies]
            cli = ["click~=8.3"]

            [dependency-groups]
            dev = ["ruff"]
            lint = ["mypy>=2", { include-group = "dev" }, 5]
            test = ["pytest"]

            [tool.uv.sources]
            indexed = [{ index = "private" }]
            "#,
        );

        let Err(error) = pyproject.check_dependencies_exact() else {
            panic!("expected unpinned dependencies");
        };
        let expected = [
            r#"project.dependencies "pydantic>=2.12" (not a single == version)"#,
            r#"project.dependencies "Indexed" (no version)"#,
            r#"project.dependencies "wild==2.*" (not a single == version)"#,
            r#"project.dependencies "two==1.0.0,!=1.0.1" (not a single == version)"#,
            r#"project.optional-dependencies.cli "click~=8.3" (not a single == version)"#,
            r#"dependency-groups.dev "ruff" (no version)"#,
            r#"dependency-groups.lint "mypy>=2" (not a single == version), dependency-groups.lint 5 (not a requirement), dependency-groups.test "pytest" (no version)"#,
        ];
        for entry in expected {
            assert!(error.contains(entry), "{entry} missing from {error}");
        }
        assert!(error.starts_with("Not pinned with ==: "), "{error}");
        assert!(
            error.contains(r#""not a requirement!" (invalid: "#),
            "{error}"
        );
    }

    #[test]
    fn malformed_fields_fail_only_this_check() {
        let pyproject = parse(
            r#"
            [project]
            name = "demo"
            dependencies = "requests"
            optional-dependencies = ["cli"]

            [project.scripts]
            demo = { reference = "demo:main" }

            [dependency-groups]
            dev = "ruff"

            [tool.uv]
            sources = []
            "#,
        );

        assert!(pyproject.is_application());
        assert_eq!(
            pyproject.check_dependencies_exact(),
            Err("Not pinned with ==: project.optional-dependencies (not a table), project.dependencies (not a list), \
                 dependency-groups.dev (not a list)"
                .to_owned())
        );
    }

    #[test]
    fn dynamic_dependencies_cannot_be_checked() {
        let pyproject = parse(
            "[project]\ndynamic = [\"version\", \"dependencies\", \"optional-dependencies\"]\n",
        );

        assert_eq!(
            pyproject.check_dependencies_exact(),
            Err(
                "Not pinned with ==: project.dependencies (dynamic, so it can't be checked), \
                 project.optional-dependencies (dynamic, so it can't be checked)"
                    .to_owned()
            )
        );
        assert_eq!(
            parse("[project]\ndynamic = [\"version\"]\n").check_dependencies_exact(),
            Ok(())
        );
    }
}
