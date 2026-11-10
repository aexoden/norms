// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Common checks that apply to all projects regardless of language.

mod ci;
mod files;
mod precommit;

pub(crate) use ci::{SETUP_UV, check_github_actions, is_exact_uv_version};
pub(crate) use files::{
    check_agent_guidance, check_changelog, check_components, check_devbox, check_editorconfig,
    check_gitattributes, check_gitignore, check_licenses, check_markdownlint, check_norms_config,
    check_readme, check_renovate,
};
pub(crate) use precommit::check_precommit;
