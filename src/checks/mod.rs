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

use crate::context::ProjectContext;
use crate::report::Report;

/// Run all applicable checks for the given project context.
///
/// Common checks always run. Language-specific echsk run for each detected language.
pub fn run_all_checks(ctx: &ProjectContext, report: &mut Report) {
    // Common checks (apply to all projects)
    common::check_readme(ctx, report);
    common::check_license(ctx, report);
    common::check_changelog(ctx, report);
    common::check_gitignore(ctx, report);
    common::check_gitattributes(ctx, report);
    common::check_editorconfig(ctx, report);
    common::check_devbox(ctx, report);
}
