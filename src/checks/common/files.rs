// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Checks of the repository-wide files every project has, whatever its languages: norms.toml, the README,
//! licenses, git and editor configuration, Devbox, Markdown and Renovate.

use std::path::Path;
use std::sync::LazyLock;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use regex::Regex;

use crate::checks::{python, rust};
use crate::config::{
    AttributeState, DevboxConfig, EditorConfig, MarkdownlintCli2Config, SHARED_PRESET, attribute,
};
use crate::context::{ComponentContext, ConfigStatus, MARKDOWNLINT_CLI2_CONFIG, ProjectContext};
use crate::models::Language;
use crate::report::{Issue, Outcome, Report, file_exists, matches_regex};

//
// norms configuration
//

/// Check `norms.toml`, which is optional: most projects have no waivers or exclusions.
pub(crate) fn check_norms_config(ctx: &ProjectContext, report: &mut Report) {
    let status = ctx.norms();

    if matches!(status, ConfigStatus::NotFound) {
        return;
    }

    let Some(config) = report.require_parsed("norms-toml", "norms.toml", status) else {
        return;
    };

    let unexplained = config.unexplained_waivers();

    report.must(
        "norms-toml-waiver-reasons",
        "norms.toml: every waiver has a reason",
        if unexplained.is_empty() {
            Ok(())
        } else {
            Err(format!("No reason given for: {}", unexplained.join(", ")))
        },
    );

    let duplicates = config.duplicate_waivers();

    report.must(
        "norms-toml-waiver-duplicates",
        "norms.toml: no check is waived twice",
        if duplicates.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "Waived more than once: {}",
                duplicates.into_iter().collect::<Vec<_>>().join(", ")
            ))
        },
    );

    let invalid = config.invalid_excludes();

    report.must(
        "norms-toml-exclude-paths",
        "norms.toml: every excluded directory is a plain relative path",
        if invalid.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "Not a plain relative path: {}",
                invalid
                    .iter()
                    .map(|path| format!("{path:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        },
    );

    for path in config.excluded_paths() {
        report.should(
            "norms-toml-exclude-exists",
            format!("norms.toml: excluded directory {} exists", path.display()),
            if ctx.path().join(&path).is_dir() {
                Outcome::Pass
            } else {
                Outcome::fail("No such directory")
            },
        );
    }
}

//
// Components
//

/// Check that the search for components covered the whole project.
pub(crate) fn check_components(ctx: &ProjectContext, report: &mut Report) {
    let errors = &ctx.detection().errors;

    report.should(
        "components-search",
        "Components: every directory searched",
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        },
    );
}

//
// Required files
//

pub(crate) fn check_readme(ctx: &ProjectContext, report: &mut Report) {
    report.must(
        "readme-exists",
        "README.md",
        file_exists(&ctx.path().join("README.md")),
    );
}

/// The import of `AGENTS.md` in `CLAUDE.md`, as the standards write it on a line of its own.
const AGENTS_IMPORT: &str = "@AGENTS.md";

/// Whether `content`, the text of a `CLAUDE.md`, imports `AGENTS.md` as the standards write it: a line outside fenced
/// code blocks, which Claude Code doesn't read imports from, is [`AGENTS_IMPORT`] alone, unindented, so that neither
/// an indented code block nor an import within a sentence, a code span or an HTML comment on that line counts.
fn imports_agents(content: &str) -> bool {
    let mut fenced = false;
    content.lines().any(|line| {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            return false;
        }
        !fenced && line.trim_end() == AGENTS_IMPORT
    })
}

