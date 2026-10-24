# Project Standards

This file documents the standards intended to be used by my projects. Some of these
standards are enforced by the `norms` tool. These standards are highly opinionated
and may not be appropriate for all projects. In developing these standards, there
were several guiding principles:

1. Humans shouldn't manually enforce things that tools can check.
2. Apply maximum strictness by default, only relaxing selectively with documented
   rationale.
3. Any developer should be able to build and run the project identically.

When a project has good reason to depart from a standard, it records the exception,
with its rationale, in its norms configuration file. `norms verify` then skips the
corresponding check instead of reporting it.

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

## Documentation

### README

Projects should contain a `README.md` file that gives an overview of the project.

### Changelog

Projects that publish versioned releases keep a hand-written `CHANGELOG.md` in the
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) format and follow
[Semantic Versioning](https://semver.org/). Releases are tagged `vX.Y.Z`. Projects
that are deployed directly from `main` without versioned releases do not need a
changelog.

### Agent Guidance

Projects may include an `AGENTS.md` describing the project's commands and conventions
for AI coding agents. Use `CLAUDE.md` only for instructions specific to Claude, and
have it import `AGENTS.md` rather than repeat it.

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

| Ecosystem | Manifest syntax                     | Lock file        |
|-----------|-------------------------------------|------------------|
| Cargo     | `serde = "=1.0.228"`                | `Cargo.lock`     |
| Python    | `"pydantic==2.12.0"`                | `uv.lock`        |
| pnpm      | `"react": "19.2.0"` (no `^` or `~`) | `pnpm-lock.yaml` |

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

#### Project Layout

Use [uv](https://docs.astral.sh/uv/) for all Python projects, with a `src/` layout:

```txt
project/
├── src/
│   └── package_name/
│       ├── __init__.py
│       ├── __about__.py
│       └── ...
├── tests/
│   ├── __init__.py
│   └── ...
├── .git_archival.txt
├── .python-version
├── pyproject.toml
├── uv.lock
└── README.md
```

#### Python and uv Versions

- `.python-version` names the exact Python version used for development and CI.
- Applications set `requires-python` to that same exact version (for example,
  `==3.15.0`). Libraries set the range of versions they support.
- The `Programming Language :: Python :: 3.x` classifiers match `requires-python`.
- `[tool.uv] required-version` sets the minimum uv version the project needs, and the
  CI setup step pins the exact uv version.

#### Dependencies

Development tools go in the `dev` dependency group, not in optional dependencies:

```toml
[dependency-groups]
dev = ["mypy==...", "pytest==...", "pytest-cov==...", "ruff==..."]
```

#### Versioning

The package version comes from git tags through [hatch-vcs](https://github.com/ofek/hatch-vcs):

```toml
[project]
dynamic = ["version"]

[build-system]
requires = ["hatchling", "hatch-vcs"]
build-backend = "hatchling.build"

[tool.hatch.version]
source = "vcs"

[tool.uv]
# Reinstall the project when HEAD or tags move, so the reported version stays current.
cache-keys = [{ file = "pyproject.toml" }, { git = { commit = true, tags = true } }]
```

`.git_archival.txt`, marked `export-subst` in `.gitattributes`, lets source archives
without git history report their version. Code reads the version at runtime with
`importlib.metadata.version()`, in `__about__.py`.

#### Formatting and Linting

Use [Ruff](https://docs.astral.sh/ruff/) for both. The baseline configuration enables
every rule, including preview rules:

```toml
[tool.ruff]
line-length = 120

[tool.ruff.format]
line-ending = "lf"

[tool.ruff.lint]
preview = true
select = ["ALL"]
ignore = ["missing-trailing-comma", "single-line-implicit-string-concatenation"]

[tool.ruff.lint.flake8-copyright]
notice-rgx = "(?i)SPDX-FileCopyrightText: \\d{4}"

[tool.ruff.lint.isort]
lines-between-types = 1

[tool.ruff.lint.per-file-ignores]
"tests/*.py" = ["assert", "private-member-access"]

[tool.ruff.lint.pydocstyle]
convention = "google"
```

The two ignored rules conflict with the Ruff formatter. Any further ignore needs a
comment in `pyproject.toml` explaining why.

#### Type Checking

Use [mypy](https://mypy.readthedocs.io/en/stable/) in strict mode:

```toml
[tool.mypy]
strict = true
```

#### Testing

Use [pytest](https://docs.pytest.org/) with [pytest-cov](https://pytest-cov.readthedocs.io/),
reporting missing lines: `uv run pytest --cov=src/package_name --cov-report=term-missing`.

#### Hooks and CI

The pre-commit hooks and CI both run:

| Hook ID       | Command                                                          |
|---------------|------------------------------------------------------------------|
| `uv-lock`     | `uv lock --check`                                                |
| `ruff-format` | `uv run ruff format --check`                                     |
| `ruff-check`  | `uv run ruff check`                                              |
| `mypy`        | `uv run mypy .`                                                  |
| `test`        | `uv run pytest --cov=src/package_name --cov-report=term-missing` |

CI installs uv with `astral-sh/setup-uv` and runs `uv sync --locked` before the checks.

### Rust

#### Toolchain

`rust-toolchain.toml` pins an exact channel and lists the components the hooks need,
so `rustup` installs everything on first use:

```toml
[toolchain]
channel = "1.99.1"
components = ["clippy", "rustfmt"]
```

Libraries also set `rust-version` (the MSRV) in `Cargo.toml`. Applications do not need
one, since the toolchain file already pins their compiler.

`.cargo/config.toml` sets the release age (see [Release Age](#release-age)):

```toml
[registry]
global-min-publish-age = "3 days"
```

#### Package Metadata

`Cargo.toml` sets `description`, `repository`, `license`, `edition` (the latest
edition), `keywords` and `categories`.

#### Lints

The baseline enables the `pedantic`, `nursery` and `cargo` Clippy groups, plus the
restriction lints against `unwrap` and `expect`:

```toml
[lints.clippy]
cargo = { level = "warn", priority = -1 }
expect_used = "warn"
multiple_crate_versions = "allow"
nursery = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
unwrap_used = "warn"

[lints.rust]
future_incompatible = { level = "warn", priority = -1 }
let_underscore = { level = "warn", priority = -1 }
```

`multiple_crate_versions` is allowed because transitive dependencies routinely pull in
several versions of the same crate, which a project cannot fix.

`clippy.toml` allows `unwrap`, `expect` and `panic` in tests:

```toml
allow-expect-in-tests = true
allow-panic-in-tests = true
allow-unwrap-in-tests = true
```

Projects may enable further restriction lints. A standard set has not been settled
yet.

The hooks and CI pass `-D warnings`, so any warning fails the check.

#### Dependency Policy

`deny.toml` configures [cargo-deny](https://embarkstudios.github.io/cargo-deny/):

- `[licenses] allow` lists only permissive licenses that the project actually uses.
- `[bans] multiple-versions = "allow"`, for the same reason as the Clippy lint above.
- Every entry in `[advisories] ignore` has a `reason`.

cargo-deny is installed separately by the developer. See the
[cargo-deny installation documentation](https://embarkstudios.github.io/cargo-deny/#install-cargo-deny).

#### Hooks and CI

The pre-commit hooks and CI both run:

| Hook ID      | Command                                                                |
|--------------|------------------------------------------------------------------------|
| `rustfmt`    | `cargo fmt --all --check`                                              |
| `clippy`     | `cargo clippy --workspace --all-targets --all-features -- -D warnings` |
| `test`       | `cargo test --workspace --all-features`                                |
| `cargo-deny` | `cargo deny check bans licenses sources`                               |

CI installs the toolchain with `actions-rust-lang/setup-rust-toolchain`, which reads
`rust-toolchain.toml`, and caches builds with `Swatinem/rust-cache`. It runs cargo-deny
with `EmbarkStudios/cargo-deny-action`. The advisory check runs in `audit.yaml` (see
[Continuous Integration](#continuous-integration)).

### TypeScript

#### Location

A TypeScript project lives at the repository root when it is the primary language.
Otherwise it lives in a named subdirectory, such as `frontend/`, which is declared in
the norms configuration. Paths below are relative to that directory.

#### Package Configuration

`package.json` sets `"type": "module"` and `"private": true` (for applications), and
pins Node.js and pnpm through `devEngines`. pnpm downloads the declared versions
automatically:

```json
"devEngines": {
  "runtime": { "name": "node", "version": "25.9.0", "onFail": "download" },
  "packageManager": { "name": "pnpm", "version": "12.7.0", "onFail": "download" }
}
```

It defines these scripts:

| Script      | Command                |
|-------------|------------------------|
| `build`     | `tsc -b && vite build` |
| `format`    | `prettier --check .`   |
| `lint`      | `eslint .`             |
| `test`      | `vitest run`           |
| `typecheck` | `tsc -b --noEmit`      |

`pnpm-workspace.yaml` sets the release age, and lists explicitly the dependencies
allowed to run build scripts:

```yaml
allowBuilds:
  esbuild: true
minimumReleaseAge: 4320
```

#### Compiler Options

`tsconfig.json` is solution-style, referencing a config for the application
(`tsconfig.app.json`) and one for tooling files such as `vite.config.ts`
(`tsconfig.node.json`). Each referenced config enables at least:

```json
"strict": true,
"noUncheckedIndexedAccess": true,
"noUncheckedSideEffectImports": true,
"noFallthroughCasesInSwitch": true,
"noUnusedLocals": true,
"noUnusedParameters": true,
"isolatedModules": true,
"verbatimModuleSyntax": true,
"moduleDetection": "force"
```

#### Formatting and Linting

Use [Prettier](https://prettier.io/) for formatting, configured in `.prettierrc` with
`printWidth: 120`, along with a `.prettierignore`.

Use [ESLint](https://eslint.org/) for linting, configured in `eslint.config.ts`. The
configuration includes the `strictTypeChecked` and `stylisticTypeChecked` configs from
[typescript-eslint](https://typescript-eslint.io/), with `projectService` enabled for
type-aware rules, and `eslint-config-prettier` to disable rules that conflict with
Prettier.

#### Testing

Use [Vitest](https://vitest.dev/).

#### Hooks and CI

The pre-commit hooks and CI both run the following, prefixed with
`pnpm --dir <path>` when the project lives in a subdirectory:

| Hook ID                  | Command                               |
|--------------------------|---------------------------------------|
| `pnpm-lock`              | `pnpm install --frozen-lockfile`      |
| `pnpm-peer-dependencies` | `pnpm peers check`                    |
| `prettier`               | `pnpm exec prettier --check .`        |
| `eslint`                 | `pnpm exec eslint . --max-warnings 0` |
| `tsc`                    | `pnpm exec tsc -b --noEmit`           |
| `test`                   | `pnpm exec vitest run`                |
