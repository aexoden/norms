// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Common checks that apply to all projects reqgardless of language.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use regex::Regex;
use serde_json::Value;

use crate::checks::{python, rust};
use crate::config::{
    AttributeState, DevboxConfig, GATE_JOB, MarkdownlintCli2Config, PrecommitConfig, PrecommitHook,
    SHARED_PRESET, Workflow, WorkflowJob, WorkflowStep, attribute, commands,
};
use crate::context::{
    AUDIT_WORKFLOW, CI_WORKFLOW, ComponentContext, ConfigStatus, MARKDOWNLINT_CLI2_CONFIG,
    ProjectContext,
};
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

/// An import of `AGENTS.md` in `CLAUDE.md`: `@AGENTS.md` (or `@./AGENTS.md`) as a word of its own. In a code span, as
/// in `` `@AGENTS.md` ``, it isn't an import.
static AGENTS_IMPORT: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r"(?:^|\s)@(?:\./)?AGENTS\.md(?:\s|$)").unwrap()
});

/// Whether `content`, the text of a `CLAUDE.md`, imports `AGENTS.md`: a line outside fenced code blocks, which Claude
/// Code doesn't read imports from, has [`AGENTS_IMPORT`]. Indented code blocks, code spans with other text inside them
/// and HTML comments, which Claude Code also skips, aren't recognized.
fn imports_agents(content: &str) -> bool {
    let mut fenced = false;
    content.lines().any(|line| {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            return false;
        }
        !fenced && AGENTS_IMPORT.is_match(line)
    })
}

/// Whether `a` and `b` are the same file, as a symlink and its target are.
fn same_file(a: &Path, b: &Path) -> bool {
    matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

/// Check that the root `CLAUDE.md` beside an `AGENTS.md` imports it rather than repeating it, or is a symlink to it,
/// which Claude Code also accepts. A `.claude/CLAUDE.md` isn't read.
pub(crate) fn check_agent_guidance(ctx: &ProjectContext, report: &mut Report) {
    let agents = ctx.path().join("AGENTS.md");
    if !agents.is_file() {
        return;
    }
    let outcome = match ctx.claude_md() {
        ConfigStatus::NotFound => return,
        ConfigStatus::ParseError { error, .. } => Err(error.clone()),
        ConfigStatus::Ok(_) if same_file(&ctx.path().join("CLAUDE.md"), &agents) => Ok(()),
        ConfigStatus::Ok(content) if imports_agents(content) => Ok(()),
        ConfigStatus::Ok(_) => Err(
            "AGENTS.md isn't imported: add @AGENTS.md on a line of its own, outside code"
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

        report.must(
            "license-copies",
            "LICENSE: root copies match LICENSES/",
            if problems.is_empty() {
                Ok(())
            } else {
                Err(problems.join("\n"))
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
        matches_regex(content, r"(?m)^\*\s.+text=auto"),
    );

    report.should(
        "gitattributes-eol-lf",
        ".gitattributes: eol=lf",
        matches_regex(content, r"(?m)^\*\s.+eol=lf"),
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

pub(crate) fn check_editorconfig(ctx: &ProjectContext, report: &mut Report) {
    let status = ctx.editorconfig();
    let Some(content) = report.require_parsed("editorconfig", ".editorconfig", status) else {
        return;
    };

    let settings = [
        ("editorconfig-root", "root=true", "root = true"),
        ("editorconfig-charset", "charset=utf-8", "charset = utf-8"),
        (
            "editorconfig-end-of-line",
            "end_of_line=lf",
            "end_of_line = lf",
        ),
        (
            "editorconfig-indent-style",
            "indent_style=space",
            "indent_style = space",
        ),
        (
            "editorconfig-indent-size",
            "indent_size=4",
            "indent_size = 4",
        ),
        (
            "editorconfig-insert-final-newline",
            "insert_final_newline=true",
            "insert_final_newline = true",
        ),
        (
            "editorconfig-trim-trailing-whitespace",
            "trim_trailing_whitespace=true",
            "trim_trailing_whitespace = true",
        ),
    ];

    for (id, label, needle) in settings {
        report.should(
            id,
            format!(".editorconfig: {label}"),
            matches_regex(content, &regex::escape(needle)),
        );
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

/// The globs that lint every Markdown file in the project, rather than only those at its root.
const MARKDOWN_GLOBS: [&str; 2] = ["**/*.md", "**/*.{md,markdown}"];

/// Whether `workflow` runs markdownlint-cli2 on every Markdown file: a `DavidAnson/markdownlint-cli2-action` step whose
/// `globs` include `**/*.md`, since the action otherwise lints only the files at the root, or a script running
/// `markdownlint-cli2` with that glob, quoted so that the shell leaves it to markdownlint-cli2.
fn markdownlint_in_ci(workflow: &Workflow) -> Result<(), Issue> {
    let mut runs = false;

    for step in workflow.steps_using("DavidAnson/markdownlint-cli2-action") {
        runs = true;
        let separator = step.input("separator");
        let separator = separator.as_deref().unwrap_or("\n");
        if step.input("globs").is_some_and(|globs| {
            globs
                .split(separator)
                .any(|glob| MARKDOWN_GLOBS.contains(&glob.trim()))
        }) {
            return Ok(());
        }
    }

    let quoted = |word: &str| {
        ['"', '\''].into_iter().any(|quote| {
            word.strip_prefix(quote)
                .and_then(|word| word.strip_suffix(quote))
                .is_some_and(|glob| MARKDOWN_GLOBS.contains(&glob))
        })
    };
    for args in workflow.filter_map_commands(|words| {
        let start = words.iter().position(|word| {
            let name = word.rsplit('/').next().unwrap_or(word);
            name == "markdownlint-cli2" || name.starts_with("markdownlint-cli2@")
        })?;
        Some(
            words
                .get(start + 1..)
                .unwrap_or_default()
                .iter()
                .any(|word| quoted(word)),
        )
    }) {
        runs = true;
        if args {
            return Ok(());
        }
    }

    if runs {
        Err(Issue::warn(
            "markdownlint-cli2 doesn't lint '**/*.md', so it checks only the files at the root",
        ))
    } else {
        Err(Issue::fail(
            "No step uses DavidAnson/markdownlint-cli2-action or runs markdownlint-cli2",
        ))
    }
}

//
// Pre-commit
//

/// The hooks from pre-commit-hooks that every project runs.
const PRE_COMMIT_HOOKS: [&str; 9] = [
    "check-added-large-files",
    "check-case-conflict",
    "check-symlinks",
    "check-toml",
    "check-vcs-permalinks",
    "check-yaml",
    "detect-private-key",
    "end-of-file-fixer",
    "trailing-whitespace",
];

/// Whether each hook of the pre-commit-hooks set runs before each commit.
fn pre_commit_hooks_in(config: &PrecommitConfig) -> Result<(), Issue> {
    let mut missing = Vec::new();
    let mut issues = Vec::new();
    for id in PRE_COMMIT_HOOKS {
        if !config
            .repos
            .iter()
            .flat_map(|repo| &repo.hooks)
            .any(|hook| hook.id == id)
        {
            missing.push(id);
        } else if let Err(issue) = config.check_commit_hook(id) {
            issues.push(issue);
        }
    }
    if !missing.is_empty() {
        issues.insert(0, Issue::fail(format!("Missing: {}", missing.join(", "))));
    }
    Issue::combine(issues)
}

pub(crate) fn check_precommit(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = report.require_parsed(
        "precommit-config",
        ".pre-commit-config.yaml",
        ctx.precommit(),
    ) else {
        return;
    };

    report.should(
        "precommit-install-hook-types",
        "Pre-commit: default_install_hook_types includes pre-commit and commit-msg",
        config.check_default_install_hook_types(),
    );
    report.should(
        "precommit-default-stages",
        "Pre-commit: default_stages is [pre-commit]",
        config.check_default_stages(),
    );
    report.should(
        "precommit-hook-ids-unique",
        "Pre-commit: each hook has a unique ID or alias",
        config.check_unique_hook_ids(),
    );
    report.check(
        "precommit-pre-commit-hooks",
        "Pre-commit: the common hooks from pre-commit-hooks",
        pre_commit_hooks_in(config),
    );
    report.check(
        "precommit-reuse",
        "Pre-commit: reuse hook",
        config.check_commit_hook("reuse"),
    );
    report.check(
        "precommit-markdownlint",
        "Pre-commit: markdownlint-cli2 hook",
        config.check_commit_hook("markdownlint-cli2"),
    );
    report.check(
        "precommit-tombi",
        "Pre-commit: tombi-format hook",
        config.check_commit_hook("tombi-format"),
    );
    report.check(
        "precommit-conventional-pre-commit",
        "Pre-commit: conventional-pre-commit hook (commit-msg stage, recognized types)",
        conventional_hook_in(config),
    );
    report.check(
        "precommit-zizmor",
        "Pre-commit: zizmor hook (pedantic)",
        config.commit_hooks("zizmor").and_then(|hooks| {
            if hooks.into_iter().any(zizmor_hook_strict) {
                Ok(())
            } else {
                Err(Issue::warn("The zizmor hook doesn't pass --pedantic"))
            }
        }),
    );
}

//
// Commit messages
//

/// The commit types STANDARDS.md recognizes.
const COMMIT_TYPES: [&str; 11] = [
    "feat", "fix", "perf", "refactor", "style", "test", "docs", "build", "ci", "revert", "chore",
];

/// The repository of conventional-pre-commit, whose hook sets the `commit-msg` stage itself.
const CONVENTIONAL_PRE_COMMIT_REPO: &str = "github.com/compilerla/conventional-pre-commit";

/// The types conventional-pre-commit always allows, adding them to any list of types that names neither.
const CONVENTIONAL_TYPES: [&str; 2] = ["feat", "fix"];

/// The commit types a conventional-pre-commit hook allows, from the words of its command after the program: the
/// positional arguments. Without any, it allows its own defaults, which the standard doesn't rely on, since a new
/// release may change them. Of its options, only `--scopes` takes a value, and argparse accepts it abbreviated to any
/// prefix down to `--sc`.
///
/// pre-commit appends the commit message file, which argparse takes from the first run of positional arguments, so
/// types followed by an option leave the arguments after it unrecognized, and the hook fails on every commit.
fn conventional_commit_types<'a>(args: &[&'a str]) -> Result<BTreeSet<&'a str>, Issue> {
    let mut types = BTreeSet::new();
    let mut args = args.iter().copied();
    let mut options = true;
    while let Some(arg) = args.next() {
        if options && arg.starts_with('-') {
            if !types.is_empty() {
                return Err(Issue::fail(format!(
                    "passes commit types before {arg}, so it fails on every commit"
                )));
            }
            if arg == "--" {
                options = false;
            } else if arg.len() >= "--sc".len() && "--scopes".starts_with(arg) {
                args.next();
            }
        } else {
            types.insert(arg);
        }
    }
    if types.is_empty() {
        return Err(Issue::warn(
            "passes no commit types, so it allows the hook's defaults, which a release may change",
        ));
    } else if CONVENTIONAL_TYPES
        .iter()
        .all(|commit_type| !types.contains(commit_type))
    {
        types.extend(CONVENTIONAL_TYPES);
    }
    Ok(types)
}

/// The module that `python -m` runs conventional-pre-commit as.
const CONVENTIONAL_PRE_COMMIT_MODULE: &str = "conventional_pre_commit.hook";

/// The arguments a command passes to conventional-pre-commit, if it runs it: as the module `python -m` runs, or as the
/// program, by name or path.
fn conventional_pre_commit_args<'a, 'b>(words: &'b [&'a str]) -> Option<&'b [&'a str]> {
    // The package's name may come first as the value of an option, as in `uvx --from conventional-pre-commit
    // conventional-pre-commit` or `uv run --with conventional-pre-commit python -m ...`, so the module, or the last
    // word naming the program, is the one run.
    if let Some(start) = words
        .windows(2)
        .position(|pair| pair == ["-m", CONVENTIONAL_PRE_COMMIT_MODULE])
    {
        return words.get(start + 2..);
    }
    let start = words
        .iter()
        .rposition(|word| word.rsplit('/').next() == Some("conventional-pre-commit"))?;
    words.get(start + 1..)
}

/// What is wrong with the commit types that conventional-pre-commit allows given the words after the program: nothing
/// if they are exactly the recognized ones.
fn conventional_types_issues(args: &[&str]) -> Vec<Issue> {
    let types = match conventional_commit_types(args) {
        Err(issue) => return vec![issue],
        Ok(types) => types,
    };
    let missing: Vec<_> = COMMIT_TYPES
        .into_iter()
        .filter(|commit_type| !types.contains(commit_type))
        .collect();
    let extra: Vec<_> = types
        .iter()
        .copied()
        .filter(|commit_type| !COMMIT_TYPES.contains(commit_type))
        .collect();
    let mut issues = Vec::new();
    if !missing.is_empty() {
        issues.push(Issue::warn(format!("doesn't allow {}", missing.join(", "))));
    }
    if !extra.is_empty() {
        issues.push(Issue::warn(format!("also allows {}", extra.join(", "))));
    }
    issues
}

/// Whether a conventional-pre-commit hook checks each commit message for the recognized types and no others. It must
/// run in the `commit-msg` stage, which the hook's own repository sets for it unless the configuration lists other
/// stages; a local hook, or one from another repository, lists it itself.
fn conventional_hook_in(config: &PrecommitConfig) -> Result<(), Issue> {
    let mut issues = Vec::new();

    for (repo, hook) in config
        .repos
        .iter()
        .flat_map(|repo| repo.hooks.iter().map(move |hook| (repo, hook)))
        .filter(|(_, hook)| hook.id == "conventional-pre-commit")
    {
        let mut hook_issues = Vec::new();

        let commit_msg = hook.stages.as_ref().map_or_else(
            || repo.repo.contains(CONVENTIONAL_PRE_COMMIT_REPO),
            |stages| stages.iter().any(|stage| stage == "commit-msg"),
        );
        // In any other stage, it never sees a commit message: pre-commit passes it the staged files instead, or, in
        // the manual stage, doesn't run it at all.
        if !commit_msg {
            hook_issues.push(Issue::fail("doesn't run in the commit-msg stage"));
        }

        let runs = hook.filter_map_commands("conventional-pre-commit", |words| {
            Some(conventional_types_issues(conventional_pre_commit_args(
                words,
            )?))
        });
        if runs.is_empty() {
            hook_issues.push(Issue::fail("doesn't run conventional-pre-commit"));
        } else if !runs.iter().any(Vec::is_empty) {
            hook_issues.extend(runs.into_iter().flatten());
        }

        let Some(severity) = hook_issues.iter().map(|issue| issue.severity).max() else {
            return Ok(());
        };
        let messages: Vec<_> = hook_issues.into_iter().map(|issue| issue.message).collect();
        issues.push(Issue {
            severity,
            message: format!("conventional-pre-commit {}", messages.join(" and ")),
        });
    }

    if issues.is_empty() {
        Err(Issue::fail("No conventional-pre-commit hook"))
    } else {
        issues.dedup();
        Issue::alternatives(issues)
    }
}

//
// pre-commit in CI
//

/// A word of a shell command, with any quotes around it removed, that pins pre-commit to a version: as uvx takes it
/// (`pre-commit@x.y.z`), or as pip and `uvx --from` do (`pre-commit==x.y.z`, also after `--from=`). A version from a
/// shell variable counts, since its value can't be seen here.
static PRE_COMMIT_PIN: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r"(?:^|[=/])pre-commit(?:@|==)(?:[0-9]+(?:\.[0-9]+)*(?:(?:a|b|rc|\.post|\.dev)[0-9]+)?$|\$)").unwrap()
});

