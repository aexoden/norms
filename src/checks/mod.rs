// SPDX-License-Identifier: Apache-2.0 OR MIT
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Check modules and runner.
//!
//! Checks are plain functions with the signature:
//!
//! ```ignore
//! fn check_something(ctx: &ProjectContext, report: &mut Report) { ... }
//! ```
//!
//! Registration is explicit - no macros or global side effects. New checks
//! are added by writing the function and listing it in [`run_all_checks`].

pub mod common;
pub mod python;

use crate::context::ProjectContext;
use crate::models::Language;
use crate::report::Report;

/// Run all applicable checks for the given project context.
///
/// Common checks always run. Language-specific checks run for each detected language.
pub fn run_all_checks(ctx: &ProjectContext, report: &mut Report) {
    // Common checks (apply to all projects)
    common::check_readme(ctx, report);
    common::check_license(ctx, report);
    common::check_changelog(ctx, report);
    common::check_gitignore(ctx, report);
    common::check_gitattributes(ctx, report);
    common::check_editorconfig(ctx, report);
    common::check_devbox(ctx, report);
    common::check_precommit(ctx, report);
    common::check_renovate(ctx, report);
    common::check_github_actions(ctx, report);

    // Language-specific checks
    for &language in ctx.languages() {
        #[expect(clippy::single_match)] // Future languages will be added here
        match language {
            Language::Python => {
                python::check_pyproject(ctx, report);
                python::check_src_layout(ctx, report);
                python::check_uv_lock(ctx, report);
                python::check_dependency_groups(ctx, report);
                python::check_ruff(ctx, report);
                python::check_ruff_lint(ctx, report);
                python::check_mypy(ctx, report);
                python::check_precommit_hooks(ctx, report);
                python::check_devbox_uv(ctx, report);
                python::check_ci(ctx, report);
            }
            _ => {}
        }
    }
}
