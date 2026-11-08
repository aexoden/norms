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
use crate::report::Issue;

/// Top-level `pyproject.toml` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct PyprojectToml {
    pub project: Option<ProjectSection>,
    pub tool: Option<ToolSection>,
    pub build_system: Option<BuildSystem>,
    /// Each group's requirements and `include-group` tables. Kept unparsed, so that a malformed group fails only the
    /// checks reading it.
    pub dependency_groups: Option<BTreeMap<String, toml::Value>>,
}

/// The classifier that marks a project as never to be published: the Python Package Index rejects any upload carrying
/// it.
const PRIVATE_CLASSIFIER: &str = "Private :: Do Not Upload";

/// The `[project]` table.
///
/// The fields that only the dependency checks read are kept unparsed, so that a malformed one fails only those checks.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct ProjectSection {
    pub name: Option<String>,
    /// A static version, which the standards replace with one from git tags. Kept unparsed, as only its presence
    /// matters.
    pub version: Option<toml::Value>,
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
    pub hatch: Option<HatchConfig>,
}

/// The `[build-system]` table. Its fields are kept unparsed, so that a malformed one fails only the check reading it.
#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub(crate) struct BuildSystem {
    /// The requirements of the build backend.
    pub requires: Option<toml::Value>,
    /// The module path of the build backend.
    pub build_backend: Option<toml::Value>,
}

/// The `[tool.hatch]` table.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct HatchConfig {
    /// `[tool.hatch.version]`, where the version comes from. Kept unparsed, so that a malformed value fails only the
    /// check reading it.
    pub version: Option<toml::Value>,
}

/// The `[tool.uv] cache-keys` entry that makes uv reinstall the project when `pyproject.toml` changes, which any list
/// of cache keys must repeat, since it replaces uv's default list.
const PYPROJECT_CACHE_KEY: &str = "{ file = \"pyproject.toml\" }";