/// Whether a word of a shell command pins pre-commit to a version.
fn is_pre_commit_pin(word: &str) -> bool {
    PRE_COMMIT_PIN.is_match(word.trim_matches(['"', '\'']))
}

/// Whether a word of a shell command, with any quotes around it removed, invokes pre-commit: `pre-commit`, a path to
/// it, or a versioned `pre-commit@x.y.z` as uvx takes it.
fn is_pre_commit_command(word: &str) -> bool {
    let word = word.trim_matches(['"', '\'']);
    let name = word.rsplit('/').next().unwrap_or(word);
    name == "pre-commit" || name.starts_with("pre-commit@")
}

/// Whether `workflow` runs every pre-commit hook it doesn't skip: a script runs `pre-commit run` with `--all-files`
/// (or `-a`), with a pinned pre-commit. The pin belongs to that command, as `uvx pre-commit@x.y.z` or `uvx --from
/// pre-commit==x.y.z pre-commit`, or, for a plain `pre-commit run`, to an earlier command of the same job that
/// installs it, such as `pip install pre-commit==x.y.z`.
fn pre_commit_in_ci(workflow: &Workflow) -> Result<(), Issue> {
    let mut runs = false;
    let mut all_files = false;
    let mut pinned = false;

    for job in workflow.jobs.values() {
        let mut installed = false;
        for line in job.run_lines() {
            for words in commands(&line) {
                let Some(start) = words.windows(2).position(
                    |pair| matches!(pair, [program, "run"] if is_pre_commit_command(program)),
                ) else {
                    installed |= words.iter().any(|word| is_pre_commit_pin(word));
                    continue;
                };
                runs = true;
                if !words
                    .iter()
                    .skip(start + 2)
                    .any(|arg| matches!(*arg, "--all-files" | "-a"))
                {
                    continue;
                }
                all_files = true;
                pinned |= words
                    .iter()
                    .take(start + 1)
                    .any(|word| is_pre_commit_pin(word))
                    || (start == 0 && installed);
            }
        }
    }

    if !runs {
        // pre-commit/action runs the hooks too, but with whatever pre-commit is newest.
        Err(if workflow.uses_action("pre-commit/action") {
            Issue::warn(
                "No step runs pre-commit run; pre-commit/action installs pre-commit unpinned",
            )
        } else {
            Issue::fail("No step runs pre-commit run")
        })
    } else if !all_files {
        // CI has no staged files, so without --all-files every hook is skipped and the job passes.
        Err(Issue::fail(
            "pre-commit run doesn't pass --all-files, so it checks no files",
        ))
    } else if !pinned {
        Err(Issue::warn(
            "pre-commit isn't pinned to a version (pre-commit@x.y.z or pre-commit==x.y.z)",
        ))
    } else {
        Ok(())
    }
}