/// Whether `a` and `b` are the same file, as a symlink and its target are.
fn same_file(a: &Path, b: &Path) -> bool {
    matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

/// Check that the root `CLAUDE.md` beside an `AGENTS.md` imports it rather than repeating it. Two files that are one,
/// through a symlink either way, which Claude Code also reads, warn, so that every project has the one form, with room
/// for instructions specific to Claude. A `.claude/CLAUDE.md` isn't read.
pub(crate) fn check_agent_guidance(ctx: &ProjectContext, report: &mut Report) {
    let agents = ctx.path().join("AGENTS.md");
    if !agents.is_file() {
        return;
    }
    let outcome = match ctx.claude_md() {
        ConfigStatus::NotFound => return,
        ConfigStatus::ParseError { error } => Err(error.clone()),
        ConfigStatus::Ok(_) if same_file(&ctx.path().join("CLAUDE.md"), &agents) => Err(
            "CLAUDE.md and AGENTS.md are the same file, through a symlink: make CLAUDE.md a file of its own that has \
             @AGENTS.md on a line of its own"
                .to_owned(),
        ),
        ConfigStatus::Ok(content) if imports_agents(content) => Ok(()),
        ConfigStatus::Ok(_) => Err(
            "AGENTS.md isn't imported: add @AGENTS.md at the start of a line of its own, outside code"
                .to_owned(),
        ),
    };
    report.should(
        "claude-md-imports-agents",
        "CLAUDE.md: imports AGENTS.md",
        outcome,
    );
}

/// Check for `CHANGELOG.md`, which a project needs if it publishes versioned releases: if it has a Rust or Python
/// library component that may be published, or a release tag. An application deployed without releases needs none,
/// so for it the check passes.
pub(crate) fn check_changelog(ctx: &ProjectContext, report: &mut Report) {
    if ctx.path().join("CHANGELOG.md").exists() {
        report.must("changelog-exists", "CHANGELOG.md", Outcome::Pass);
        return;
    }

    let releases = if let Some(library) = ctx
        .component_contexts()
        .iter()
        .find(|component_ctx| match component_ctx.component().language {
            Language::Python => python::publishes_library(component_ctx),
            Language::Rust => rust::publishes_library(component_ctx),
            Language::Cpp | Language::TypeScript => false,
        })
        .map(ComponentContext::component)
    {
        format!(
            "the {} component at {} is a published library",
            library.language,
            library.display_path()
        )
    } else if let Some(tag) = ctx.release_tags().first() {
        format!("it has release tags, such as {tag}")
    } else {
        // Recorded as passing, so that a waiver for it is reported as unneeded rather than as naming no check.
        report.must(
            "changelog-exists",
            "CHANGELOG.md (not needed: no published library or release tag)",
            Outcome::Pass,
        );
        return;
    };
    report.must(
        "changelog-exists",
        "CHANGELOG.md",
        Outcome::fail(format!(
            "Not found: CHANGELOG.md, which the project needs because {releases}"
        )),
    );
}

//
// Licensing
//

/// Check the license files: the REUSE texts in `LICENSES/`, their copies at the root, and `REUSE.toml`. Whether every
/// file carries SPDX headers or a `REUSE.toml` annotation is left to `reuse lint`.
pub(crate) fn check_licenses(ctx: &ProjectContext, report: &mut Report) {
    let files = ctx.license_files();

    report.must("license-exists", "LICENSE", files.has_copy());

    report.must(
        "licenses-dir-exists",
        "LICENSES/: holds the license texts",
        if files.texts.is_empty() {
            Outcome::fail("No license texts in LICENSES/")
        } else {
            Outcome::Pass
        },
    );

    if !files.texts.is_empty() || files.has_copy() {
        let problems = files.copy_problems();

        // A text without a copy at all is a missing file; a copy that's misnamed or differs only warns.
        let missing = files.copies.len() < files.texts.len();
        report.check(
            "license-copies",
            "LICENSE: root copies match LICENSES/",
            if problems.is_empty() {
                Ok(())
            } else if missing {
                Err(Issue::fail(problems.join("\n")))
            } else {
                Err(Issue::warn(problems.join("\n")))
            },
        );
    }

    let Some(config) = report.require_parsed("reuse-toml", "REUSE.toml", ctx.reuse_toml()) else {
        return;
    };

    report.must(
        "reuse-toml-version",
        "REUSE.toml: version 1",
        config.check_version(),
    );
}

//
// Git configuration
//

pub(crate) fn check_gitignore(ctx: &ProjectContext, report: &mut Report) {
    report.must(
        "gitignore-exists",
        ".gitignore",
        file_exists(&ctx.path().join(".gitignore")),
    );
}

pub(crate) fn check_gitattributes(ctx: &ProjectContext, report: &mut Report) {
    let status = ctx.gitattributes();
    let Some(content) = report.require_parsed("gitattributes", ".gitattributes", status) else {
        return;
    };

    report.should(
        "gitattributes-text-auto",
        ".gitattributes: text=auto",
        every_file_gets(content, ctx.path(), "text", "auto"),
    );

    report.should(
        "gitattributes-eol-lf",
        ".gitattributes: eol=lf",
        every_file_gets(content, ctx.path(), "eol", "lf"),
    );

    report.should(
        "gitattributes-binary",
        ".gitattributes: binary markers",
        matches_regex(content, r"(?m)^\*\.[a-zA-Z0-9]+\s+binary"),
    );

    for (language, id, driver, extensions) in DIFF_DRIVERS {
        if !ctx.languages().contains(&language) {
            continue;
        }
        report.should(
            id,
            format!(".gitattributes: {language} files use diff={driver}"),
            diff_driver_set(content, ctx.path(), driver, extensions),
        );
    }
}

/// Check that the root `.gitattributes`, whose text is `content`, sets the attribute `name` to `value` on every file,
/// as the line `* name=value` does. It is tested on a file without an extension at the root and on one in a
/// subdirectory, so that a pattern matching only the root's files, such as `/*`, doesn't count.
fn every_file_gets(content: &str, root: &Path, name: &str, value: &str) -> Result<(), String> {
    let files = [
        root.join("file"),
        root.join("src").join("module").join("file"),
    ];
    if files
        .iter()
        .all(|file| attribute(content, root, file, name) == AttributeState::Value(value))
    {
        Ok(())
    } else {
        Err(format!(
            "{name}={value} isn't set on every file, as * {name}={value} sets it"
        ))
    }
}

/// The languages with a diff driver built into git, each with its check's ID, the driver, and the extensions of the
/// files it applies to.
const DIFF_DRIVERS: [(Language, &str, &str, &[&str]); 2] = [
    (
        Language::Python,
        "gitattributes-diff-python",
        "python",
        &["py", "pyi"],
    ),
    (Language::Rust, "gitattributes-diff-rust", "rust", &["rs"]),
];

/// Check that the root `.gitattributes`, whose text is `content`, sets `diff=<driver>` on files with each of
/// `extensions`, as the line `*.<extension> diff=<driver>` does. It is tested on a file of that extension at the root
/// and on one in a subdirectory, which a pattern such as `*.py` matches wherever the file is.
fn diff_driver_set(
    content: &str,
    root: &Path,
    driver: &str,
    extensions: &[&str],
) -> Result<(), String> {
    let missing: Vec<String> = extensions
        .iter()
        .filter(|extension| {
            let file = format!("example.{extension}");
            [
                root.join(&file),
                root.join("src").join("module").join(&file),
            ]
            .iter()
            .any(|path| attribute(content, root, path, "diff") != AttributeState::Value(driver))
        })
        .map(|extension| format!("*.{extension}"))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("diff={driver} isn't set on {}", missing.join(", ")))
    }
}

