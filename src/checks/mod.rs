// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Check modules, and the runner that runs them.
//!
//! Checks are plain functions with the signature:
//!
//! ```ignore
//! fn check_something(ctx: &ProjectContext, report: &mut Report) { ... }
//! ```
//!
//! Registration is explicit - no macros or global side effects. New checks
//! are added by writing the function and listing it in [`run_all_checks`].

pub(crate) mod common;
pub(crate) mod python;
pub(crate) mod rust;
pub(crate) mod typescript;

mod runner;

pub(crate) use runner::run_all_checks;