/// The options of `pre-commit run` that take a value, which isn't a hook ID.
const PRE_COMMIT_RUN_VALUE_OPTIONS: [&str; 20] = [
    "--checkout-type",
    "--color",
    "--commit-msg-filename",
    "--commit-object-name",
    "--config",
    "--from-ref",
    "--hook-stage",
    "--is-squash-merge",
    "--local-branch",
    "--origin",
    "--prepare-commit-message-source",
    "--remote-branch",
    "--remote-name",
    "--remote-url",
    "--rewrite-command",
    "--source",
    "--to-ref",
    "-c",
    "-o",
    "-s",
];

/// The words before pre-commit in a command, as its words, that runs `pre-commit run` for every hook of a stage,
/// however pre-commit is invoked, and that stage, `--hook-stage`'s value or else `pre-commit`, or `None` if the
/// command doesn't run it or names the one hook it runs. Option values aren't hook IDs, and the words after `--files`
/// are files.
fn pre_commit_run_all<'a, 'b>(words: &'b [&'a str]) -> Option<(&'b [&'a str], &'a str)> {
    let start = words
        .windows(2)
        .position(|pair| matches!(pair, [program, "run"] if is_pre_commit_command(program)))?;
    let mut stage = "pre-commit";
    let mut args = words.iter().skip(start + 2);
    while let Some(arg) = args.next() {
        if *arg == "--files" || arg.starts_with('#') {
            break;
        }
        if !arg.starts_with('-') {
            return None;
        }
        if let Some(value) = arg.strip_prefix("--hook-stage=") {
            stage = value;
        } else if PRE_COMMIT_RUN_VALUE_OPTIONS.contains(arg) {
            let value = args.next();
            if *arg == "--hook-stage"
                && let Some(value) = value
            {
                stage = value;
            }
        }
    }
    Some((words.get(..start)?, stage.trim_matches(['"', '\''])))
}

/// The value a word of a shell command assigns to `SKIP`, as `SKIP=<value>` does, without quotes around it.
fn skip_assignment(word: &str) -> Option<&str> {
    word.strip_prefix("SKIP=")
        .map(|value| value.trim_matches(['"', '\'']))
}

/// The hooks whose jobs in `ci.yaml` may have other IDs than theirs: the job each of them gets in the standards.
const HOOK_JOBS: [(&str, &str); 2] = [
    ("markdownlint-cli2", "markdownlint"),
    ("tombi-format", "tombi"),
];

/// The hooks of `config` that run in `stage` and have a job of their own in `workflow`, other than `job_id`, as each
/// one's alias or ID, that `skip`, a comma-separated list of hook IDs and aliases as pre-commit reads `SKIP`, doesn't
/// list. A hook has a job if a job has its ID or alias as its own ID, or is the job [`HOOK_JOBS`] gives it.
fn hooks_not_skipped<'a>(
    config: &'a PrecommitConfig,
    workflow: &Workflow,
    job_id: &str,
    stage: &'a str,
    skip: &str,
) -> Vec<&'a str> {
    let skipped: BTreeSet<&str> = skip
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    let has_job = |name: &str| {
        let mapped = HOOK_JOBS
            .iter()
            .find(|(hook, _)| *hook == name)
            .map(|(_, mapped)| *mapped);
        std::iter::once(name)
            .chain(mapped)
            .any(|job| job != job_id && workflow.jobs.contains_key(job))
    };
    let mut missing: Vec<&str> = Vec::new();
    for hook in config.hooks_in(stage) {
        let names: Vec<&str> = std::iter::once(hook.id.as_str())
            .chain(hook.alias())
            .collect();
        let label = hook.alias().unwrap_or(&hook.id);
        if names.iter().any(|name| has_job(name))
            && !names.iter().any(|name| skipped.contains(name))
            && !missing.contains(&label)
        {
            missing.push(label);
        }
    }
    missing
}

/// Check that each command of `workflow` that runs `pre-commit run` for every hook of a stage at the repository's
/// root, where `config` applies, lists in its `SKIP` every hook of `config` that runs in that stage and has a job of
/// its own, or
/// `None` if no command runs it. `SKIP` is read from the command's own `SKIP=` prefix, or else an earlier command of
/// the step that sets it (`SKIP=...` or `export SKIP=...`), or else the step's, job's or workflow's `env`.
fn pre_commit_skip(config: &PrecommitConfig, workflow: &Workflow) -> Option<Result<(), String>> {
    let mut found = false;
    let mut issues = Vec::new();
    for (job_id, job) in &workflow.jobs {
        for step in job
            .steps
            .iter()
            .filter(|step| workflow.runs_at_root(job, step))
        {
            let mut shell_skip: Option<String> = None;
            for line in step.run_lines() {
                for words in commands(&line) {
                    let Some((prefix, stage)) = pre_commit_run_all(&words) else {
                        let assignment = match words.as_slice() {
                            ["export", word] | [word] => skip_assignment(word),
                            _ => None,
                        };
                        if let Some(value) = assignment {
                            shell_skip = Some(value.to_owned());
                        }
                        continue;
                    };
                    found = true;
                    let skip = prefix
                        .iter()
                        .find_map(|word| skip_assignment(word))
                        .map(str::to_owned)
                        .or_else(|| shell_skip.clone())
                        .or_else(|| workflow.env(job, step, "SKIP").map(Cow::into_owned))
                        .unwrap_or_default();
                    let missing = hooks_not_skipped(config, workflow, job_id, stage, &skip);
                    let issue = format!(
                        "SKIP in {job_id} doesn't list these hooks, which have jobs of their own: {}",
                        missing.join(", ")
                    );
                    if !missing.is_empty() && !issues.contains(&issue) {
                        issues.push(issue);
                    }
                }
            }
        }
    }
    found.then(|| {
        if issues.is_empty() {
            Ok(())
        } else {
            Err(issues.join("; "))
        }
    })
}

//
// zizmor
//

/// The zizmor personas that report at least what the pedantic one does: pedantic itself, and auditor.
const STRICT_ZIZMOR_PERSONAS: [&str; 2] = ["pedantic", "auditor"];

/// Whether zizmor's command-line arguments select a persona at least as strict as pedantic: `--pedantic` (or `-p`), or
/// `--persona` naming one, as `--persona=<name>` or `--persona <name>`.
fn zizmor_args_strict<'a>(args: impl IntoIterator<Item = &'a str>) -> bool {
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let persona = match arg {
            "--pedantic" | "-p" => return true,
            "--persona" => args.next(),
            _ => arg.strip_prefix("--persona="),
        };
        if persona.is_some_and(|persona| STRICT_ZIZMOR_PERSONAS.contains(&persona)) {
            return true;
        }
    }
    false
}

/// Whether a zizmor hook runs a persona at least as strict as pedantic, from its entry and the args pre-commit appends
/// to it. A hook without an entry, as a remote hook may leave it to the hook's repository, is taken to run `zizmor`.
fn zizmor_hook_strict(hook: &PrecommitHook) -> bool {
    hook.filter_map_commands("zizmor", |words| {
        let start = words.iter().position(|word| is_zizmor_command(word))?;
        Some(zizmor_args_strict(words.iter().skip(start + 1).copied()))
    })
    .contains(&true)
}

/// Whether a word of a shell command invokes zizmor: `zizmor`, a path to it, or a versioned `zizmor@x.y.z` as uvx
/// takes it.
fn is_zizmor_command(word: &str) -> bool {
    let name = word.rsplit('/').next().unwrap_or(word);
    name == "zizmor" || name.starts_with("zizmor@") || name.starts_with("zizmor==")
}