/// The `[tool.uv] cache-keys` entry that makes uv reinstall the project when HEAD or the tags move, so that the version
/// hatch-vcs derives stays current.
const GIT_CACHE_KEY: &str = "{ git = { commit = true, tags = true } }";

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
        self.project.is_some() && !self.is_unpackaged() && !self.is_application()
    }

    /// Whether uv is told not to build and install the project as a package, with `[tool.uv] package = false`.
    pub(crate) fn is_unpackaged(&self) -> bool {
        self.tool
            .as_ref()
            .and_then(|tool| tool.uv.as_ref())
            .and_then(|uv| uv.package.as_ref())
            .is_some_and(|package| package.as_bool() == Some(false))
    }

    /// Whether the project has a version for the build backend to give it: it has a `[project]` table, unlike a
    /// virtual workspace root, and isn't marked `[tool.uv] package = false`.
    pub(crate) fn has_version(&self) -> bool {
        self.project.is_some() && !self.is_unpackaged()
    }

    /// Check that the project leaves its version to the build backend: `version` is listed in `[project] dynamic`
    /// rather than set in `[project]`.
    pub(crate) fn check_version_dynamic(&self) -> Result<(), String> {
        let project = self.project.as_ref();
        if project.is_some_and(|p| p.version.is_some()) {
            return Err("[project] sets a static version".to_owned());
        }
        let dynamic = project
            .and_then(|p| p.dynamic.as_ref())
            .and_then(toml::Value::as_array);
        if dynamic
            .into_iter()
            .flatten()
            .any(|field| field.as_str() == Some("version"))
        {
            Ok(())
        } else {
            Err("project.dynamic doesn't list version".to_owned())
        }
    }

    /// Check that the project builds with hatchling and hatch-vcs: `[build-system]` requires both, at any version, and
    /// names `hatchling.build` as its backend. Lacking hatch-vcs while hatchling builds the project and
    /// `[tool.hatch.version]` takes the version from git fails, since the build can't load it; any other difference
    /// warns.
    pub(crate) fn check_build_system(&self) -> Result<(), Issue> {
        let Some(build) = &self.build_system else {
            return Err(Issue::warn("No [build-system] table"));
        };
        let mut issues = Vec::new();
        let names: Vec<PackageName> = match build.requires.as_ref().map(toml::Value::as_array) {
            None => Vec::new(),
            Some(None) => {
                issues.push("build-system.requires isn't a list".to_owned());
                Vec::new()
            }
            Some(Some(requires)) => requires
                .iter()
                .filter_map(|entry| {
                    let parsed = entry.as_str().map(Requirement::<VerbatimUrl>::from_str);
                    match parsed {
                        Some(Ok(requirement)) => Some(requirement.name),
                        Some(Err(error)) => {
                            issues.push(format!(
                                "build-system.requires has an invalid requirement {entry}: {}",
                                error.message
                            ));
                            None
                        }
                        None => {
                            issues.push(format!(
                                "build-system.requires has {entry}, not a requirement"
                            ));
                            None
                        }
                    }
                })
                .collect(),
        };
        let missing: Vec<&str> = ["hatch-vcs", "hatchling"]
            .into_iter()
            .filter(|name| !names.iter().any(|known| known.as_ref() == *name))
            .collect();
        let mut issues: Vec<_> = issues.into_iter().map(Issue::warn).collect();
        if !missing.is_empty() {
            let message = format!("build-system.requires lacks {}", missing.join(" and "));
            let hatchling = build.build_backend.as_ref().and_then(toml::Value::as_str)
                == Some("hatchling.build");
            issues.push(
                if missing.contains(&"hatch-vcs")
                    && hatchling
                    && self.hatch_version_source() == Some("vcs")
                {
                    Issue::fail(message)
                } else {
                    Issue::warn(message)
                },
            );
        }
        match build.build_backend.as_ref().map(toml::Value::as_str) {
            Some(Some("hatchling.build")) => {}
            Some(Some(backend)) => {
                issues.push(Issue::warn(format!(
                    "build-system.build-backend is {backend}, not hatchling.build"
                )));
            }
            Some(None) => issues.push(Issue::warn("build-system.build-backend isn't a string")),
            None => issues.push(Issue::warn("build-system.build-backend isn't set")),
        }
        Issue::combine(issues)
    }

    /// The `source` of `[tool.hatch.version]`, if it sets one.
    fn hatch_version_source(&self) -> Option<&str> {
        self.tool
            .as_ref()
            .and_then(|tool| tool.hatch.as_ref())
            .and_then(|hatch| hatch.version.as_ref())
            .and_then(|version| version.get("source"))
            .and_then(toml::Value::as_str)
    }

    /// Check that hatch takes the version from git: `[tool.hatch.version] source = "vcs"`. A project below the root of
    /// its repository also needs `raw-options = { search_parent_directories = true }`, without which hatch-vcs looks
    /// for the repository only in the project's own directory, and so fails to build, which fails; any other
    /// difference warns.
    pub(crate) fn check_hatch_version(&self, in_subdirectory: bool) -> Result<(), Issue> {
        let Some(version) = self
            .tool
            .as_ref()
            .and_then(|tool| tool.hatch.as_ref())
            .and_then(|hatch| hatch.version.as_ref())
        else {
            return Err(Issue::warn("No [tool.hatch.version] table"));
        };
        let mut issues = Vec::new();
        let vcs = version.get("source").and_then(toml::Value::as_str) == Some("vcs");
        if !vcs {
            issues.push(Issue::warn("[tool.hatch.version] source isn't \"vcs\""));
        }
        if in_subdirectory
            && vcs
            && version
                .get("raw-options")
                .and_then(|options| options.get("search_parent_directories"))
                .and_then(toml::Value::as_bool)
                != Some(true)
        {
            issues.push(Issue::fail(
                "[tool.hatch.version] raw-options doesn't set search_parent_directories = true, which a project \
                 below the repository root needs",
            ));
        }
        Issue::combine(issues)
    }

    /// `[tool.uv] required-version`, parsed as uv parses it, or why it can't be.
    pub(crate) fn uv_required_version(&self) -> Result<VersionSpecifiers, String> {
        let Some(required) = self
            .tool
            .as_ref()
            .and_then(|tool| tool.uv.as_ref())
            .and_then(|uv| uv.required_version.as_ref())
        else {
            return Err("[tool.uv] sets no required-version".to_owned());
        };
        let Some(required) = required.as_str() else {
            return Err("[tool.uv] required-version isn't a string".to_owned());
        };
        // uv reads a bare version as that exact version.
        VersionSpecifiers::from_str(required).or_else(|error| {
            Version::from_str(required.trim())
                .map(|version| VersionSpecifier::equals_version(version).into())
                .map_err(|_not_a_version| format!("Invalid [tool.uv] required-version: {error}"))
        })
    }

    /// Check that `[tool.uv] required-version` sets only the oldest uv the project needs, as a single `>=`, so that
    /// newer releases of uv keep working.
    pub(crate) fn check_uv_required_version(&self) -> Result<(), String> {
        let specifiers = self.uv_required_version()?;
        match &*specifiers {
            [specifier] if *specifier.operator() == Operator::GreaterThanEqual => Ok(()),
            _ => Err(format!(
                "[tool.uv] required-version ({specifiers}) isn't a single >= minimum"
            )),
        }
    }

    /// Check that `[tool.uv] cache-keys` makes uv reinstall the project when `pyproject.toml` changes and when HEAD or
    /// the tags move, so that the installed version stays current. Other keys may be listed too.
    pub(crate) fn check_uv_cache_keys(&self) -> Result<(), String> {
        let Some(keys) = self
            .tool
            .as_ref()
            .and_then(|tool| tool.uv.as_ref())
            .and_then(|uv| uv.cache_keys.as_ref())
        else {
            return Err("[tool.uv] sets no cache-keys".to_owned());
        };
        let Some(keys) = keys.as_array() else {
            return Err("[tool.uv] cache-keys isn't a list".to_owned());
        };
        let pyproject = keys
            .iter()
            .any(|key| key.get("file").and_then(toml::Value::as_str) == Some("pyproject.toml"));
        let git = keys.iter().any(|key| {
            let git = key.get("git");
            let flag = |name| {
                git.and_then(|git| git.get(name))
                    .and_then(toml::Value::as_bool)
                    == Some(true)
            };
            flag("commit") && flag("tags")
        });
        let missing: Vec<&str> = [(pyproject, PYPROJECT_CACHE_KEY), (git, GIT_CACHE_KEY)]
            .into_iter()
            .filter(|(present, _)| !present)
            .map(|(_, key)| key)
            .collect();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "[tool.uv] cache-keys lacks {}",
                missing.join(" and ")
            ))
        }
    }

    /// Whether the project is published: it isn't marked `Private :: Do Not Upload`, the classifier that the Python
    /// Package Index rejects on upload.
    pub(crate) fn is_publishable(&self) -> bool {
        !self
            .project
            .as_ref()
            .and_then(|project| project.classifiers.as_ref())
            .and_then(toml::Value::as_array)
            .is_some_and(|classifiers| {
                classifiers
                    .iter()
                    .any(|classifier| classifier.as_str() == Some(PRIVATE_CLASSIFIER))
            })
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
    /// The uv versions the project accepts. Kept unparsed, so that a malformed value fails only the checks reading it.
    pub required_version: Option<toml::Value>,
    /// What uv's cached build of the project depends on, so that a change to any of them reinstalls it. Kept
    /// unparsed, so that a malformed entry fails only the check reading it.
    pub cache_keys: Option<toml::Value>,
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

    #[test]
    fn versions_come_from_git_through_hatch_vcs() {
        let conforming = parse(
            r#"
[project]
dynamic = ["version"]

[build-system]
requires = ["hatch-vcs>=0.4", "Hatchling"]
build-backend = "hatchling.build"

[tool.hatch.version]
source = "vcs"

[tool.uv]
cache-keys = [{ file = "pyproject.toml" }, { file = "README.md" }, { git = { commit = true, tags = true } }]
"#,
        );
        assert_eq!(conforming.check_version_dynamic(), Ok(()));
        assert_eq!(conforming.check_build_system(), Ok(()));
        assert_eq!(conforming.check_hatch_version(false), Ok(()));
        assert_eq!(conforming.check_uv_cache_keys(), Ok(()));
        assert_eq!(
            conforming.check_hatch_version(true),
            Err(Issue::fail(
                "[tool.hatch.version] raw-options doesn't set search_parent_directories = true, which a project \
                 below the repository root needs"
            ))
        );
        let nested = parse(
            "[tool.hatch.version]\nsource = 'vcs'\nraw-options = { search_parent_directories = true }\n",
        );
        assert_eq!(nested.check_hatch_version(true), Ok(()));

        let drifted = parse(
            r#"
[project]
version = "1.0.0"
dynamic = ["readme"]

[build-system]
requires = ["setuptools", "hatchling"]
build-backend = "setuptools.build_meta"

[tool.hatch.version]
path = "src/demo/__about__.py"

[tool.uv]
cache-keys = [{ git = { commit = true } }]
"#,
        );
        assert_eq!(
            drifted.check_version_dynamic(),
            Err("[project] sets a static version".to_owned())
        );
        assert_eq!(
            parse("[project]\ndynamic = ['readme']\n").check_version_dynamic(),
            Err("project.dynamic doesn't list version".to_owned())
        );
        assert_eq!(
            drifted.check_build_system(),
            Err(Issue::warn(
                "build-system.requires lacks hatch-vcs; build-system.build-backend is setuptools.build_meta, not \
                 hatchling.build"
            ))
        );
        assert_eq!(
            drifted.check_hatch_version(false),
            Err(Issue::warn("[tool.hatch.version] source isn't \"vcs\""))
        );
        assert_eq!(
            drifted.check_uv_cache_keys(),
            Err(format!(
                "[tool.uv] cache-keys lacks {PYPROJECT_CACHE_KEY} and {GIT_CACHE_KEY}"
            ))
        );

        let empty = parse("");
        assert_eq!(
            empty.check_build_system(),
            Err(Issue::warn("No [build-system] table"))
        );
        assert_eq!(
            empty.check_hatch_version(false),
            Err(Issue::warn("No [tool.hatch.version] table"))
        );
        assert_eq!(
            empty.check_uv_cache_keys(),
            Err("[tool.uv] sets no cache-keys".to_owned())
        );
        assert_eq!(
            parse("[build-system]\nrequires = 'hatchling'\nbuild-backend = 1\n")
                .check_build_system(),
            Err(Issue::warn(
                "build-system.requires isn't a list; build-system.requires lacks hatch-vcs and hatchling; \
                 build-system.build-backend isn't a string"
            ))
        );
        assert_eq!(
            parse("[tool.uv]\ncache-keys = 'pyproject.toml'\n").check_uv_cache_keys(),
            Err("[tool.uv] cache-keys isn't a list".to_owned())
        );
        assert!(parse("[tool.uv]\npackage = false\n").is_unpackaged());
        assert!(!parse("[tool.uv]\npackage = true\n").is_unpackaged());
    }

    #[test]
    fn versioning_issues_that_break_the_build_fail() {
        assert_eq!(
            parse(
                "[build-system]\nrequires = ['hatchling']\nbuild-backend = 'hatchling.build'\n\n\
                 [tool.hatch.version]\nsource = 'vcs'\n"
            )
            .check_build_system(),
            Err(Issue::fail("build-system.requires lacks hatch-vcs"))
        );
        assert_eq!(
            parse("[tool.hatch.version]\npath = 'src/demo/__about__.py'\n")
                .check_hatch_version(true),
            Err(Issue::warn("[tool.hatch.version] source isn't \"vcs\""))
        );
    }

    #[test]
    fn versioning_issues_are_named() {
        assert_eq!(
            parse("[build-system]\nrequires = ['hatch-vcs', 'hatchling']\n").check_build_system(),
            Err(Issue::warn("build-system.build-backend isn't set"))
        );
        let requires = parse(
            "[build-system]\nrequires = ['hatch-vcs >= 0.4,', 1, 'hatchling']\nbuild-backend = 'hatchling.build'\n",
        )
        .check_build_system()
        .unwrap_err()
        .message;
        assert!(
            requires.starts_with(
                "build-system.requires has an invalid requirement \"hatch-vcs >= 0.4,\": "
            ),
            "{requires}"
        );
        assert!(
            requires.ends_with(
                "; build-system.requires has 1, not a requirement; build-system.requires lacks hatch-vcs"
            ),
            "{requires}"
        );
        assert_eq!(
            parse("[tool.hatch.version]\nsource = 'vcs'\nraw-options = { search_parent_directories = false }\n")
                .check_hatch_version(true),
            Err(Issue::fail("[tool.hatch.version] raw-options doesn't set search_parent_directories = true, which a project \
                 below the repository root needs"))
        );
        assert_eq!(
            parse(
                "[tool.uv]\ncache-keys = [{ file = 'pyproject.toml' }, { git = { tags = true } }]\n"
            )
            .check_uv_cache_keys(),
            Err(format!("[tool.uv] cache-keys lacks {GIT_CACHE_KEY}"))
        );
        assert!(parse("[project]\nname = 'lib'\n").has_version());
        assert!(!parse("[project]\nname = 'svc'\n\n[tool.uv]\npackage = false\n").has_version());
        assert!(!parse("[tool.uv.workspace]\nmembers = ['packages/*']\n").has_version());
    }

    #[test]
    fn uv_required_version_is_a_minimum() {
        let required = |value: &str| parse(&format!("[tool.uv]\nrequired-version = {value}\n"));
        assert_eq!(required("'>=0.9.5'").check_uv_required_version(), Ok(()));
        assert_eq!(required("'>= 0.9'").check_uv_required_version(), Ok(()));
        for (value, written) in [
            ("'==0.9.5'", "==0.9.5"),
            ("'>=0.9, <0.10'", ">=0.9, <0.10"),
            ("'~=0.9.5'", "~=0.9.5"),
        ] {
            assert_eq!(
                required(value).check_uv_required_version(),
                Err(format!(
                    "[tool.uv] required-version ({written}) isn't a single >= minimum"
                ))
            );
        }
        assert_eq!(
            parse("").check_uv_required_version(),
            Err("[tool.uv] sets no required-version".to_owned())
        );
        assert_eq!(
            required("0.9").check_uv_required_version(),
            Err("[tool.uv] required-version isn't a string".to_owned())
        );
        assert_eq!(
            required("'0.9.5'").check_uv_required_version(),
            Err("[tool.uv] required-version (==0.9.5) isn't a single >= minimum".to_owned())
        );
        assert!(
            required("'0.9.5,'")
                .check_uv_required_version()
                .unwrap_err()
                .starts_with("Invalid [tool.uv] required-version: ")
        );
    }

    #[test]
    fn private_projects_are_not_publishable() {
        assert!(parse("[project]\nname = 'lib'\n").is_publishable());
        assert!(parse("").is_publishable());
        assert!(
            parse("[project]\nclassifiers = ['License :: OSI Approved :: MIT License']\n")
                .is_publishable()
        );
        assert!(
            !parse("[project]\nclassifiers = ['Typing :: Typed', 'Private :: Do Not Upload']\n")
                .is_publishable()
        );
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
