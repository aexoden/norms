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
