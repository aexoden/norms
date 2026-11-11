# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/2.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Add the `norms-commit-msg` and `norms-verify` hooks to the template, with a
  `norms-verify` CI job that runs the pinned hook and a `pr-title.yaml` workflow
  that checks pull request titles with the commit message hooks.
- **Breaking:** Check that projects run norms: the `norms-commit-msg` and
  `norms-verify` hooks, the latter with `--deny-warnings`, a CI job running
  `norms verify` with the whole history, and a `pr-title.yaml` workflow running
  the commit message hooks on pull request titles. Add them as the template
  does.

### Changed

- `STANDARDS.md` marks a breaking change in a commit message with `!` before
  the colon, matching the changelog's `**Breaking:**` entries, writes commit
  types in lowercase, and gives footers git's trailer format.
- `norms commit-msg` rejects a commit type that isn't in lowercase, such as
  `Feat:`, which `conventional-pre-commit` lets through.

## [0.2.0] - 2026-11-10

norms 0.2.0 replaces the Python prototype of 0.1.0 with a rewrite in Rust, and
adds the standards it checks, a Copier template for new projects, and pre-commit
hooks. Projects that passed 0.1.0 should expect findings from the new checks:
each says what it expects, `STANDARDS.md` explains the standard behind it, and
`norms.toml` can waive a check that a project departs from on purpose.

### Added

- Add `STANDARDS.md`, the standards `norms verify` checks projects against.
- Add a Copier template that renders new projects following the standards, for
  Rust, Python and TypeScript in any combination, with every version it pins
  kept in `versions.yml`.
- Add the `norms-verify` and `norms-commit-msg` pre-commit hooks.
- Add `norms commit-msg`, which checks a commit message's subject and body line
  lengths.
- Add `norms --version`, which prints the version of norms.
- Add `--deny-warnings` to `norms verify`, which then also fails when a check
  warns.
- Add `norms.toml`, which waives checks with a reason and excludes directories
  from component detection.
- Give every check a stable ID, shown in the text and JSON reports.
- Add the detected components, each result's component and the number of
  skipped checks to `norms verify --json`.
- Detect components anywhere in the project by their marker files, label each
  finding with its component, and check Cargo workspace members with their
  workspace. C++ components are detected but not yet checked.
- **Breaking:** Check the repository's own files: the changelog, `AGENTS.md` and
  `CLAUDE.md`, markdownlint's configuration, and REUSE licensing.
- **Breaking:** Check the pre-commit configuration: the required hooks and how
  they run, their stages, the installed hook types, and unique hook IDs.
- **Breaking:** Check the GitHub Actions workflows: the jobs CI must run and how
  they run them, the gate job, triggers, timeouts, concurrency, runners, zizmor
  and the scheduled audit.
- Check that `renovate.json` uses the shared preset and that `devbox.json`
  installs no toolchains.
- **Breaking:** Require a `devbox.lock` beside `devbox.json`.
- **Breaking:** Check Rust components: the toolchain pin, Cargo manifests and
  lints, exact dependency pins in applications, the release age, the Clippy and
  cargo-deny configuration, and that libraries test their MSRV.
- **Breaking:** Check Python components: the Python and uv pins, classifiers,
  exact dependency pins in applications, the release age, versions from git
  tags, the `src/` layout, the Ruff, mypy, pytest and uv configuration, kept in
  `pyproject.toml` rather than files of their own, and that libraries test their
  oldest Python.
- **Breaking:** Check TypeScript components: `package.json`, the Node.js and
  pnpm pins, exact dependency pins in applications, pnpm's release age and build
  settings, the tsconfig compiler options, the ESLint, Prettier, Vite and Vitest
  setup, and that each tool has one lock or configuration file.

### Changed

- **Breaking:** Rewrite norms in Rust. Run it through the hooks, or install it
  with `cargo install --git https://github.com/aexoden/norms --tag v0.2.0`,
  rather than as a Python package.
- License norms under MIT OR Apache-2.0.
- **Breaking:** Leave out a result's `message` in `norms verify --json` when it
  is empty, rather than give an empty string. Treat a missing `message` as
  empty.
- Check the properties `.editorconfig` gives each file and the line endings and
  diff drivers `.gitattributes` sets, rather than only a few `.editorconfig`
  settings and whether `.gitattributes` exists.
- Check `devbox.json` only when it exists, rather than requiring it.
- **Breaking:** Fail when `renovate.json` is missing, rather than warn.
- **Breaking:** Require license texts in `LICENSES/`, each named by its SPDX
  identifier and with an identical copy at the root, rather than a single
  `LICENSE` file: copy `LICENSE` to `LICENSES/`, under its license's identifier.

### Removed

- Stop warning when norms finds no language it checks.

## [0.1.0] - 2026-01-10

### Added

- Add initial version of the `norms` verification tool.

[Unreleased]: https://github.com/aexoden/norms/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/aexoden/norms/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/aexoden/norms/releases/tag/v0.1.0
