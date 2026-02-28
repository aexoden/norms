// SPDX-License-Identifier: Apache-2.0 OR MIT
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Common checks that apply to all projects reqgardless of language.

use crate::context::ProjectContext;
use crate::report::{Report, file_exists, matches_regex};

//
// Required files
//

pub fn check_readme(ctx: &ProjectContext, report: &mut Report) {
    report.must("README.md", file_exists(&ctx.path().join("README.md")));
}

pub fn check_license(ctx: &ProjectContext, report: &mut Report) {
    let candidates = ["LICENSE", "LICENSE-APACHE-2.0", "LICENSE-MIT"];
    let found = candidates.iter().any(|name| ctx.path().join(name).exists());
    report.must("LICENSE", found);
}

pub fn check_changelog(ctx: &ProjectContext, report: &mut Report) {
    report.should(
        "CHANGELOG.md",
        file_exists(&ctx.path().join("CHANGELOG.md")),
    );
}

//
// Git configuration
//

pub fn check_gitignore(ctx: &ProjectContext, report: &mut Report) {
    report.must(".gitignore", file_exists(&ctx.path().join(".gitignore")));
}

pub fn check_gitattributes(ctx: &ProjectContext, report: &mut Report) {
    let status = ctx.gitattributes();
    let Some(content) = report.require_parsed(".gitattributes", status) else {
        return;
    };

    report.should(
        ".gitattributes: text=auto",
        matches_regex(content, r"(?m)^\*\s.+text=auto"),
    );

    report.should(
        ".gitattributes: eol=lf",
        matches_regex(content, r"(?m)^\*\s.+eol=lf"),
    );

    report.should(
        ".gitattributes: binary markers",
        matches_regex(content, r"(?m)^\*\.[a-zA-Z0-9]+\s+binary"),
    );
}

//
// Editor configuration
//

pub fn check_editorconfig(ctx: &ProjectContext, report: &mut Report) {
    let status = ctx.editorconfig();
    let Some(content) = report.require_parsed(".editorconfig", status) else {
        return;
    };

    let settings = [
        ("root=true", "root = true"),
        ("charset=utf-8", "charset = utf-8"),
        ("end_of_line=lf", "end_of_line = lf"),
        ("indent_style=space", "indent_style = space"),
        ("indent_size=4", "indent_size = 4"),
        ("insert_final_newline=true", "insert_final_newline = true"),
        (
            "trim_trailing_whitespace=true",
            "trim_trailing_whitespace = true",
        ),
    ];

    for (label, needle) in settings {
        report.should(
            format!(".editorconfig: {label}"),
            matches_regex(content, &regex::escape(needle)),
        );
    }
}

//
// Development environment
//

pub fn check_devbox(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = report.require_parsed("devbox.json", ctx.devbox()) else {
        return;
    };

    report.should("devbox.json: has $schema", config.schema.is_some());
    report.should("devbox.json: has packages", !config.packages.is_empty());

    // Check for devbox.lock
    report.should("devbox.lock", file_exists(&ctx.path().join("devbox.lock")));
}

//
// Pre-commit
//

pub fn check_precommit(ctx: &ProjectContext, report: &mut Report) {
    report.must(
        ".pre-commit-config.yaml",
        file_exists(&ctx.path().join(".pre-commit-config.yaml")),
    );
}

//
// Dependency management
//

pub fn check_renovate(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = report.recommend_parsed("renovate.json", ctx.renovate()) else {
        return;
    };

    report.should("renovate.json: has $schema", config.schema.is_some());
    report.should("renovate.json: has extends", !config.extends.is_empty());
    report.should(
        "renovate.json: extends best-practices",
        config.extends_best_practices(),
    );
}