/// Whether `workflow` runs zizmor so that its findings fail the job, with a persona at least as strict as pedantic:
/// either a `run` script line invoking it with such a persona, or a `zizmorcore/zizmor-action` step setting one in
/// `persona` and turning off `advanced-security`, whose default uploads findings to code scanning and passes the job.
fn zizmor_in_ci(workflow: &Workflow) -> Result<(), Issue> {
    let mut issues = Vec::new();

    for line in workflow.run_lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        let Some(start) = words.iter().position(|word| is_zizmor_command(word)) else {
            continue;
        };
        if zizmor_args_strict(words.iter().skip(start + 1).copied()) {
            return Ok(());
        }
        issues.push(Issue::warn("A step runs zizmor without --pedantic"));
    }

    for step in workflow.steps_using("zizmorcore/zizmor-action") {
        let strict = step
            .input("persona")
            .is_some_and(|persona| STRICT_ZIZMOR_PERSONAS.contains(&persona.as_ref()));
        let gating = step.input("advanced-security").as_deref() == Some("false");
        match (strict, gating) {
            (true, true) => return Ok(()),
            (false, true) => {
                issues.push(Issue::warn("zizmor-action doesn't set persona: pedantic"));
            }
            // Uploading the findings to code scanning instead passes the job whatever zizmor finds.
            (true, false) => {
                issues.push(Issue::fail(
                    "zizmor-action doesn't set advanced-security: false",
                ));
            }
            (false, false) => issues.push(Issue::fail(
                "zizmor-action doesn't set persona: pedantic or advanced-security: false",
            )),
        }
    }

    if issues.is_empty() {
        Err(Issue::fail(
            "No step runs zizmor or uses zizmorcore/zizmor-action",
        ))
    } else {
        issues.dedup();
        // Each issue is a step's, and the best of them decides: one that fails the job on findings, only with too
        // lax a persona, makes the check a warning whatever the others do.
        Issue::alternatives(issues)
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

//
// Continuous integration
//

/// `reuse lint` in a CI script, but not `reuse lint-file`, which lints only the files it's given.
static REUSE_LINT: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r"(?m)\breuse\s+lint(?:$|[^\w-])").unwrap()
});

/// The action that installs uv.
pub(crate) const SETUP_UV: &str = "astral-sh/setup-uv";

/// An exact uv version, such as `0.9.5`.
static EXACT_UV_VERSION: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r"^[0-9]+\.[0-9]+\.[0-9]+$").unwrap()
});

/// Whether `version`, a setup-uv step's `version` input, is an exact uv version.
pub(crate) fn is_exact_uv_version(version: &str) -> bool {
    EXACT_UV_VERSION.is_match(version)
}

/// Whether a job runs uv itself, such as `uv sync` or `uv run`, rather than only running tools through `uvx`: some
/// command's first word is `uv` or a path to it. A later word naming uv, as in `pip install uv` or `rm -rf ~/.cache/uv`,
/// doesn't run it.
fn job_runs_uv(job: &WorkflowJob) -> bool {
    job.run_lines().any(|line| {
        commands(&line).any(|words| {
            words
                .first()
                .is_some_and(|word| word.rsplit('/').next() == Some("uv"))
        })
    })
}

/// Check that every setup-uv step in a job that runs uv itself pins an exact uv version with its `version` input, or
/// `None` if there is no such step. Without the input, setup-uv installs the newest uv that `required-version` allows,
/// so the version CI runs changes with every uv release. A job that only runs tools through `uvx`, such as the
/// pre-commit job, doesn't install the project, so its uv needn't match.
fn setup_uv_exact(workflow: &Workflow) -> Option<Result<(), String>> {
    let mut used = false;
    let mut inexact = Vec::new();
    for (job, step) in workflow.job_steps_using(SETUP_UV) {
        if !workflow.jobs.get(job).is_some_and(job_runs_uv) {
            continue;
        }
        used = true;
        match step.input("version") {
            Some(version) if is_exact_uv_version(&version) => {}
            Some(version) => inexact.push(format!("{job} ({version})")),
            None if step.input("version-file").is_some() => {
                inexact.push(format!("{job} (version-file, not version)"));
            }
            None => inexact.push(format!("{job} (no version)")),
        }
    }
    used.then(|| {
        if inexact.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "setup-uv doesn't pin an exact uv version in: {}",
                inexact.join(", ")
            ))
        }
    })
}

/// `tombi format --check` in a CI script, which fails on unformatted files instead of rewriting them.
static TOMBI_FORMAT_CHECK: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r"(?m)\btombi\s+format\b[^\n]*\s--check\b").unwrap()
});

pub(crate) fn check_github_actions(ctx: &ProjectContext, report: &mut Report) {
    let workflows_dir = ctx.path().join(".github/workflows");
    report.must(
        "ci-workflows-dir-exists",
        "CI: .github/workflows/ exists",
        file_exists(&workflows_dir),
    );

    if !workflows_dir.exists() {
        return;
    }

    report.require_parsed("ci-yaml", "CI: ci.yaml", ctx.ci_workflow());

    if let Some(audit) = report.require_parsed("audit-yaml", "CI: audit.yaml", ctx.audit_workflow())
    {
        report.must(
            "audit-yaml-schedule",
            "CI: audit.yaml: runs on a schedule",
            audit.runs_on_schedule(),
        );
    }

    // Every other workflow must parse too. The checks above already cover ci.yaml and audit.yaml.
    for (name, status) in ctx
        .workflows()
        .iter()
        .filter(|(name, _)| *name != CI_WORKFLOW && *name != AUDIT_WORKFLOW)
    {
        report.must(
            "workflow-valid",
            format!("CI: {name}: valid"),
            match status {
                ConfigStatus::NotFound => Outcome::fail("File not found"),
                ConfigStatus::ParseError { error, .. } => Outcome::fail(error.clone()),
                ConfigStatus::Ok(_) => Outcome::Pass,
            },
        );
    }

    for (name, workflow) in ctx
        .workflows()
        .iter()
        .filter_map(|(name, status)| Some((name, status.as_ref_ok()?)))
    {
        let untimed = workflow.jobs_without_timeout();
        report.should(
            "workflow-job-timeouts",
            format!("CI: {name}: every job sets timeout-minutes"),
            if untimed.is_empty() {
                Ok(())
            } else {
                Err(format!("No timeout-minutes: {}", untimed.join(", ")))
            },
        );
        report.should(
            "workflow-concurrency",
            format!("CI: {name}: concurrency cancels superseded runs"),
            workflow.cancels_superseded_runs(),
        );
        if let Some(exact) = setup_uv_exact(workflow) {
            report.should(
                "workflow-setup-uv-version",
                format!("CI: {name}: setup-uv pins an exact uv version in jobs running uv"),
                exact,
            );
        }
    }

    if let ConfigStatus::Ok(workflow) = ctx.ci_workflow() {
        check_ci_workflow(ctx, workflow, report);
    }
}

/// Check `ci.yaml`, `workflow`: the common hooks' jobs, its triggers and its gate job.
fn check_ci_workflow(ctx: &ProjectContext, workflow: &Workflow, report: &mut Report) {
    report.must(
        "ci-reuse",
        "CI: REUSE compliance",
        workflow.uses_action("fsfe/reuse-action") || workflow.runs_matching(&REUSE_LINT),
    );
    report.check(
        "ci-markdownlint",
        "CI: Markdown linting (every file)",
        markdownlint_in_ci(workflow),
    );
    report.must(
        "ci-tombi",
        "CI: TOML formatting",
        workflow.runs_matching(&TOMBI_FORMAT_CHECK),
    );
    report.check("ci-zizmor", "CI: zizmor (pedantic)", zizmor_in_ci(workflow));
    report.check(
        "ci-pre-commit",
        "CI: remaining pre-commit hooks (pinned, --all-files)",
        pre_commit_in_ci(workflow),
    );
    if let ConfigStatus::Ok(config) = ctx.precommit()
        && let Some(skip) = pre_commit_skip(config, workflow)
    {
        report.should(
            "ci-pre-commit-skip",
            "CI: the pre-commit job's SKIP lists every hook with a job of its own",
            skip,
        );
    }

    report.check(
        "ci-yaml-pull-request",
        "CI: ci.yaml: runs on every pull request",
        workflow.runs_on_every_pull_request(),
    );
    report.check(
        "ci-yaml-push",
        "CI: ci.yaml: runs on pushes to main and dev",
        workflow.runs_on_push_to(&["main", "dev"]),
    );

    check_slim_runner(workflow, report);
    check_gate(workflow, report);
}

/// The actions of the jobs that install nothing, each of which is the whole job apart from checking out the
/// repository: markdownlint's and zizmor's, and Tombi's setup action, which downloads a prebuilt binary.
const LIGHT_ACTIONS: [&str; 3] = [
    "DavidAnson/markdownlint-cli2-action",
    "tombi-toml/setup-tombi",
    "zizmorcore/zizmor-action",
];

/// The runner of the jobs that install nothing, which starts faster.
const SLIM_RUNNER: &str = "ubuntu-slim";

/// Whether `job` installs nothing, so that it belongs on [`SLIM_RUNNER`]: it is the gate job, or it uses one of
/// [`LIGHT_ACTIONS`] and otherwise only checks out the repository or runs `tombi`, with nothing else on the script line.
/// A job that runs in a container or starts services, which need Docker, or picks its runner with an expression, as an
/// OS matrix does, has its runner chosen for it.
fn is_light_job(id: &str, job: &WorkflowJob) -> bool {
    let runner_chosen = job.container.is_some()
        || job.services.is_some()
        || job
            .runs_on
            .as_ref()
            .and_then(Value::as_str)
            .is_some_and(|runner| runner.contains("${{"));
    if runner_chosen {
        return false;
    }
    if id == GATE_JOB {
        return true;
    }
    let light = |step: &WorkflowStep| LIGHT_ACTIONS.iter().any(|action| step.uses_action(action));
    job.steps.iter().any(light)
        && job.steps.iter().all(|step| match (&step.uses, &step.run) {
            (Some(_), _) => light(step) || step.uses_action("actions/checkout"),
            (None, Some(_)) => step.run_lines().iter().all(|line| {
                commands(line).all(|words| {
                    words
                        .first()
                        .is_none_or(|word| *word == "tombi" || word.starts_with('#'))
                })
            }),
            (None, None) => true,
        })
}