//
// Editor configuration
//

/// The properties the standard's `[*]` section sets, with the IDs of their checks.
const EDITORCONFIG_SETTINGS: [(&str, &str, &str); 6] = [
    ("editorconfig-charset", "charset", "utf-8"),
    ("editorconfig-end-of-line", "end_of_line", "lf"),
    ("editorconfig-indent-style", "indent_style", "space"),
    ("editorconfig-indent-size", "indent_size", "4"),
    (
        "editorconfig-insert-final-newline",
        "insert_final_newline",
        "true",
    ),
    (
        "editorconfig-trim-trailing-whitespace",
        "trim_trailing_whitespace",
        "true",
    ),
];

/// Files the standard indents by two spaces, as a file of each such extension: YAML, JSON and TOML.
const TWO_SPACE_FILES: [&str; 4] = ["file.yml", "file.yaml", "file.json", "file.toml"];

/// The file without an extension that stands for every file, which the `[*]` section covers.
const ANY_FILE: &str = "file";

/// Check `.editorconfig` by the properties it gives files, as editors resolve its sections: [`ANY_FILE`] for every file,
/// [`TWO_SPACE_FILES`] for the files the standard indents by two spaces, and, in a project with TypeScript, Prettier's
/// `.prettierrc`, which has no extension. Each gets the `[*]` section's settings, with an `indent_size` of 2 rather
/// than 4 for those indented by two spaces. Other sections may set more, or change these for other files.
pub(crate) fn check_editorconfig(ctx: &ProjectContext, report: &mut Report) {
    let status = ctx.editorconfig();
    let Some(config) = report.require_parsed("editorconfig", ".editorconfig", status) else {
        return;
    };

    let mut two_space = TWO_SPACE_FILES.to_vec();
    if ctx.languages().contains(&Language::TypeScript) {
        two_space.push(".prettierrc");
    }
    let every: Vec<&str> = std::iter::once(ANY_FILE)
        .chain(two_space.iter().copied())
        .collect();

    report.should(
        "editorconfig-root",
        ".editorconfig: root=true",
        config.is_root(),
    );
    for (id, key, expected) in EDITORCONFIG_SETTINGS {
        let paths = if key == "indent_size" {
            &[ANY_FILE][..]
        } else {
            &every
        };
        report.should(
            id,
            format!(".editorconfig: {key}={expected}"),
            editorconfig_values_are(config, paths, key, expected),
        );
    }
    report.should(
        "editorconfig-indent-size-data",
        ".editorconfig: indent_size=2 for YAML, JSON and TOML",
        editorconfig_values_are(config, &two_space, "indent_size", "2"),
    );
}

