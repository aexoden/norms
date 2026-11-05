// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Common checks that apply to all projects reqgardless of language.

use crate::config::SHARED_PRESET;
use crate::context::{CI_WORKFLOW, ConfigStatus, ProjectContext};
use crate::report::{Outcome, Report, file_exists, matches_regex};

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

pub(crate) fn check_changelog(ctx: &ProjectContext, report: &mut Report) {
    report.should(
        "changelog-exists",
        "CHANGELOG.md",
        file_exists(&ctx.path().join("CHANGELOG.md")),
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

    let Some(config) = report.recommend_parsed("reuse-toml", "REUSE.toml", ctx.reuse_toml()) else {
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

    // Check for devbox.lock
    report.should(
        "devbox-lock-exists",
        "devbox.lock",
        file_exists(&ctx.path().join("devbox.lock")),
    );
}

//
// Pre-commit
//

pub(crate) fn check_precommit(ctx: &ProjectContext, report: &mut Report) {
    report.must(
        "precommit-config-exists",
        ".pre-commit-config.yaml",
        file_exists(&ctx.path().join(".pre-commit-config.yaml")),
    );
}

//
// Dependency management
//

pub(crate) fn check_renovate(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = report.recommend_parsed("renovate", "renovate.json", ctx.renovate()) else {
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

    // Every other workflow must parse too. The checks above already cover ci.yaml.
    for (name, status) in ctx
        .workflows()
        .iter()
        .filter(|(name, _)| *name != CI_WORKFLOW)
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
}