/// Whether `job` runs on [`SLIM_RUNNER`]: its `runs-on` is that label, alone or as a list of one.
fn runs_on_slim(job: &WorkflowJob) -> bool {
    match &job.runs_on {
        Some(Value::String(runner)) => runner == SLIM_RUNNER,
        Some(Value::Array(runners)) => {
            matches!(runners.as_slice(), [runner] if runner == SLIM_RUNNER)
        }
        _ => false,
    }
}

/// Check that the jobs of `ci.yaml`, `workflow`, that install nothing run on [`SLIM_RUNNER`].
fn check_slim_runner(workflow: &Workflow, report: &mut Report) {
    let elsewhere: Vec<&str> = workflow
        .jobs
        .iter()
        .filter(|(id, job)| is_light_job(id, job) && !runs_on_slim(job))
        .map(|(id, _)| id.as_str())
        .collect();
    report.should(
        "ci-ubuntu-slim",
        format!("CI: jobs that install nothing run on {SLIM_RUNNER}"),
        if elsewhere.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "These jobs install nothing, but don't run on {SLIM_RUNNER}: {}",
                elsewhere.join(", ")
            ))
        },
    );
}

/// Check `ci.yaml`'s gate job, `ci-success`, the one check branch protection requires: it runs even when other jobs
/// fail, needs every other job, and fails unless each of them succeeded. Whether or not there is a gate, no job may set
/// `continue-on-error`, which lets the workflow pass when it fails.
fn check_gate(workflow: &Workflow, report: &mut Report) {
    let continuing = workflow.jobs_continuing_on_error();
    report.must(
        "ci-gate-continue-on-error",
        "CI: no job, the gate included, sets continue-on-error",
        if continuing.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "continue-on-error lets the workflow pass when these jobs fail: {}",
                continuing.join(", ")
            ))
        },
    );

    let Some(gate) = workflow.jobs.get(GATE_JOB) else {
        report.must(
            "ci-gate-exists",
            format!("CI: gate job {GATE_JOB}"),
            Outcome::fail(format!("No {GATE_JOB} job")),
        );
        return;
    };
    report.must(
        "ci-gate-exists",
        format!("CI: gate job {GATE_JOB}"),
        Outcome::Pass,
    );

    report.must(
        "ci-gate-always",
        "CI: the gate runs whatever the other jobs' results",
        if gate.runs_always() {
            Ok(())
        } else {
            Err(format!(
                "{GATE_JOB} doesn't set if: always(), so it's skipped when a job fails, which passes a required check"
            ))
        },
    );

    report.must(
        "ci-gate-needs",
        "CI: the gate needs every other job",
        gate.needed_jobs().and_then(|needed| {
            let missing: Vec<_> = workflow
                .jobs
                .keys()
                .map(String::as_str)
                .filter(|id| *id != GATE_JOB && !needed.contains(id))
                .collect();
            if missing.is_empty() {
                Ok(())
            } else {
                Err(format!("{GATE_JOB} doesn't need {}", missing.join(", ")))
            }
        }),
    );

    let not_failing = gate.results_not_failing();
    report.must(
        "ci-gate-results",
        "CI: the gate fails unless every job succeeded",
        if not_failing.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "No step of {GATE_JOB} exits with an error when a job's result is {}, as one with \
                 if: contains(needs.*.result, '<result>') does",
                not_failing.join(" or ")
            ))
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_md_imports_agents_md() {
        for content in [
            "@AGENTS.md\n",
            "# CLAUDE.md\n\n@./AGENTS.md\n",
            "See @AGENTS.md for the commands.\n",
        ] {
            assert!(imports_agents(content), "{content}");
        }
        for content in [
            "",
            "See AGENTS.md.\n",
            "Import it with `@AGENTS.md`.\n",
            "@AGENTS.md.bak\n",
            "@docs/AGENTS.md\n",
            "To share it, add:\n\n```markdown\n@AGENTS.md\n```\n",
            "~~~\n@AGENTS.md\n~~~\n",
        ] {
            assert!(!imports_agents(content), "{content}");
        }
        assert!(imports_agents("```sh\ncargo test\n```\n\n@AGENTS.md\r\n"));
    }

    #[cfg(unix)]
    #[test]
    fn claude_md_may_link_to_agents_md() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::TempDir::new().unwrap();
        let agents = dir.path().join("AGENTS.md");
        let claude = dir.path().join("CLAUDE.md");
        let other = dir.path().join("OTHER.md");
        std::fs::write(&agents, "# AGENTS.md\n").unwrap();
        std::fs::write(&other, "# AGENTS.md\n").unwrap();
        symlink("AGENTS.md", &claude).unwrap();
        assert!(same_file(&claude, &agents));
        assert!(!same_file(&other, &agents));
        assert!(!same_file(&dir.path().join("MISSING.md"), &agents));
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

    fn skip(precommit: &str, workflow: &str) -> Option<Result<(), String>> {
        pre_commit_skip(
            &serde_saphyr::from_str(precommit).unwrap(),
            &serde_saphyr::from_str(workflow).unwrap(),
        )
    }

    #[test]
    fn pre_commit_skips_the_hooks_with_jobs() {
        let precommit = "\
default_stages: [pre-commit]
repos:
  - repo: local
    hooks:
      - {id: clippy}
      - {id: cargo-test}
      - {id: ruff-check, alias: ruff-check-api}
      - {id: markdownlint-cli2}
      - {id: tombi-format}
      - {id: check-yaml}
      - {id: conventional-pre-commit, stages: [commit-msg]}
";
        let workflow = |skip: &str| {
            format!(
                "\
env:
  SKIP: other
jobs:
  pre-commit:
    env:
      SKIP: {skip}
    steps:
      - uses: astral-sh/setup-uv@v7
      - run: uvx pre-commit@4.3.0 run --all-files
  clippy: {{}}
  cargo-test: {{}}
  ruff-check-api: {{}}
  markdownlint: {{}}
  tombi: {{}}
  conventional-pre-commit: {{}}
"
            )
        };
        assert_eq!(
            skip(
                precommit,
                &workflow("\"cargo-test, clippy,ruff-check,markdownlint-cli2,tombi-format\"")
            ),
            Some(Ok(()))
        );
        assert_eq!(
            skip(
                precommit,
                &workflow("cargo-test,markdownlint,tombi-format")
            ),
            Some(Err(
                "SKIP in pre-commit doesn't list these hooks, which have jobs of their own: clippy, \
                 ruff-check-api, markdownlint-cli2"
                    .to_owned()
            ))
        );
        // The step's own SKIP replaces the job's, and the workflow's applies when neither sets one.
        assert_eq!(
            skip(
                "repos: [{repo: local, hooks: [{id: clippy}]}]\n",
                "jobs:\n  clippy: {}\n  hooks:\n    env: {SKIP: clippy}\n    steps:\n      - run: pre-commit run -a\n        env: {SKIP: other}\n"
            ),
            Some(Err(
                "SKIP in hooks doesn't list these hooks, which have jobs of their own: clippy"
                    .to_owned()
            ))
        );
        assert_eq!(
            skip(
                "repos: [{repo: local, hooks: [{id: clippy}]}]\n",
                "env: {SKIP: clippy}\njobs:\n  clippy: {}\n  hooks:\n    steps:\n      - run: pre-commit run -a\n"
            ),
            Some(Ok(()))
        );
        // A hook named like the job running pre-commit has no job of its own.
        assert_eq!(
            skip(
                "repos: [{repo: local, hooks: [{id: lint}]}]\n",
                "jobs:\n  lint:\n    steps:\n      - run: uvx pre-commit run --all-files\n"
            ),
            Some(Ok(()))
        );
        assert_eq!(
            skip(
                precommit,
                "jobs:\n  clippy:\n    steps:\n      - run: cargo clippy\n"
            ),
            None
        );
    }

    #[test]
    fn pre_commit_skip_is_read_from_the_script() {
        let clippy =
            "repos: [{repo: local, hooks: [{id: clippy}, {id: cargo-test}, {id: clippy}]}]\n";
        let script = |env: &str, run: &str| {
            format!(
                "jobs:\n  clippy: {{}}\n  cargo-test: {{}}\n  hooks:\n    {env}steps:\n      - run: {run:?}\n"
            )
        };
        // pre-commit run naming one hook needn't skip any, so it isn't checked; the hook's ID follows the values of
        // options.
        for run in [
            "uvx pre-commit@4.3.0 run clippy --all-files",
            "pre-commit run --hook-stage pre-push clippy",
        ] {
            assert_eq!(skip(clippy, &script("", run)), None, "{run}");
        }
        // A run of another stage's hooks skips those of that stage that have jobs, given by its current or old name,
        // quoted or not.
        let stages = "default_stages: [pre-commit]\nrepos: [{repo: local, hooks: [{id: clippy, stages: [pre-commit, pre-push]}, {id: cargo-test}]}]\n";
        for run in [
            "pre-commit run --hook-stage commit-msg --commit-msg-filename title.txt",
            "pre-commit run --hook-stage=manual --all-files",
        ] {
            assert_eq!(skip(stages, &script("", run)), Some(Ok(())), "{run}");
        }
        for run in [
            "pre-commit run --hook-stage pre-push --all-files",
            "pre-commit run --hook-stage=push -a",
            "pre-commit run --hook-stage 'pre-push' -a",
        ] {
            assert_eq!(
                skip(stages, &script("", run)),
                Some(Err(
                    "SKIP in hooks doesn't list these hooks, which have jobs of their own: clippy"
                        .to_owned()
                )),
                "{run}"
            );
        }
        for run in [
            "pre-commit run --hook-stage pre-commit --all-files",
            "pre-commit run --hook-stage=commit -a",
            "pre-commit run --hook-stage \"pre-commit\" -a",
        ] {
            assert_eq!(
                skip(stages, &script("", run)),
                Some(Err(
                    "SKIP in hooks doesn't list these hooks, which have jobs of their own: clippy, cargo-test"
                        .to_owned()
                )),
                "{run}"
            );
        }
        // SKIP set in the script, as a prefix or by an earlier command, comes before env.
        for run in [
            "SKIP=clippy,cargo-test uvx pre-commit@4.3.0 run --all-files",
            "export SKIP='clippy,cargo-test'\npre-commit run -a",
            "SKIP=clippy,cargo-test; pre-commit run --files a.rs",
        ] {
            assert_eq!(
                skip(clippy, &script("env: {SKIP: other}\n    ", run)),
                Some(Ok(())),
                "{run}"
            );
        }
        // Each hook is named once, and an env given by an expression, which can't be read, sets no SKIP.
        assert_eq!(
            skip(
                clippy,
                &script("env: ${{ fromJSON(vars.ENV) }}\n    ", "pre-commit run -a")
            ),
            Some(Err(
                "SKIP in hooks doesn't list these hooks, which have jobs of their own: clippy, cargo-test"
                    .to_owned()
            ))
        );
        // A job with the hook's own ID counts, as well as the job the standards give it.
        assert_eq!(
            skip(
                "repos: [{repo: local, hooks: [{id: tombi-format}]}]\n",
                "jobs:\n  tombi-format: {}\n  hooks:\n    steps:\n      - run: pre-commit run -a\n"
            ),
            Some(Err(
                "SKIP in hooks doesn't list these hooks, which have jobs of their own: tombi-format"
                    .to_owned()
            ))
        );

        // pre-commit run in another directory runs another configuration.
        for elsewhere in [
            "jobs:\n  clippy: {}\n  hooks:\n    steps:\n      - run: pre-commit run -a\n        working-directory: ${{ runner.temp }}/project\n",
            "jobs:\n  clippy: {}\n  hooks:\n    defaults: {run: {working-directory: frontend}}\n    steps:\n      - run: pre-commit run -a\n",
            "defaults: {run: {working-directory: frontend}}\njobs:\n  clippy: {}\n  hooks:\n    steps:\n      - run: pre-commit run -a\n",
        ] {
            assert_eq!(skip(clippy, elsewhere), None, "{elsewhere}");
        }
        assert!(matches!(
            skip(
                clippy,
                "jobs:\n  clippy: {}\n  hooks:\n    defaults: {run: {working-directory: frontend}}\n    steps:\n      - run: pre-commit run -a\n        working-directory: .\n"
            ),
            Some(Err(_))
        ));
    }

    #[test]
    fn slim_runner_as_a_label_or_list_of_one() {
        let slim = |yaml: &str| runs_on_slim(&serde_saphyr::from_str(yaml).unwrap());
        assert!(slim("runs-on: ubuntu-slim\n"));
        assert!(slim("runs-on: [ubuntu-slim]\n"));
        assert!(!slim("runs-on: ubuntu-latest\n"));
        assert!(!slim("runs-on: [ubuntu-slim, self-hosted]\n"));
        assert!(!slim("steps: []\n"));
    }

    #[test]
    fn light_jobs_install_nothing() {
        let light = |id: &str, yaml: &str| is_light_job(id, &serde_saphyr::from_str(yaml).unwrap());
        assert!(light(GATE_JOB, "steps:\n  - run: exit 1\n"));
        assert!(light(
            "markdownlint",
            "steps:\n  - uses: actions/checkout@v6\n  - uses: DavidAnson/markdownlint-cli2-action@v23\n"
        ));
        assert!(light(
            "tombi",
            "steps:\n  - uses: actions/checkout@v6\n  - uses: tombi-toml/setup-tombi@v1\n  - run: tombi format --check\n"
        ));
        assert!(light(
            "zizmor",
            "steps:\n  - uses: zizmorcore/zizmor-action@v0\n"
        ));

        assert!(light(
            "tombi",
            "steps:\n  - uses: tombi-toml/setup-tombi@v1\n  - run: |\n      # Check formatting\n      tombi format --check\n"
        ));

        assert!(!light("lint", "steps:\n  - uses: actions/checkout@v6\n"));
        assert!(!light(
            "markdownlint",
            "services: {db: {image: postgres}}\nsteps:\n  - uses: DavidAnson/markdownlint-cli2-action@v23\n"
        ));
        assert!(!light(
            "zizmor",
            "runs-on: ${{ matrix.os }}\nsteps:\n  - uses: zizmorcore/zizmor-action@v0\n"
        ));
        assert!(!light(
            GATE_JOB,
            "container: node:24\nsteps:\n  - run: exit 1\n"
        ));
        assert!(!light("zizmor", "steps:\n  - run: uvx zizmor .\n"));
        assert!(!light(
            "tombi",
            "steps:\n  - uses: tombi-toml/setup-tombi@v1\n  - run: tombi format --check && cargo fmt --check\n"
        ));
        assert!(!light(
            "markdownlint",
            "steps:\n  - uses: actions/setup-node@v6\n  - uses: DavidAnson/markdownlint-cli2-action@v23\n"
        ));
    }

    fn pre_commit(run: &str) -> Result<(), Issue> {
        let yaml = format!("jobs:\n  hooks:\n    steps:\n      - run: {run:?}\n");
        pre_commit_in_ci(&serde_saphyr::from_str(&yaml).unwrap())
    }

    #[test]
    fn pre_commit_runs_every_file_pinned() {
        for run in [
            "uvx pre-commit@4.3.0 run --all-files --show-diff-on-failure",
            "uvx \"pre-commit@$PRE_COMMIT_VERSION\" run --all-files",
            "uvx --from pre-commit==4.3.0 pre-commit run -a",
            "pip install pre-commit==4.3.0\npre-commit run --show-diff-on-failure --all-files",
            "uv tool install 'pre-commit==4.3.0' && pre-commit run \\\n  --all-files",
            "uvx --from=pre-commit==4.3.0rc1 pre-commit run -a",
        ] {
            assert_eq!(pre_commit(run), Ok(()), "{run}");
        }
        for (run, issue) in [
            (
                "pre-commit install",
                Issue::fail("No step runs pre-commit run"),
            ),
            (
                "uvx pre-commit@4.3.0 run",
                Issue::fail("pre-commit run doesn't pass --all-files, so it checks no files"),
            ),
            (
                "uvx pre-commit@4.3.0 run --all-files-later",
                Issue::fail("pre-commit run doesn't pass --all-files, so it checks no files"),
            ),
            (
                "uvx pre-commit@4.3.0 run cargo-test; echo --all-files",
                Issue::fail("pre-commit run doesn't pass --all-files, so it checks no files"),
            ),
            (
                "uvx pre-commit run --all-files",
                Issue::warn(
                    "pre-commit isn't pinned to a version (pre-commit@x.y.z or pre-commit==x.y.z)",
                ),
            ),
            (
                "uvx pre-commit@latest run --all-files",
                Issue::warn(
                    "pre-commit isn't pinned to a version (pre-commit@x.y.z or pre-commit==x.y.z)",
                ),
            ),
            (
                "pip install pre-commit==4.3.0\nuvx pre-commit run -a",
                Issue::warn(
                    "pre-commit isn't pinned to a version (pre-commit@x.y.z or pre-commit==x.y.z)",
                ),
            ),
            (
                "pip install pre-commit>=4 && pre-commit run -a",
                Issue::warn(
                    "pre-commit isn't pinned to a version (pre-commit@x.y.z or pre-commit==x.y.z)",
                ),
            ),
        ] {
            assert_eq!(pre_commit(run), Err(issue), "{run}");
        }

        // A pin in another job doesn't apply.
        let split: Workflow = serde_saphyr::from_str(
            "jobs:\n  a:\n    steps:\n      - run: pip install pre-commit==4.3.0\n  b:\n    steps:\n      - run: pre-commit run -a\n",
        )
        .unwrap();
        assert!(pre_commit_in_ci(&split).is_err());
        let action: Workflow = serde_saphyr::from_str(
            "jobs:\n  a:\n    steps:\n      - uses: pre-commit/action@v3.0.1\n",
        )
        .unwrap();
        assert_eq!(
            pre_commit_in_ci(&action),
            Err(Issue::warn(
                "No step runs pre-commit run; pre-commit/action installs pre-commit unpinned"
            ))
        );
    }

    #[test]
    fn reuse_lint_excludes_lint_file() {
        for run in [
            "reuse lint",
            "uvx reuse lint --json",
            "reuse  lint\necho done",
            "reuse lint; echo",
        ] {
            assert!(REUSE_LINT.is_match(run), "{run}");
        }
        for run in [
            "reuse lint-file src/main.rs",
            "reuse --root . lint",
            "nonreuse lint",
        ] {
            assert!(!REUSE_LINT.is_match(run), "{run}");
        }
    }

    #[test]
    fn setup_uv_pins_an_exact_version() {
        let workflow = |yaml: &str| -> Workflow { serde_saphyr::from_str(yaml).unwrap() };
        assert_eq!(
            setup_uv_exact(&workflow(
                "jobs:\n  test:\n    steps:\n      - run: uv sync\n"
            )),
            None
        );
        // Jobs that only run tools through uvx needn't pin uv.
        assert_eq!(
            setup_uv_exact(&workflow(
                "
jobs:
  pre-commit:
    steps:
      - uses: astral-sh/setup-uv@abc # v7.1.0
        with:
          enable-cache: false
      - run: rm -rf ~/.cache/uv && pip install uv
      - run: uvx pre-commit@4.3.0 run --all-files
"
            )),
            None
        );
        assert_eq!(
            setup_uv_exact(&workflow(
                "
jobs:
  pre-commit:
    steps:
      - uses: astral-sh/setup-uv@abc # v7.1.0
      - run: uvx pre-commit@4.3.0 run --all-files
  pytest:
    steps:
      - uses: astral-sh/setup-uv@abc
        with:
          version: 0.9.5
          python-version: \"3.12\"
      - run: uv run --locked pytest
"
            )),
            Some(Ok(()))
        );
        assert_eq!(
            setup_uv_exact(&workflow(
                "
jobs:
  audit:
    steps:
      - uses: astral-sh/setup-uv@v7
      - run: uv audit --locked
  lint:
    steps:
      - uses: astral-sh/setup-uv@v7
        with:
          version: \">=0.9\"
      - run: uv --directory api run ruff check
  test:
    steps:
      - uses: astral-sh/setup-uv@v7
        with:
          version: \"0.9\"
      - uses: astral-sh/setup-uv@v7
        with:
          version: \"0.9.5\"
      - run: cd api && uv sync --locked
  matrix:
    steps:
      - uses: astral-sh/setup-uv@v7
        with:
          version: ${{ matrix.uv }}
      - run: uv lock --check
  file:
    steps:
      - uses: astral-sh/setup-uv@v7
        with:
          version-file: uv.toml
      - run: ~/.local/bin/uv run mypy .
"
            )),
            Some(Err(
                "setup-uv doesn't pin an exact uv version in: audit (no version), file (version-file, not version), \
                 lint (>=0.9), matrix (${{ matrix.uv }}), test (0.9)"
                    .to_owned()
            ))
        );
    }

    #[test]
    fn zizmor_args_need_a_strict_persona() {
        for args in [
            "--pedantic",
            "--offline --pedantic",
            "-p",
            "--persona=pedantic",
            "--persona=auditor",
            "--persona pedantic .",
        ] {
            assert!(zizmor_args_strict(args.split_whitespace()), "{args}");
        }
        for args in [
            "",
            "--offline",
            "--persona=regular",
            "--persona",
            "--persona regular --pedantic-ish",
            "pedantic",
        ] {
            assert!(!zizmor_args_strict(args.split_whitespace()), "{args}");
        }
    }

    #[test]
    fn zizmor_hooks_need_a_strict_persona() {
        let hook = |entry: Option<&str>, args: &[&str]| PrecommitHook {
            id: "zizmor".to_owned(),
            entry: entry.map(str::to_owned),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            ..PrecommitHook::default()
        };
        assert!(zizmor_hook_strict(&hook(None, &["--pedantic"])));
        assert!(zizmor_hook_strict(&hook(Some("zizmor --pedantic"), &[])));
        assert!(zizmor_hook_strict(&hook(
            Some("uvx zizmor"),
            &["--persona", "auditor"]
        )));
        assert!(!zizmor_hook_strict(&hook(None, &[])));
        assert!(!zizmor_hook_strict(&hook(Some("zizmor --offline"), &["."])));
        assert!(!zizmor_hook_strict(&hook(Some("echo --pedantic"), &[])));
    }

    fn zizmor_in(yaml: &str) -> Result<(), Issue> {
        let workflow: Workflow = serde_saphyr::from_str(yaml).expect("workflow parses");
        zizmor_in_ci(&workflow)
    }

    #[test]
    fn zizmor_runs_in_ci() {
        let script = |run: &str| format!("jobs:\n  z:\n    steps:\n      - run: \"{run}\"\n");
        for run in [
            "zizmor --pedantic .",
            "uvx zizmor@1.32.0 --persona=pedantic --format github .",
            "echo start\\n  ~/.cargo/bin/zizmor --pedantic .",
            "zizmor --format github \\\\\\n  --pedantic .",
        ] {
            assert_eq!(zizmor_in(&script(run)), Ok(()), "{run}");
        }
        assert_eq!(
            zizmor_in(&script("zizmor .")),
            Err(Issue::warn("A step runs zizmor without --pedantic"))
        );
        assert_eq!(
            zizmor_in(&script("echo zizmorish --pedantic")),
            Err(Issue::fail(
                "No step runs zizmor or uses zizmorcore/zizmor-action"
            ))
        );
    }

    #[test]
    fn zizmor_action_must_be_pedantic_and_gating() {
        let step = |with: &str| {
            format!(
                "jobs:\n  z:\n    steps:\n      - uses: zizmorcore/zizmor-action@abc # v0.8.1\n        with: {{{with}}}\n"
            )
        };
        assert_eq!(
            zizmor_in(&step("persona: pedantic, advanced-security: false")),
            Ok(())
        );
        assert_eq!(
            zizmor_in(&step("persona: auditor, advanced-security: \"false\"")),
            Ok(())
        );
        assert_eq!(
            zizmor_in(&step("advanced-security: false")),
            Err(Issue::warn("zizmor-action doesn't set persona: pedantic"))
        );
        assert_eq!(
            zizmor_in(&step("persona: pedantic")),
            Err(Issue::fail(
                "zizmor-action doesn't set advanced-security: false"
            ))
        );
        assert_eq!(
            zizmor_in(&step("")),
            Err(Issue::fail(
                "zizmor-action doesn't set persona: pedantic or advanced-security: false"
            ))
        );

        // A step that fails the job on findings decides, though another uploads them instead.
        assert_eq!(
            zizmor_in(&format!(
                "{}      - run: zizmor .\n",
                step("persona: pedantic")
            )),
            Err(Issue::warn(
                "A step runs zizmor without --pedantic; zizmor-action doesn't set advanced-security: false"
            ))
        );
    }

    fn conventional(repo: &str, hook: &str) -> Result<(), Issue> {
        conventional_hook_in(
            &serde_saphyr::from_str(&format!(
                "repos:\n  - repo: {repo}\n    hooks:\n      - {hook}\n"
            ))
            .unwrap(),
        )
    }

    const CONVENTIONAL_REPO: &str = "https://github.com/compilerla/conventional-pre-commit";

    #[test]
    fn conventional_pre_commit_runs_as_the_last_program_word_or_module() {
        for entry in [
            "uv run --with conventional-pre-commit python -m conventional_pre_commit.hook",
            "uvx --from conventional-pre-commit conventional-pre-commit",
        ] {
            let hook = format!(
                "{{id: conventional-pre-commit, entry: {entry} feat fix perf refactor style test docs build ci revert \
                 chore, language: system, stages: [commit-msg]}}"
            );
            assert_eq!(conventional("local", &hook), Ok(()), "{entry}");
        }
    }

    #[test]
    fn conventional_hook_passes_with_commit_msg_and_the_recognized_types() {
        for (repo, hook) in [
            (
                CONVENTIONAL_REPO,
                "{id: conventional-pre-commit, stages: [commit-msg], args: [feat, fix, perf, refactor, style, test, docs, \
                 build, ci, revert, chore]}",
            ),
            // The hook's repository sets the stage.
            (
                CONVENTIONAL_REPO,
                "{id: conventional-pre-commit, args: [feat, fix, perf, refactor, style, test, docs, build, ci, revert, \
                 chore]}",
            ),
            (
                "local",
                "{id: conventional-pre-commit, entry: python -m conventional_pre_commit.hook feat fix perf refactor \
                 style test docs build ci revert chore, language: python, stages: [commit-msg]}",
            ),
            // feat and fix are added to types that name neither.
            (
                CONVENTIONAL_REPO,
                "{id: conventional-pre-commit, args: [--st, --strict, --scopes, api, perf, refactor, style, test, docs, \
                 build, ci, revert, chore]}",
            ),
            (
                "local",
                "{id: conventional-pre-commit, entry: .venv/bin/conventional-pre-commit --strict, language: system, \
                 stages: [commit-msg, prepare-commit-msg], args: [feat, fix, perf, refactor, style, test, docs, build, \
                 ci, revert, chore]}",
            ),
        ] {
            assert_eq!(conventional(repo, hook), Ok(()), "{hook}");
        }
    }

    #[test]
    fn conventional_hook_fails_when_absent_or_broken_and_warns_otherwise() {
        for (repo, hook, issue) in [
            (
                CONVENTIONAL_REPO,
                "{id: commitizen}",
                Issue::fail("No conventional-pre-commit hook"),
            ),
            (
                CONVENTIONAL_REPO,
                "{id: conventional-pre-commit, stages: [pre-commit]}",
                Issue::fail(
                    "conventional-pre-commit doesn't run in the commit-msg stage and passes no commit types, so it allows \
                 the hook's defaults, which a release may change",
                ),
            ),
            (
                CONVENTIONAL_REPO,
                "{id: conventional-pre-commit, args: [--strict]}",
                Issue::warn(
                    "conventional-pre-commit passes no commit types, so it allows the hook's defaults, which a release may \
                 change",
                ),
            ),
            (
                "local",
                "{id: conventional-pre-commit, entry: python -m conventional_pre_commit feat, stages: [commit-msg]}",
                Issue::fail("conventional-pre-commit doesn't run conventional-pre-commit"),
            ),
            (
                "local",
                "{id: conventional-pre-commit, entry: conventional-pre-commit feat, language: system}",
                Issue::fail(
                    "conventional-pre-commit doesn't run in the commit-msg stage and doesn't allow fix, perf, refactor, \
                 style, test, docs, build, ci, revert, chore",
                ),
            ),
            (
                "local",
                "{id: conventional-pre-commit, entry: commitlint --edit, stages: [commit-msg]}",
                Issue::fail("conventional-pre-commit doesn't run conventional-pre-commit"),
            ),
            (
                CONVENTIONAL_REPO,
                "{id: conventional-pre-commit, args: [--sc, chore, --, feat, fix, wip, --docs]}",
                Issue::warn(
                    "conventional-pre-commit doesn't allow perf, refactor, style, test, docs, build, ci, revert, chore and \
                 also allows --docs, wip",
                ),
            ),
            (
                CONVENTIONAL_REPO,
                "{id: conventional-pre-commit, args: [feat, fix, --strict, perf, refactor, style, test, docs, build, \
                 ci, revert, chore]}",
                Issue::fail(
                    "conventional-pre-commit passes commit types before --strict, so it fails on every commit",
                ),
            ),
            // Only the hook's own repository sets the stage, and an empty list falls back to default_stages.
            (
                "https://github.com/example/mirror",
                "{id: conventional-pre-commit, args: [feat, fix, perf, refactor, style, test, docs, build, ci, revert, \
                 chore]}",
                Issue::fail("conventional-pre-commit doesn't run in the commit-msg stage"),
            ),
            (
                CONVENTIONAL_REPO,
                "{id: conventional-pre-commit, stages: [], args: [feat, fix, perf, refactor, style, test, docs, build, \
                 ci, revert, chore]}",
                Issue::fail("conventional-pre-commit doesn't run in the commit-msg stage"),
            ),
            (
                CONVENTIONAL_REPO,
                "{id: conventional-pre-commit, stages: [commit-msg], args: [--scopes=api, docs]}",
                Issue::warn(
                    "conventional-pre-commit doesn't allow perf, refactor, style, test, build, ci, revert, chore",
                ),
            ),
        ] {
            assert_eq!(conventional(repo, hook), Err(issue), "{hook}");
        }
    }

    #[test]
    fn conventional_hook_is_judged_by_its_best_hook() {
        let config = serde_saphyr::from_str(&format!(
            "repos:\n  - repo: {CONVENTIONAL_REPO}\n    hooks:\n      - {{id: conventional-pre-commit, args: [feat, fix]}}\n\
             \x20 - repo: local\n    hooks:\n      - {{id: conventional-pre-commit, alias: lint, entry: commitlint \
             --edit, stages: [commit-msg]}}\n"
        ))
        .unwrap();
        assert_eq!(
            conventional_hook_in(&config),
            Err(Issue::warn(
                "conventional-pre-commit doesn't allow perf, refactor, style, test, docs, build, ci, revert, chore; \
                 conventional-pre-commit doesn't run conventional-pre-commit"
            ))
        );
    }

    #[test]
    fn pre_commit_hooks_fail_when_missing_and_warn_in_another_stage() {
        let config = |hooks: &str| -> PrecommitConfig {
            serde_saphyr::from_str(&format!(
                "repos:\n  - repo: https://github.com/pre-commit/pre-commit-hooks\n    hooks: [{hooks}]\n"
            ))
            .unwrap()
        };
        let all: Vec<_> = PRE_COMMIT_HOOKS
            .iter()
            .map(|id| format!("{{id: {id}}}"))
            .collect();
        assert_eq!(pre_commit_hooks_in(&config(&all.join(", "))), Ok(()));

        let manual = all
            .iter()
            .map(|hook| hook.replace("{id: check-yaml}", "{id: check-yaml, stages: [manual]}"))
            .collect::<Vec<_>>()
            .join(", ");
        assert_eq!(
            pre_commit_hooks_in(&config(&manual)),
            Err(Issue::warn(
                "The check-yaml hook doesn't run in the pre-commit stage"
            ))
        );

        let without_toml: Vec<_> = manual
            .split(", ")
            .filter(|hook| *hook != "{id: check-toml}")
            .collect();
        assert_eq!(
            pre_commit_hooks_in(&config(&without_toml.join(", "))),
            Err(Issue::fail(
                "Missing: check-toml; The check-yaml hook doesn't run in the pre-commit stage"
            ))
        );
    }

    fn markdownlint(steps: &str) -> Result<(), Issue> {
        markdownlint_in_ci(
            &serde_saphyr::from_str(&format!("jobs:\n  markdownlint:\n    steps:\n{steps}"))
                .unwrap(),
        )
    }

    #[test]
    fn markdownlint_must_lint_every_file() {
        const ACTION: &str = "      - uses: DavidAnson/markdownlint-cli2-action@0000000000000000000000000000000000000000\n";
        for steps in [
            format!("{ACTION}        with:\n          globs: '**/*.md'\n"),
            format!(
                "{ACTION}        with:\n          globs: |\n            **/*.md\n            !vendor\n"
            ),
            format!(
                "{ACTION}        with:\n          globs: 'README.md;**/*.{{md,markdown}}'\n          separator: ';'\n"
            ),
            "      - run: npx markdownlint-cli2 '**/*.md'\n".to_owned(),
            "      - run: pnpm exec markdownlint-cli2@0.24.0 \"**/*.md\" --fix\n".to_owned(),
            // A step that lints only the root files doesn't hide one that lints them all.
            format!("{ACTION}      - run: markdownlint-cli2 '**/*.md'\n"),
        ] {
            assert_eq!(markdownlint(&steps), Ok(()), "{steps}");
        }
        for (steps, issue) in [
            (
                "      - run: echo\n".to_owned(),
                Issue::fail(
                    "No step uses DavidAnson/markdownlint-cli2-action or runs markdownlint-cli2",
                ),
            ),
            (
                ACTION.to_owned(),
                Issue::warn(
                    "markdownlint-cli2 doesn't lint '**/*.md', so it checks only the files at the root",
                ),
            ),
            (
                format!("{ACTION}        with:\n          globs: '*.md'\n"),
                Issue::warn(
                    "markdownlint-cli2 doesn't lint '**/*.md', so it checks only the files at the root",
                ),
            ),
            (
                format!("{ACTION}        with:\n          globs: '**/*.md,docs'\n"),
                Issue::warn(
                    "markdownlint-cli2 doesn't lint '**/*.md', so it checks only the files at the root",
                ),
            ),
            // Unquoted, the shell expands the glob itself.
            (
                "      - run: markdownlint-cli2 **/*.md\n".to_owned(),
                Issue::warn(
                    "markdownlint-cli2 doesn't lint '**/*.md', so it checks only the files at the root",
                ),
            ),
        ] {
            assert_eq!(markdownlint(&steps), Err(issue), "{steps}");
        }
    }

    #[test]
    fn tombi_format_must_check() {
        for run in [
            "tombi format --check",
            "tombi format --check --diagnostics-format github",
            "uvx tombi format . --check",
        ] {
            assert!(TOMBI_FORMAT_CHECK.is_match(run), "{run}");
        }
        for run in [
            "tombi format",
            "tombi format\necho --check",
            "tombi lint --check",
            "tombi format --checked",
        ] {
            assert!(!TOMBI_FORMAT_CHECK.is_match(run), "{run}");
        }
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