/// Whether `config` gives each file at `paths` the value `expected` for `key`: if not, every one that it doesn't.
fn editorconfig_values_are(
    config: &EditorConfig,
    paths: &[&str],
    key: &str,
    expected: &str,
) -> Result<(), String> {
    let issues: Vec<String> = paths
        .iter()
        .filter_map(|path| {
            let subject = if *path == ANY_FILE {
                "every file".to_owned()
            } else {
                path.strip_prefix("file.")
                    .map_or_else(|| (*path).to_owned(), |extension| format!("*.{extension}"))
            };
            match config.value(path, key) {
                Some(value) if value == expected => None,
                Some(value) => Some(format!("{key} is {value} for {subject}, not {expected}")),
                None => Some(format!("{key} isn't set for {subject}")),
            }
        })
        .collect();
    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues.join("; "))
    }
}

//
// Development environment
//

/// Check `devbox.json`, which is optional: only projects that run local services use Devbox.
pub(crate) fn check_devbox(ctx: &ProjectContext, report: &mut Report) {
    let status = ctx.devbox();

    if matches!(status, ConfigStatus::NotFound) {
        return;
    }

    let Some(config) = report.require_parsed("devbox", "devbox.json", status) else {
        return;
    };

    report.should(
        "devbox-schema",
        "devbox.json: has $schema",
        config.schema.is_some(),
    );
    report.should(
        "devbox-packages",
        "devbox.json: has packages",
        !config.packages.is_empty(),
    );

    report.should(
        "devbox-no-toolchains",
        "devbox.json: installs no language toolchains",
        devbox_toolchains(config),
    );

    // Check for devbox.lock
    report.must(
        "devbox-lock-exists",
        "devbox.lock",
        file_exists(&ctx.path().join("devbox.lock")),
    );
}

/// A Nix package or flake name that is a toolchain, including versioned attributes such as `nodejs_22`, or its
/// version or package manager, for a language norms checks: Rust (rustup, rustc, cargo, clippy, rustfmt, and the
/// rust-overlay and fenix flakes), Python (`python*`, `pypy*`, uv, pip, pipx, Poetry, PDM, Hatch, Rye, pyenv, Conda) or
/// TypeScript (`nodejs*`, pnpm, Yarn, npm, Corepack, Bun, Deno).
static DEVBOX_TOOLCHAIN: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(concat!(
        r"^(?:rustup|rustc|cargo|clippy|rustfmt|rust-bin|rust-overlay|fenix",
        r"|python3?[0-9]*(?:-?full|-?minimal)?|pypy[0-9]*|uv|pip|pipx|poetry|pdm|hatch|rye|pyenv|conda|micromamba",
        r"|nodejs(?:-slim)?(?:_[0-9]+|_latest|-[0-9]+_x)?|pnpm(?:_[0-9]+|_latest)?|yarn(?:-berry(?:_[0-9]+)?)?|npm",
        r"|corepack(?:_[0-9]+|_latest)?|bun|deno)$",
    ))
    .unwrap()
});

/// The names under which a Devbox package reference may name a toolchain, lowercased so that mixed-case attributes
/// such as `python313Full` match: the first and last parts of its attribute path (`rust-bin.stable.latest.default`,
/// `python312Packages.pip`), taken after a flake reference's `#` and without any `@version`, plus the repository of a
/// `github:` flake (`github:nix-community/fenix#stable.toolchain`, `github:astral-sh/uv`) or a `runx:` package
/// (`runx:astral-sh/uv`).
fn devbox_package_names(package: &str) -> Vec<String> {
    let mut names = Vec::new();

    if let Some(repo) = package.strip_prefix("runx:") {
        let repo = repo.split_once('@').map_or(repo, |(repo, _)| repo);
        names.extend(repo.rsplit('/').next());
    } else {
        // A flake reference without an attribute (`github:astral-sh/uv`) installs the flake's default package.
        let (flake, attr) = match package.split_once('#') {
            Some((flake, attr)) => (Some(flake), Some(attr)),
            None if package.contains(':') => (Some(package), None),
            None => (None, Some(package)),
        };
        if let Some(path) = flake.and_then(|flake| flake.strip_prefix("github:")) {
            names.extend(path.split('/').nth(1));
        }
        if let Some(attr) = attr {
            let attr = attr.split_once('@').map_or(attr, |(attr, _)| attr);
            names.extend(attr.split('.').next());
            names.extend(attr.rsplit('.').next());
        }
    }

    names.iter().map(|name| name.to_ascii_lowercase()).collect()
}

