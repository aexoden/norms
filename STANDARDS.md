# Project Standards

This file documents the standards intended to be used by my projects. Some of these
standards are enforced by the `norms` tool. These standards are highly opinionated
and may not be appropriate for all projects. In developing these standards, there
were several guiding principles:

1. Humans shouldn't manually enforce things that tools can check.
2. Apply maximum strictness by default, only relaxing selectively with documented
   rationale.
3. Any developer should be able to build and run the project identically.

## Development Environment

Each language's own toolchain manager provides the development environment. Tool
versions are pinned in files committed to the repository, so every developer and every
CI run uses identical versions without any additional environment layer:

| Language   | Required tool | Pinned by                                                             |
|------------|---------------|-----------------------------------------------------------------------|
| Rust       | rustup        | `rust-toolchain.toml` (exact channel and required components)         |
| Python     | uv            | `.python-version`, `requires-python` and `[tool.uv] required-version` |
| TypeScript | pnpm          | `devEngines` in `package.json` (Node.js and pnpm versions)            |

The language-specific sections below describe each of these files in detail.

[pre-commit](https://pre-commit.com/) must also be installed, by whatever method the
developer prefers. See the [pre-commit installation documentation](https://pre-commit.com/#install).

### Devbox

Use [Devbox](https://www.jetify.com/devbox) only where it provides a clear benefit
that the language toolchains cannot. At present, the only such use is running local
services, such as a development PostgreSQL server via `devbox services`.

When Devbox is used:

- `devbox.json` includes a `$schema` reference and `devbox.lock` is committed.
- Packages are limited to the services and the tools needed to manage them (for
  example, `postgresql` and `sqlx-cli`). Language toolchains are not installed through
  Devbox.
- Service setup steps (initializing and creating databases, for example) are provided
  as Devbox scripts and documented in the README.
- Pre-commit hooks and CI steps invoke tools directly. Only steps that need a running
  service may depend on Devbox.

## Editor Configuration

Include an `.editorconfig` file at the repository root. The standard configuration:

```ini
root = true

[*]
charset = utf-8
end_of_line = lf
indent_style = space
indent_size = 4
insert_final_newline = true
trim_trailing_whitespace = true

[*.{yml,yaml,json,toml}]
indent_size = 2
```

Additional project-specific options can be included if required.

## README

Projects should contain a `README.md` file that gives an overview of the project.

## Licensing

By default, projects should be under an MIT-style license unless another license
is required because of code heritage. Rust projects, if possible, should dual-license
under both MIT and Apache 2.0, written as the SPDX expression `MIT OR Apache-2.0`.

### License Files

Projects follow the [REUSE](https://reuse.software/) specification, so license texts
live in `LICENSES/`, named by SPDX identifier (for example, `LICENSES/MIT.txt` and
`LICENSES/Apache-2.0.txt`).

Projects also keep a copy at the repository root, where GitHub and most readers look
for it. The copy is `LICENSE` for a single license. Multiple licenses are suffixed with
`-<description>` (for example, `LICENSE-MIT` and `LICENSE-APACHE-2.0`). The root copies
must match the texts in `LICENSES/`. More complicated licensing schemes should be
documented in the README.

### File Headers

Every source file starts with SPDX headers, in the file's comment syntax, giving the
license and the copyright holder with the year the file was created:

```rust
// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>
```

Files that cannot carry a comment header, such as JSON files, lock files, images and
generated caches, are annotated in a `REUSE.toml` at the repository root instead.

`reuse lint` checks all of this, in both the pre-commit hooks and CI.

## Version Control

Use `git` as the version control system. Both `.gitattributes` and `.gitignore`
should be present and configured as appropriate for the project.

Additional guidance may be added here at a later date.

### Commit Messages

Commit messages follow [Conventional Commits](https://www.conventionalcommits.org/),
checked by the `conventional-pre-commit` hook (see [Pre-Commit Hooks](#pre-commit-hooks)).
The [Conventional Commits Cheatsheet](https://gist.github.com/qoomon/5dfcdf8eec66a051ecd85625518cfd13)
is a useful reference for choosing types.

The standard template for a commit message:

```txt
<type>(<optional scope>): <subject>

[optional body]

[optional footer]
```

The recognized types are:

- `feat` - adds, changes or removes a feature
- `fix` - fixes one or more bugs
- `perf` - improves performance
- `refactor` - rewrites or restructures code without altering behavior
- `style` - addresses code style without altering behavior
- `test` - adds missing tests or corrects existing ones
- `docs` - adds or updates documentation only
- `build` - affects build-related components
- `ci` - affects CI workflows or other automation configuration
- `revert` - reverts an earlier commit
- `chore` - anything not otherwise mentioned

Strictly speaking, dependency updates could fall under many of these, intentionally
or not. Renovate uses `fix(deps)` for runtime dependencies and `chore(deps)` for
everything else. It may be desirable to revisit that default.

Scopes are inherently project-specific. A good practice for mature projects would
be to identify allowed scopes in developer documentation.

The subject should be in the imperative mood, lowercase (except proper nouns, acronyms,
or otherwise conventionally capitalized words), with no concluding punctuation.
Aim to keep the entire first line within 50 characters, and look for a shorter
wording when it runs past that, but never exceed 72 characters.

A revert uses the `revert` type with the subject of the reverted commit, for example
`revert: add the history panel`. The body names the reverted commit and explains why.

The optional body, if present, should document the motivation for the change and
identify differences from existing behavior. Avoid documenting things that are either
self-explanatory from the code or from comments within the code. Wrap the body at
72 characters.

Commits written with an AI agent carry a `Co-Authored-By` trailer that identifies the
agent. A `Signed-off-by` trailer is not required.

### Branches

`main` is the default branch. A project may also keep a long-lived `dev` integration
branch, in which case CI runs on pushes to both.

Other branch names should follow the pattern `<type>/<short-description>`. Use the
same types as listed above. Branches created by AI agents or bots may use whatever
names those tools choose.

Pull requests are squash-merged, so the pull request title becomes the commit subject.
Every pull request title, whoever opens it, must follow the subject rules above.

## Continuous Integration

Continuous integration is provided by GitHub Actions. Every project has the following
workflows:

| Workflow                        | Triggers                                        | Purpose                                 |
|---------------------------------|-------------------------------------------------|-----------------------------------------|
| `.github/workflows/ci.yaml`     | Pushes to `main` (and `dev`), all pull requests | Every check the pre-commit hooks run    |
| `.github/workflows/audit.yaml`  | Daily schedule, manual dispatch                 | Security advisory audit of dependencies |
| `.github/workflows/zizmor.yaml` | Pushes to `main` (and `dev`), all pull requests | Security analysis of the workflows      |

### Checks

`ci.yaml` runs formatting checks, linting, type checking, tests (with coverage where
the language supports it) and dependency policy checks, such as `cargo deny check bans
licenses sources` and lock file freshness. CI should fail in every case where the
pre-commit hooks would.

Security advisories are checked by `audit.yaml` rather than `ci.yaml`. New advisories
appear independently of code changes, so a daily audit catches them promptly without
blocking unrelated pull requests. The audit tools are:

- Rust: `cargo deny check advisories`.
- Python: `uv audit` (currently experimental in uv).
- TypeScript: `pnpm audit`.

### Structure

Install toolchains with each language's standard setup action (for example,
`astral-sh/setup-uv` or `actions-rust-lang/setup-rust-toolchain`), using the versions
pinned in the repository. Use Devbox only in jobs that need a local service.

Split checks into parallel jobs where feasible. Use an operating system matrix when
platform differences could plausibly cause problems, such as file system behavior or
external tools.

Each workflow sets a `concurrency` group that cancels superseded runs of the same ref.

### Hardening

- Pin every action to a full commit SHA, with the version in a trailing comment
  (`uses: actions/checkout@<sha> # v7.0.1`). Renovate keeps these current.
- Set `permissions: {}` at the top level of every workflow, and grant each job only
  the permissions it needs.
- Set `persist-credentials: false` on every `actions/checkout` step.
- Run [zizmor](https://docs.zizmor.sh/) in both CI and the pre-commit hooks.

## Pre-Commit Hooks

Pre-commit hooks are provided by [pre-commit](https://pre-commit.com/), configured in
`.pre-commit-config.yaml` at the repository root. The configuration sets
`default_install_hook_types` so that a plain `pre-commit install` also installs the
commit message hook:

```yaml
default_install_hook_types: [pre-commit, commit-msg]
```

Hook revisions are pinned to release tags and updated by Renovate.

### Common Hooks

Every project uses these hooks:

- [pre-commit-hooks](https://github.com/pre-commit/pre-commit-hooks):
  `check-added-large-files`, `check-case-conflict`, `check-symlinks`, `check-toml`,
  `check-vcs-permalinks`, `check-yaml`, `detect-private-key`, `end-of-file-fixer`
  and `trailing-whitespace`.
- [reuse](https://github.com/fsfe/reuse-tool): `reuse` (see [Licensing](#licensing)).
- [zizmor](https://github.com/zizmorcore/zizmor-pre-commit): `zizmor`.
- [conventional-pre-commit](https://github.com/compilerla/conventional-pre-commit):
  `conventional-pre-commit` in the `commit-msg` stage, with the recognized commit
  types passed as arguments:

  ```yaml
  - id: conventional-pre-commit
    stages: [commit-msg]
    args: [feat, fix, perf, refactor, style, test, docs, build, ci, revert, chore]
  ```

- A commit message length hook, also in the `commit-msg` stage, which rejects a
  subject line over 72 characters or a body line over 72 characters (lines containing a
  URL are exempt). CI applies the same subject rules to pull request titles.

### Language Hooks

Language hooks are `local` hooks with `language: system` and `pass_filenames: false`.
They invoke the project's tools directly (for example, `cargo clippy` or
`uv run ruff check`), never through Devbox. Each one checks without modifying files,
so the hooks and CI produce the same results. The language sections below list the
required hooks.

## Dependency Management

### Automated Updates

Use [Renovate](https://docs.renovatebot.com/) for automated dependency updates. The
shared [renovate-config](https://github.com/aexoden/renovate-config) preset builds on
`config:best-practices` and provides the automerge, grouping, pinning and release age
policies, so `renovate.json` normally contains nothing else:

```json
{
  "$schema": "https://docs.renovatebot.com/renovate-schema.json",
  "extends": ["github>aexoden/renovate-config"]
}
```

Project-specific rules may be added when a project needs them.

### Version Pinning

Every project commits its lock files, as each tool recommends. Applications also pin
every direct dependency, including development dependencies, to an exact version:

| Ecosystem | Manifest syntax                      | Lock file        |
|-----------|--------------------------------------|------------------|
| Cargo     | `serde = "=1.0.228"`                 | `Cargo.lock`     |
| Python    | `"pydantic==2.12.0"`                 | `uv.lock`        |
| pnpm      | `"react": "19.2.0"` (no `^` or `~`)  | `pnpm-lock.yaml` |

Libraries instead declare compatible version ranges, so that downstream projects can
resolve shared dependencies to a single version. Their lock files are still committed
for reproducible development and CI.

### Release Age

A new release must be at least three days old before a project adopts it. The delay
gives the community time to catch and yank compromised or broken releases before they
reach a project. It is enforced at two levels:

1. **Renovate**, through `minimumReleaseAge` in the shared preset.
2. **The package manager**, which covers what Renovate cannot age-check, such as lock
   file maintenance and manual updates:
   - uv: `exclude-newer = "3 days"` under `[tool.uv]` in `pyproject.toml`.
   - pnpm: `minimumReleaseAge: 4320` (in minutes) in `pnpm-workspace.yaml`.
   - Cargo: `global-min-publish-age = "3 days"` under `[registry]` in
     `.cargo/config.toml`. This is stable as of Rust 1.100; earlier toolchains silently
     ignore it, so it takes effect when the pinned toolchain reaches 1.100.

#### Urgent Updates

Security fixes may bypass the release age requirement. Renovate's vulnerability alert
PRs are exempt from `minimumReleaseAge` in the shared preset. To update a single package
early through the package manager, add a temporary per-package exemption, with a
comment naming the advisory:

- uv: `exclude-newer-package = { package-name = false }` under `[tool.uv]`.
- pnpm: `minimumReleaseAgeExclude: [package-name]` in `pnpm-workspace.yaml`.
- Cargo: there is no per-package setting. Run
  `CARGO_RESOLVER_INCOMPATIBLE_PUBLISH_AGE=allow cargo update -p package-name`, and the
  lock file keeps the newer version.

Remove a configured exemption once the release has passed the normal release age.

## Language-Specific Standards

### Python

Use [uv](https://github.com/astral-sh/uv) for all Python projects. The basic project
structure should look like the following:

```txt
project/
├── src/
│   └── package_name/
│       ├── __init__.py
│       └── ...
├── tests/
│   ├── __init__.py
│   └── ...
├── pyproject.toml
├── uv.lock
└── README.md
```

For linting and formatting, use [Ruff](https://docs.astral.sh/ruff/). For type
checking, use [mypy](https://mypy.readthedocs.io/en/stable/) in strict mode.

Eventually, standards for testing will likely be added.