/// Whether `devbox.json` installs no language toolchains, as packages or by enabling Corepack, which Devbox's
/// Node.js plugin does when `DEVBOX_COREPACK_ENABLED` is set to anything.
fn devbox_toolchains(config: &DevboxConfig) -> Result<(), String> {
    let mut problems = Vec::new();

    let toolchains: Vec<&str> = config
        .packages
        .iter()
        .filter(|package| {
            devbox_package_names(package)
                .iter()
                .any(|name| DEVBOX_TOOLCHAIN.is_match(name))
        })
        .map(String::as_str)
        .collect();
    if !toolchains.is_empty() {
        problems.push(format!("installs {}", toolchains.join(", ")));
    }

    if config
        .env
        .as_ref()
        .and_then(|env| env.get("DEVBOX_COREPACK_ENABLED"))
        .is_some_and(|value| !value.is_null() && value.as_str() != Some(""))
    {
        problems.push("enables Corepack (DEVBOX_COREPACK_ENABLED)".to_owned());
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "devbox.json {}; install toolchains and their managers outside Devbox",
            problems.join(" and ")
        ))
    }
}

//
// Markdown
//

/// Check `.markdownlint-cli2.jsonc`, the markdownlint configuration that the hook and the CI action share.
pub(crate) fn check_markdownlint(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = report.require_parsed(
        "markdownlint-cli2",
        MARKDOWNLINT_CLI2_CONFIG,
        ctx.markdownlint_cli2(),
    ) else {
        return;
    };

    let unread = other_markdownlint_configs(ctx, config);
    report.should(
        "markdownlint-cli2-rules",
        format!("{MARKDOWNLINT_CLI2_CONFIG}: every default rule enabled"),
        unread.clone().and_then(|()| config.check_rules()),
    );
    report.should(
        "markdownlint-cli2-settings",
        format!("{MARKDOWNLINT_CLI2_CONFIG}: dash lists, 80 columns, repeated headings under different parents"),
        unread.and_then(|()| config.check_settings()),
    );
}

/// Whether `.markdownlint-cli2.jsonc` is the project's only markdownlint configuration. markdownlint-cli2 would use
/// any other: a `.markdownlint.*` file beside it in place of its `config`, and the files in a subdirectory for the
/// files there. Those in directories whose Markdown files its `ignores` patterns match apply to no file it lints, so
/// they don't count.
fn other_markdownlint_configs(
    ctx: &ProjectContext,
    config: &MarkdownlintCli2Config,
) -> Result<(), String> {
    let mut ignores = GlobSetBuilder::new();
    for pattern in config.ignores() {
        if let Ok(glob) = GlobBuilder::new(pattern.trim_start_matches("./"))
            .literal_separator(true)
            .build()
        {
            ignores.add(glob);
        }
    }
    let ignores = ignores.build().unwrap_or_else(|_| GlobSet::empty());
    let others: Vec<_> = ctx
        .markdownlint_configs()
        .iter()
        .filter(|path| path.as_os_str() != MARKDOWNLINT_CLI2_CONFIG)
        // A config applies to the Markdown files of its directory, whether or not `ignores` matches the config itself, so
        // it counts unless a Markdown file beside it would be ignored.
        .filter(|path| !ignores.is_match(path.with_file_name("file.md")))
        .map(|path| {
            path.iter()
                .map(|part| part.to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        })
        .collect();
    if others.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Other markdownlint configuration that norms doesn't read: {}",
            others.join(", ")
        ))
    }
}

//
// Dependency management
//

pub(crate) fn check_renovate(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = report.require_parsed("renovate", "renovate.json", ctx.renovate()) else {
        return;
    };

    report.should(
        "renovate-schema",
        "renovate.json: has $schema",
        config.schema.is_some(),
    );
    report.should(
        "renovate-extends-preset",
        "renovate.json: extends the shared preset",
        if config.extends_shared_preset() {
            Ok(())
        } else {
            Err(format!("Doesn't extend {SHARED_PRESET}"))
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Status;

    #[test]
    fn claude_md_imports_agents_md() {
        for content in [
            "@AGENTS.md\n",
            "# CLAUDE.md\n\n@AGENTS.md  \n\nUse Claude's tools.\n",
            "```sh\ncargo test\n```\n\n@AGENTS.md\r\n",
        ] {
            assert!(imports_agents(content), "{content}");
        }
        for content in [
            "",
            "See AGENTS.md.\n",
            "See @AGENTS.md for the commands.\n",
            "Import it with `@AGENTS.md`.\n",
            "<!-- @AGENTS.md -->\n",
            " @AGENTS.md\n",
            "\t@AGENTS.md\n",
            "@./AGENTS.md\n",
            "@AGENTS.md.bak\n",
            "@docs/AGENTS.md\n",
            "To share it, add:\n\n    @AGENTS.md\n",
            "To share it, add:\n\n```markdown\n@AGENTS.md\n```\n",
            "~~~\n@AGENTS.md\n~~~\n",
        ] {
            assert!(!imports_agents(content), "{content}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn claude_md_linked_to_agents_md_warns() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::TempDir::new().unwrap();
        let agents = dir.path().join("AGENTS.md");
        let claude = dir.path().join("CLAUDE.md");
        let other = dir.path().join("OTHER.md");
        // AGENTS.md imports itself, so that the import alone would pass and only the symlink warns.
        std::fs::write(&agents, "# AGENTS.md\n\n@AGENTS.md\n").unwrap();
        std::fs::write(&other, "# AGENTS.md\n").unwrap();
        let check = || {
            let mut report = Report::new(dir.path().to_path_buf(), Vec::new());
            check_agent_guidance(&ProjectContext::new(dir.path().to_path_buf()), &mut report);
            report
                .results
                .iter()
                .map(|result| (result.status, result.message.clone()))
                .collect::<Vec<_>>()
        };

        std::fs::write(&claude, "# CLAUDE.md\n\n@AGENTS.md\n").unwrap();
        assert_eq!(check(), [(Status::Pass, String::new())]);

        std::fs::remove_file(&claude).unwrap();
        symlink("AGENTS.md", &claude).unwrap();
        assert!(same_file(&claude, &agents));
        assert!(!same_file(&other, &agents));
        assert!(!same_file(&dir.path().join("MISSING.md"), &agents));
        let linked = vec![(
            Status::Warn,
            "CLAUDE.md and AGENTS.md are the same file, through a symlink: make CLAUDE.md a file of its own that \
             has @AGENTS.md on a line of its own"
                .to_owned(),
        )];
        assert_eq!(check(), linked);

        // The other way round, AGENTS.md links to CLAUDE.md.
        std::fs::remove_file(&claude).unwrap();
        std::fs::rename(&agents, &claude).unwrap();
        symlink("CLAUDE.md", &agents).unwrap();
        assert_eq!(check(), linked);
    }

    #[test]
    fn text_and_eol_apply_to_every_file() {
        let root = Path::new("/project");
        for content in [
            "* text=auto eol=lf\n",
            "*       text=auto eol=lf\n",
            "*\ttext=auto\n*\teol=lf\n",
        ] {
            assert_eq!(
                every_file_gets(content, root, "text", "auto"),
                Ok(()),
                "{content}"
            );
            assert_eq!(
                every_file_gets(content, root, "eol", "lf"),
                Ok(()),
                "{content}"
            );
        }
        for content in [
            "",
            "*.rs text=auto eol=lf\n",
            "* text eol=crlf\n",
            "# * text=auto eol=lf\n",
            "* text=auto eol=lf\n* -text !eol\n",
            "/* text=auto eol=lf\n",
        ] {
            assert!(
                every_file_gets(content, root, "text", "auto").is_err(),
                "{content}"
            );
            assert!(
                every_file_gets(content, root, "eol", "lf").is_err(),
                "{content}"
            );
        }
        assert_eq!(
            every_file_gets("", root, "text", "auto"),
            Err("text=auto isn't set on every file, as * text=auto sets it".to_owned())
        );
    }

    #[test]
    fn diff_drivers_cover_each_extension() {
        let root = Path::new("/project");
        let python = |content: &str| diff_driver_set(content, root, "python", &["py", "pyi"]);
        assert_eq!(
            python("*.py    text diff=python\n*.pyi   text diff=python\n"),
            Ok(())
        );
        assert_eq!(python("*.py* diff=python\n"), Ok(()));
        assert_eq!(
            python("*.py diff=python\n"),
            Err("diff=python isn't set on *.pyi".to_owned())
        );
        assert_eq!(
            python("*.py diff\n*.pyi diff=rust\n"),
            Err("diff=python isn't set on *.py, *.pyi".to_owned())
        );
        assert_eq!(
            python("/src/*.py diff=python\n*.pyi diff=python\n"),
            Err("diff=python isn't set on *.py".to_owned())
        );
        assert_eq!(
            python("/*.py diff=python\n*.pyi diff=python\n"),
            Err("diff=python isn't set on *.py".to_owned())
        );
        assert_eq!(
            python("*.py* diff=python\n*.pyi binary\n"),
            Err("diff=python isn't set on *.pyi".to_owned())
        );
        assert_eq!(
            diff_driver_set("*.rs text diff=rust\n", root, "rust", &["rs"]),
            Ok(())
        );
    }

    #[test]
    fn editorconfig_values_are_resolved_through_sections() {
        let standard = EditorConfig::parse(
            "root = true\n\n[*]\nindent_size = 4\nend_of_line = lf\n\n[*.{yml,yaml,json,toml}]\nindent_size = 2\n",
        )
        .unwrap();
        assert_eq!(
            editorconfig_values_are(&standard, &[ANY_FILE], "indent_size", "4"),
            Ok(())
        );
        assert_eq!(
            editorconfig_values_are(&standard, &TWO_SPACE_FILES, "indent_size", "2"),
            Ok(())
        );
        assert_eq!(
            editorconfig_values_are(&standard, &[ANY_FILE, "file.json"], "end_of_line", "lf"),
            Ok(())
        );

        // A setting in another section than [*] doesn't reach every file, and one the 2-space section lacks is
        // inherited from [*]. Every file that differs is named.
        let other = EditorConfig::parse(
            "[*.py]\nindent_size = 4\n\n[*.json]\nindent_size = 2\nend_of_line = crlf\n",
        )
        .unwrap();
        assert_eq!(
            editorconfig_values_are(&other, &[ANY_FILE], "indent_size", "4"),
            Err("indent_size isn't set for every file".to_owned())
        );
        assert_eq!(
            editorconfig_values_are(
                &other,
                &["file.json", "file.toml", ".prettierrc"],
                "end_of_line",
                "lf"
            ),
            Err(
                "end_of_line is crlf for *.json, not lf; end_of_line isn't set for *.toml; \
                 end_of_line isn't set for .prettierrc"
                    .to_owned()
            )
        );
    }

    #[test]
    fn editorconfig_checks_report_what_files_get() {
        // The editorconfig results for a project with the given .editorconfig and files.
        let results = |editorconfig: &str, files: &[&str]| {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join(".editorconfig"), editorconfig).unwrap();
            for file in files {
                std::fs::write(dir.path().join(file), "{}").unwrap();
            }
            let project = ProjectContext::new(dir.path().to_path_buf());
            let mut report = Report::new(dir.path().to_path_buf(), Vec::new());
            check_editorconfig(&project, &mut report);
            report
                .results
                .iter()
                .filter(|result| result.status != Status::Pass)
                .map(|result| (result.id.clone(), result.message.clone()))
                .collect::<Vec<_>>()
        };
        let standard = "root = true\n\n[*]\ncharset = utf-8\nend_of_line = lf\nindent_style = space\nindent_size = 4\n\
                        insert_final_newline = true\ntrim_trailing_whitespace = true\n\n[*.{yml,yaml,json,toml}]\nindent_size = 2\n";
        assert_eq!(results(standard, &[]), []);
        // root = true counts only in the preamble, before any section.
        assert_eq!(
            results(
                &standard.replacen("root = true\n\n[*]\n", "[*]\nroot = true\n", 1),
                &[]
            ),
            [("editorconfig-root".to_owned(), String::new())]
        );
        // A project with TypeScript indents Prettier's .prettierrc by two spaces too.
        assert_eq!(
            results(standard, &["package.json"]),
            [(
                "editorconfig-indent-size-data".to_owned(),
                "indent_size is 4 for .prettierrc, not 2".to_owned()
            )]
        );
        assert_eq!(
            results(
                &format!("{standard}\n[.prettierrc]\nindent_size = 2\n"),
                &["package.json"]
            ),
            []
        );
    }

    fn devbox(json: &str) -> Result<(), String> {
        devbox_toolchains(&json5::from_str(json).unwrap())
    }

    fn is_devbox_toolchain(package: &str) -> bool {
        devbox_package_names(package)
            .iter()
            .any(|name| DEVBOX_TOOLCHAIN.is_match(name))
    }

    #[test]
    fn devbox_package_names_parsed() {
        for (package, names) in [
            ("postgresql", vec!["postgresql", "postgresql"]),
            ("nodejs@22.11.0", vec!["nodejs", "nodejs"]),
            (
                "github:NixOS/nixpkgs/5233fd2#nodejs_22",
                vec!["nixpkgs", "nodejs_22", "nodejs_22"],
            ),
            ("nixpkgs#uv@latest", vec!["uv", "uv"]),
            ("python312Packages.pip", vec!["python312packages", "pip"]),
            ("runx:astral-sh/uv@0.9.0", vec!["uv"]),
            ("runx:astral-sh/uv", vec!["uv"]),
            ("python3Full", vec!["python3full", "python3full"]),
            ("github:astral-sh/uv", vec!["uv"]),
            ("github:NixOS/nixpkgs/nixos-24.05", vec!["nixpkgs"]),
            ("path:./flakes/db", vec![]),
            (
                "github:oxalica/rust-overlay#rust-bin.stable.latest.default",
                vec!["rust-overlay", "rust-bin", "default"],
            ),
        ] {
            assert_eq!(devbox_package_names(package), names, "{package}");
        }
    }

    #[test]
    fn devbox_toolchain_packages() {
        for package in [
            "rustup",
            "rustc@1.91",
            "cargo",
            "clippy",
            "rustfmt",
            "github:oxalica/rust-overlay#rust-bin.stable.latest.default",
            "github:nix-community/fenix#stable.toolchain",
            "python3",
            "python312",
            "python313Full",
            "python3Minimal",
            "pypy3",
            "uv@0.9",
            "pip",
            "pipx",
            "poetry",
            "pdm",
            "hatch",
            "rye",
            "pyenv",
            "conda",
            "micromamba",
            "nodejs",
            "nodejs@24",
            "nodejs_22",
            "nodejs_latest",
            "nodejs-slim",
            "nodejs-slim_24",
            "nodejs-slim_latest",
            "nodejs-18_x",
            "pnpm@12",
            "pnpm_10",
            "pnpm_latest",
            "yarn",
            "yarn-berry",
            "yarn-berry_4",
            "npm",
            "corepack",
            "corepack_22",
            "corepack_latest",
            "bun",
            "deno",
            "nixpkgs#pnpm",
            "python312Packages.poetry",
            "github:astral-sh/uv",
            "runx:astral-sh/uv",
        ] {
            assert!(is_devbox_toolchain(package), "{package}");
        }
        for package in [
            "postgresql@17",
            "postgresql_16",
            "sqlx-cli@0.8",
            "cargo-deny",
            "cargo-watch",
            "pre-commit",
            "redis",
            "python312Packages.psycopg",
            "nodePackages.prettier",
            "rust-analyzer",
            "uvicorn",
            "deno-lint",
            "github:NixOS/nixpkgs",
            "github:NixOS/nixpkgs/nixos-24.05#postgresql",
            "path:./flakes/db#db",
        ] {
            assert!(!is_devbox_toolchain(package), "{package}");
        }
    }

    #[test]
    fn devbox_toolchains_reported() {
        assert_eq!(
            devbox(r#"{"packages": ["postgresql@17", "sqlx-cli@0.8"]}"#),
            Ok(())
        );
        assert_eq!(
            devbox(r#"{"packages": {"redis": "latest"}, "env": {"DEVBOX_COREPACK_ENABLED": ""}}"#),
            Ok(())
        );
        assert_eq!(devbox(r#"{"packages": ["redis"], "env": null}"#), Ok(()));
        assert_eq!(
            devbox(
                r#"{"packages": ["redis"], "env": {"PORT": 6379, "DEVBOX_COREPACK_ENABLED": null}}"#
            ),
            Ok(())
        );
        assert_eq!(
            devbox(r#"{"packages": ["postgresql@17", "nodejs@24", "pnpm@11.22.0"]}"#),
            Err("devbox.json installs nodejs@24, pnpm@11.22.0; install toolchains and their managers outside Devbox".to_owned())
        );
        assert_eq!(
            devbox(r#"{"packages": {"uv": {"version": "latest"}}, "env": {"DEVBOX_COREPACK_ENABLED": "true"}}"#),
            Err(
                "devbox.json installs uv and enables Corepack (DEVBOX_COREPACK_ENABLED); install toolchains and their managers outside Devbox"
                    .to_owned()
            )
        );
    }
}
