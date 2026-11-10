# Project Standards

This file documents the standards intended to be used by my projects. Some of
these standards are enforced by the `norms` tool. These standards are highly
opinionated and may not be appropriate for all projects. In developing these
standards, there were several guiding principles:

1. Humans shouldn't manually enforce things that tools can check.
2. Apply maximum strictness by default, only relaxing selectively with
   documented rationale.
3. Any developer should be able to build and run the project identically.

When a project has good reason to depart from a standard, it records the
exception, with its rationale, in its
[norms configuration](#norms-configuration). `norms verify` then reports the
corresponding check as skipped, with the rationale, instead of failing it.

## Project Template

New projects start from the [Copier](https://copier.readthedocs.io/) template in
norms, which renders the files these standards describe for the chosen
languages:

```sh
uvx copier copy --trust gh:aexoden/norms <path>
```

The template asks for the primary language, which lives at the repository root,
any secondary languages and their subdirectories, and whether the project is an
application or a library. Copying needs `--trust` because the template runs
each language's tool to generate any missing lock files.

Existing projects stay current with `uvx copier update --trust`, which merges
the changes between the template version recorded in `.copier-answers.yml` and
the latest one. Template versions are norms release tags (`vX.Y.Z`).

## norms Configuration

A project that departs from a standard records the exception in `norms.toml`, at
the repository root. Most projects need no exceptions, and so no `norms.toml`.

```toml
[components]
exclude = ["tests/fixtures"]

[[waivers]]
check = "gitattributes-binary"
reason = "The repository holds no binary files."
```

- `norms verify` finds the project's components by their marker files:
  `Cargo.toml`, `pyproject.toml`, `package.json` and `CMakeLists.txt`, at the
  root or in any subdirectory. It checks each component's own files in its
  directory, and the repository-wide files, such as the workflows, once. A
  marker inside a component of the same language, such as a Cargo workspace
  member, belongs to that component; components of different languages may
  nest. The search skips hidden directories, whatever the project's
  `.gitignore` files exclude, `node_modules`, `target` and `.venv`, and doesn't
  follow symlinked directories or symlinked markers that lead outside the
  project. A directory it can't read is reported as a warning.
- Some standards differ between applications and libraries. norms treats a Rust
  component as an application if it builds a binary (`src/main.rs` or
  `src/bin/`, unless `autobins = false`, or a `[[bin]]` target), even alongside
  a library, and a Python component as an application if it installs a command
  (a non-empty `[project.scripts]` or `[project.gui-scripts]`). Other components
  of these languages are libraries. Every TypeScript component is an
  application. A Rust component that is a Cargo workspace is an application if
  any of its members builds a binary, and then every member pins its
  dependencies exactly.
- norms finds a workspace's members as Cargo does: the directories that
  `[workspace] members` names or matches, and the packages inside the workspace
  that the root package or a member depends on by path, less those `exclude`
  lists. A member without a valid `Cargo.toml` fails; one outside the
  workspace's directory, or a pattern norms can't expand, is reported as a
  warning. Each member's package metadata and Clippy lints are checked like
  any package's, with what it inherits from the workspace resolved. A virtual
  manifest, which has no `[package]`, isn't checked for them itself, so one
  with no members is reported as a warning.
- `[components] exclude` lists directories, relative to the root, that hold
  marker files but no components, such as test fixtures. norms doesn't search
  them. An entry must be a plain relative path, without `.` or `..`; one that
  names no directory is reported as a warning.
- Each `[[waivers]]` entry names a check by its ID and gives the reason for the
  exception. A waived check is reported as skipped, with the reason, instead of
  failing or warning; a check that inspects several files or components is
  waived for all of them. A waiver whose check passes or doesn't run is
  reported as a warning, so waivers don't outlive their need. The checks of
  `norms.toml` itself can't be waived; a waiver naming one fails.
- Every waiver needs a reason, and a check may be waived only once.
- Check IDs are stable, kebab-case names of the subject and then the property
  checked, such as `cargo-toml-license`. `norms verify` prints each one before
  the check's description, which may change between versions while the ID
  doesn't.
- Unknown keys are errors.

`norms verify <path>` reports each check as passed, failed, warned or skipped,
and exits with failure if any check failed. With `--deny-warnings`, it also
exits with failure if any check warned.

A check fails when a safeguard these standards require is absent or unusable: a
required file is missing, a configuration can't be parsed, a tool isn't
configured or run at all (no hook, CI step or linter configuration), or it is
set up so that it can't run or passes what it should fail, such as a gate job
that a failed job doesn't fail. What a tool already catches by failing a hook or
CI job with an error that names the problem, such as a configuration that loads
a package missing from the dependencies, is left to it rather than checked. A
check warns when the safeguard is present but differs from the standard (weaker
settings, other flags, versions, pins, names or metadata), when norms can't be
sure of the issue, or when configuration is stale, such as an unneeded waiver. A
malformed `norms.toml` fails. Three safeguards are an exception to warning when
norms can't be sure: the gate job's failing step, and the
`actions-rust-lang/setup-rust-toolchain` `toolchain` and `astral-sh/setup-uv`
`python-version` inputs of the jobs testing a library's oldest supported Rust
and Python. norms can't tell one written another way from one that doesn't work,
so written another way, it fails as if absent.

## Development Environment

Each language's own toolchain manager provides the development environment. Tool
versions are pinned in files committed to the repository, so every developer and
every CI run uses identical versions without any additional environment layer:

| Language   | Required tool | Pinned by                                                             |
|------------|---------------|-----------------------------------------------------------------------|
| Rust       | rustup        | `rust-toolchain.toml` (exact channel and required components)         |
| Python     | uv            | `.python-version`, `requires-python` and `[tool.uv] required-version` |
| TypeScript | pnpm          | `devEngines` in `package.json` (Node.js and pnpm versions)            |

The language-specific sections below describe each of these files in detail.

[pre-commit](https://pre-commit.com/) must also be installed, by whatever method
the developer prefers. See the [pre-commit installation
documentation](https://pre-commit.com/#install).

### Devbox

Use [Devbox](https://www.jetify.com/devbox) only where it provides a clear
benefit that the language toolchains cannot. At present, the only such use is
running local services, such as a development PostgreSQL server via `devbox
services`.

When Devbox is used:

- `devbox.json` includes a `$schema` reference and `devbox.lock` is committed.
- Packages are limited to the services and the tools needed to manage them (for
  example, `postgresql` and `sqlx-cli`). Language toolchains and their managers
  are not installed through Devbox, nor is Corepack enabled with
  `DEVBOX_COREPACK_ENABLED`.
- Service setup steps (initializing and creating databases, for example) are
  provided as Devbox scripts and documented in the README.
- Pre-commit hooks and CI steps invoke tools directly. Only steps that need a
  running service may depend on Devbox.

## Editor Configuration

Include an `.editorconfig` file at the repository root. The standard
configuration:

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

A project with TypeScript also indents Prettier's `.prettierrc` by two spaces,
since Prettier reads `.editorconfig` and `.prettierrc` has no extension:

```ini
[.prettierrc]
indent_size = 2
```

Additional project-specific options can be included if required.

`norms verify` reads the file as editors do, and checks the properties it gives
every file, YAML, JSON and TOML files, and, with TypeScript, `.prettierrc`,
whichever sections set them, so other sections may change them for other
files.

## Documentation

### README

Projects should contain a `README.md` file that gives an overview of the
project.

### Changelog

Projects that publish versioned releases keep a hand-written `CHANGELOG.md` in
the [Keep a Changelog](https://keepachangelog.com/en/2.0.0/) format and follow
[Semantic Versioning](https://semver.org/). Releases are tagged `vX.Y.Z`.
Projects that are deployed directly from `main` without versioned releases do
not need a changelog.

An `[Unreleased]` section at the top collects notable changes as they land, so
that anyone who comes across the repository long after a release can see what
has changed since. A release renames it to the new version, and a new, empty
`[Unreleased]` section takes its place. The link definitions at the bottom of
the file change with it: each version links to a comparison with the one before
it, or the first to its tag, and `[Unreleased]` to a comparison of the latest
tag with `HEAD` (before the first release, to the commits on `main`, as the
template writes it). Before a release, go through the commits since the previous
one for notable changes the section missed. The changelog is written for its
readers, not as a list of commits: a change that was made and then reverted
before a release isn't mentioned, and several changes that together make one
difference are one entry.

An entry for a breaking change starts with `**Breaking:**`, under the type of
change it is, and says briefly how to upgrade. The README should say what the
version numbers cover, such as a command line, a library's API or a file
format, so that readers know what a breaking change breaks.

`norms verify` expects a changelog in a project that has a Rust or Python
library component, since libraries are published, or a release tag (`vX.Y.Z`,
which it reads with `git` when the project is at the top of its repository),
and doesn't require one elsewhere. A library that is never published doesn't
count: a Rust one whose packages all set `publish = false` (or inherit it from
`[workspace.package]`), or a Python one with the `Private :: Do Not Upload`
classifier. A shallow clone, such as CI's default checkout, has no tags, so
there only the libraries count.

### Agent Guidance

Projects may include an `AGENTS.md` describing the project's commands and
conventions for AI coding agents. Use `CLAUDE.md` only for instructions specific
to Claude, and have it import `AGENTS.md` rather than repeat it, with
`@AGENTS.md` at the start of a line of its own. `CLAUDE.md` and `AGENTS.md` are
files of their own, not one a symlink to the other, so that `CLAUDE.md` has room
for those instructions.

### Markdown

Markdown files are checked by
[markdownlint-cli2](https://github.com/DavidAnson/markdownlint-cli2), in both
the pre-commit hooks and CI. The standard `.markdownlint-cli2.jsonc` enables
every default rule, requires dashes for unordered lists, wraps prose at 80
columns and allows repeated headings under different parents:

```jsonc
{
    "config": {
        "default": true,

        // MD004 - Unordered list style
        "MD004": {
            "style": "dash"
        },

        // MD013 - Line length
        "MD013": {
            "line_length": 80,
            "tables": false,
            "code_blocks": false
        },

        // MD024 - No duplicate headings
        "MD024": {
            "siblings_only": true
        }
    }
}
```

Tables and code blocks are exempt from the line length, since wrapping them
would break them. Headings aren't, and keep the default limit, the same 80
columns.

CI runs `DavidAnson/markdownlint-cli2-action` with `globs: '**/*.md'`, since the
action otherwise checks only the files at the repository root.

`norms verify` reads the `config` object as markdownlint does, following rule
aliases and tags and the order of keys, and checks that every rule runs as an
error and that the options above hold. A config that `extends` another file or
sets `overrides` gets a warning instead, since norms doesn't read them, and so
does a project with any other markdownlint configuration file outside the
directories `ignores` matches: a `.markdownlint.*` file beside it would take the
place of its `config`, and one in a subdirectory changes the rules there.

## Licensing

By default, projects should be under an MIT-style license unless another license
is required because of code heritage. Rust projects, if possible, should
dual-license under both MIT and Apache 2.0, written as the SPDX expression `MIT
OR Apache-2.0`.

### License Files

Projects follow the [REUSE](https://reuse.software/) specification, so license
texts live in `LICENSES/`, named by SPDX identifier (for example,
`LICENSES/MIT.txt` and `LICENSES/Apache-2.0.txt`).

Projects also keep a copy at the repository root, where GitHub and most readers
look for it. The copy is `LICENSE` for a single license. Multiple licenses are
suffixed with `-<description>` (for example, `LICENSE-MIT` and
`LICENSE-APACHE-2.0`). Each text in `LICENSES/` has a root copy identical to it,
byte for byte, and each root copy is one of those texts. More complicated
licensing schemes should be documented in the README.

Each manifest's license (`license` in `Cargo.toml`, `pyproject.toml` and
`package.json`) is an SPDX expression, and `LICENSES/` holds a text for every
license and exception it names. In `pyproject.toml` it is a string, as PEP 639
requires, not the older table form.

### File Headers

Every source file starts with SPDX headers, in the file's comment syntax, giving
the license and the copyright holder with the year the file was created:

<!-- REUSE-IgnoreStart -->
```rust
// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>
```
<!-- REUSE-IgnoreEnd -->

Files that cannot carry a comment header, such as JSON files, lock files, images
and generated caches, are annotated in a `REUSE.toml` at the repository root
instead.

`reuse lint` checks all of this, in both the pre-commit hooks and CI, where it
runs through `fsfe/reuse-action`.

## Version Control

Use `git` as the version control system. Both `.gitattributes` and `.gitignore`
should be present and configured as appropriate for the project.

`.gitattributes` normalizes line endings, marks binary formats, and enables
git's built-in diff drivers for each language in the project:

```txt
# Auto-detect text files and normalize line endings to LF
*       text=auto eol=lf

# Binary files
*.gif   binary
*.ico   binary
*.jpg   binary
*.jpeg  binary
*.png   binary
*.webp  binary
*.woff  binary
*.woff2 binary

# Archives
*.bz2   binary
*.gz    binary
*.tar   binary
*.zip   binary

# Python
*.py    text diff=python
*.pyi   text diff=python

# Rust
*.rs    text diff=rust
```

Include only the language blocks that apply. Python projects also mark
`.git_archival.txt` as `export-subst` (see [Versioning](#versioning)).

### Commit Messages

Commit messages follow [Conventional
Commits](https://www.conventionalcommits.org/), checked by the
`conventional-pre-commit` hook (see [Pre-Commit Hooks](#pre-commit-hooks)). The
[Conventional Commit Types
Cheatsheet](https://www.bavaga.com/blog/2025/01/27/my-ultimate-conventional-commit-types-cheatsheet/)
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

Strictly speaking, dependency updates could fall under many of these,
intentionally or not. Renovate uses `fix(deps)` for runtime dependencies and
`chore(deps)` for everything else. It may be desirable to revisit that default.

Scopes are inherently project-specific. A good practice for mature projects
would be to identify allowed scopes in developer documentation.

The subject should be in the imperative mood, lowercase (except proper nouns,
acronyms, or otherwise conventionally capitalized words), with no concluding
punctuation. Aim to keep the entire first line within 50 characters, and look
for a shorter wording when it runs past that, but never exceed 72 characters.

A revert uses the `revert` type with the subject of the reverted commit, for
example `revert: add the history panel`. The body names the reverted commit and
explains why.

The optional body, if present, should document the motivation for the change and
identify differences from existing behavior. Avoid documenting things that are
either self-explanatory from the code or from comments within the code. Wrap the
body at 72 characters.

Commits written with an AI agent carry a `Co-Authored-By` trailer that
identifies the agent. A `Signed-off-by` trailer is not required.

### Branches

`main` is the default branch. A solo project may also keep a long-lived `dev`
branch, so that commits can be tested against CI without the friction of a pull
request. Commits are pushed directly to `dev`, and once CI passes on them,
`main` is fast-forwarded to `dev` (for example, `git push origin dev:main`).
Work may still go through a pull request when that is useful. A project with
multiple contributors documents its own use of `dev`, if it keeps one. CI runs
on pushes to both branches.

Other branch names should follow the pattern `<type>/<short-description>`. Use
the same types as listed above. Branches created by AI agents or bots may use
whatever names those tools choose.

Pull requests are squash-merged, so the pull request title becomes the commit
subject. The repository's merge settings allow only squash merging, with the
default commit message set to "Pull request title and commit details"
(`squash_merge_commit_title: PR_TITLE` and
`squash_merge_commit_message: COMMIT_MESSAGES` in the API). GitHub's own default
takes the subject of a pull request with a single commit from that commit
rather than the title, so the title check below wouldn't check the subject
that is merged. Every pull request title, whoever opens it, must follow the
subject rules above.

A `pr-title.yaml` workflow checks each title by running the `commit-msg` hooks
on the subject the squash merge would create, which is the title followed by
its number, as in `(#123)`, so the number takes part of the 72 characters. It
first rejects a title starting with `fixup!`, `squash!`, `amend!` or `Merge`,
which the hooks let through in local commits but a squash merge keeps. It runs
when a pull request is opened or reopened and when its title is edited: its
job is skipped for other edits, which also get a concurrency group of their own
so that they don't cancel a check in progress. It's separate from `ci.yaml`,
so editing a pull request doesn't rerun CI, and advisory: the
[gate job](#gate-job) doesn't depend on it, and
[branch protection](#branch-protection) doesn't require it.

`norms verify` fails a project without `pr-title.yaml`, or whose
`pr-title.yaml` doesn't run on pull requests or doesn't run `pre-commit run
--hook-stage commit-msg`, and warns when its `pull_request` types leave out
`opened`, `reopened` or `edited`.

### Branch Protection

A repository ruleset named `Default` protects the default branch. It is active,
has no bypass actors, and enables these rules:

| Rule                          | Settings                                              |
|-------------------------------|-------------------------------------------------------|
| Restrict deletions            |                                                       |
| Block force pushes            |                                                       |
| Require linear history        |                                                       |
| Require signed commits        |                                                       |
| Require status checks to pass | `CI success` from GitHub Actions; branches up to date |

The one required check is the CI gate job (see [Gate Job](#gate-job)). Selecting
GitHub Actions as its source means a commit status of the same name from
anywhere else cannot satisfy it. Requiring branches to be up to date means the
tree CI tested is the tree that is merged. Checks that do not report on every
commit are never required, since a required check that never reports blocks the
merge. This excludes `audit.yaml`, which runs on a schedule, `pr-title.yaml`,
which runs when a pull request's title changes, and Renovate's commit statuses
such as `renovate/artifacts`, which appear only on some Renovate branches.
Renovate waits for those itself, because the shared preset disables platform
automerge.

The ruleset does not require a pull request, so that `main` can be
fast-forwarded to `dev`. The required status check still applies to a direct
push: GitHub accepts it only if the commit being pushed has already passed,
which it has if CI passed on `dev`. An untested commit can reach `main` by
neither route.

Commits pushed directly must be signed locally. Commits that GitHub creates,
such as squash merges and Renovate's commits, are signed by GitHub.

The `dev` branch is not protected. It is a working branch, and rewriting it
before `main` catches up is harmless.

Rulesets on private repositories require GitHub Pro. A private repository
without it follows the same workflow by convention, without enforcement.

## Continuous Integration

Continuous integration is provided by GitHub Actions. Every project has the
following workflows:

| Workflow                          | Triggers                                        | Purpose                                         |
|-----------------------------------|-------------------------------------------------|-------------------------------------------------|
| `.github/workflows/ci.yaml`       | Pushes to `main` (and `dev`), all pull requests | Every check the pre-commit hooks run            |
| `.github/workflows/audit.yaml`    | Daily schedule, manual dispatch                 | Security advisory audit of dependencies         |
| `.github/workflows/pr-title.yaml` | Pull requests opened, reopened or retitled      | Commit message checks on pull request titles    |

### Checks

`ci.yaml` runs formatting checks, linting, type checking, tests (with coverage
for Python and TypeScript) and dependency policy checks, such as `cargo
deny --workspace --all-features check -D warnings bans licenses sources` and
lock file freshness. CI should fail in every case where the pre-commit hooks
would.

Where this standard names an action for a check, such as
`zizmorcore/zizmor-action` or `EmbarkStudios/cargo-deny-action`, CI uses that
action rather than running the tool itself. `norms verify` warns on a step that
runs the tool instead, without checking its arguments.

Most hooks have a job of their own. A `pre-commit` job runs the rest, such as
the [common hooks](#common-hooks) from pre-commit-hooks, with `pre-commit run
--all-files --show-diff-on-failure` at a pinned pre-commit version, installed
through `astral-sh/setup-uv` and run with `uvx`. Its `SKIP` lists every hook
that has its own job, so a hook added later runs in this job until it gets one.
A hook's job has the hook's ID, or for the `markdownlint-cli2` and
`tombi-format` hooks, is the `markdownlint` or `tombi` job.
The `commit-msg` hooks don't run in it, but in the `pr-title.yaml` workflow on
pull request titles (see [Branches](#branches)).

The `norms-verify` hook's job runs it as `pre-commit run norms-verify
--all-files`, so that pre-commit builds the norms that the hook's `rev` pins.
Its checkout fetches the whole history (`fetch-depth: 0`), since without
`CHANGELOG.md`, `norms verify` looks for release tags (see
[Changelog](#changelog)). `norms verify` fails a `ci.yaml` in which no job runs
it, as the hook or itself, and warns when that job's checkout lacks
`fetch-depth: 0`, when it runs without `--deny-warnings`, or when the hook runs
only in the `pre-commit` job.

Libraries also check the oldest versions they support, which the hooks don't,
since that needs a second toolchain: Rust libraries in an `msrv` job and Python
libraries in a `pytest-min-python` job (see the language sections).

Security advisories are checked by `audit.yaml` rather than `ci.yaml`. New
advisories appear independently of code changes, so a daily audit catches them
promptly without blocking unrelated pull requests. The audit tools are:

- Rust: `cargo deny --workspace --all-features check -D warnings advisories`.
- Python: `uv audit` (currently experimental in uv).
- TypeScript: `pnpm audit`.

`audit.yaml` also runs on `workflow_dispatch`, so that the audit can be rerun by
hand once a fix lands. `norms verify` fails an `audit.yaml` without a schedule,
and warns when no `cron` entry runs every day, or when it can't be dispatched.

### zizmor

`ci.yaml` runs [zizmor](https://docs.zizmor.sh/) as a job, using
`zizmorcore/zizmor-action` with `advanced-security: false` and `annotations:
true`. In that mode the action fails the job on any finding and annotates the
offending lines, so a finding shows up on the CI run like any other failure. CI
also runs zizmor's online audits, which the pre-commit hook skips when no GitHub
token is available.

Both run zizmor's pedantic persona, the action with `persona: pedantic` and the
hook with `--pedantic`, so they report the same findings.

The action's default GitHub Advanced Security mode is not used. It uploads
findings to code scanning and passes the job, relying on a code scanning rule in
the ruleset to block the merge. Findings on commits pushed straight to `dev`
then appear only in the code scanning alert list for that branch, and every
finding must be fixed before merge regardless of severity, so the alert triage
it offers adds little. It is also specific to GitHub, and it is the only reason
zizmor would need more than `contents: read`: its upload needs
`security-events: write`, and in a private repository `actions: read`.

Repositories still using a separate `zizmor.yaml` with code scanning switch to
the `ci.yaml` job, then remove the workflow and the ruleset's **Require code
scanning results** rule together, since that rule would otherwise wait for
results that never arrive.

### Structure

Install toolchains with each language's standard setup action (for example,
`astral-sh/setup-uv` or `actions-rust-lang/setup-rust-toolchain`), using the
versions pinned in the repository. An `astral-sh/setup-uv` step in a job that
runs `uv` itself, such as `uv sync` or `uv audit`, sets `version` to an exact uv
version: without it, the action installs the newest uv that `required-version`
allows. A job that only runs tools through `uvx`, such as the `pre-commit` job,
doesn't install the project, so its uv needn't be pinned, as long as every
hook that runs `uv` has a job of its own and so is in its `SKIP`. Use Devbox
only in jobs that need a local service.

Split checks into parallel jobs where feasible. Use an operating system matrix
when platform differences could plausibly cause problems, such as file system
behavior or external tools.

Jobs that install nothing and run only one JavaScript action or shell step,
namely the gate job, markdownlint and zizmor, run on `ubuntu-slim`, which
starts faster, as does the Tombi job, whose setup action only downloads a
prebuilt binary. It runs jobs in a container without Docker, so Docker-based
actions such as `fsfe/reuse-action` and `EmbarkStudios/cargo-deny-action` can't
run there. The other jobs, which install a toolchain or tools, stay on
`ubuntu-latest`.

Each workflow sets a `concurrency` group that cancels superseded runs of the
same ref, with `cancel-in-progress: true` rather than an expression that cancels
only some of them.

Every job sets `timeout-minutes`, so a hung step fails within minutes rather
than at GitHub's six-hour default: 30 for jobs that compile, type check or run
tests, such as Clippy, mypy, ESLint and the test suites, and 10 for the rest. A
project whose jobs legitimately take longer raises the value for those jobs.
Jobs on `ubuntu-slim` can't exceed its own 15-minute limit.

`ci.yaml` runs on pushes to `main` and `dev`, naming both in
`push: branches: [dev, main]` rather than matching them with patterns, whether
or not the project keeps a `dev` branch (see [Branches](#branches)). A `push`
without a branch filter, which runs on every branch, also does.

`ci.yaml` never filters `pull_request` by `paths` or `branches` (or their
`-ignore` forms), nor limits its `types` to fewer than the default `opened`,
`synchronize` and `reopened`. The ruleset waits for its result, and a workflow
that does not run leaves the pull request blocked.

### Gate Job

`ci.yaml` ends with a gate job, `ci-success`, that succeeds only if every other
job in the workflow succeeded. Branch protection requires this single check
rather than each job, so jobs can be added, renamed, split or given new matrix
entries without changing repository settings:

```yaml
ci-success:
  name: CI success
  if: always()
  needs: [lint, test]
  runs-on: ubuntu-slim
  timeout-minutes: 10
  steps:
    - name: Fail unless every job succeeded
      if: contains(needs.*.result, 'failure') || contains(needs.*.result, 'cancelled') || contains(needs.*.result, 'skipped')
      run: exit 1
```

- `needs` lists every other job in the workflow. `needs.*.result` covers only
  direct dependencies, so a job reached only through another job goes unchecked.
- `if: always()` is essential. Without it, the gate is skipped when a job fails,
  and a skipped job satisfies a required check.
- Any result other than success fails the gate, including a skipped job.
- No job in the workflow, the gate included, sets `continue-on-error`: a failed
  job that sets it lets the workflow pass, so it can't block a pull request.
- The name itself isn't important, but that's the field that appears in the
  GitHub UI for the check, not the job ID.

`norms verify` checks that `ci-success` exists, runs `if: always()`, needs
every other job, and fails on each result other than success: some step that
exits with an error, without `continue-on-error`, has a condition holding for
it, such as `contains(needs.*.result, 'skipped')`, alone or joined to others by
`||`. It also checks that no job sets `continue-on-error` to `true` or an
expression.

### Hardening

- Pin every action to a full commit SHA, with the version in a trailing comment
  (`uses: actions/checkout@<sha> # v7.0.1`). Renovate keeps these current.
- Set `permissions: {}` at the top level of every workflow, and grant each job
  only the permissions it needs.
- Grant `contents: read` to every job that checks out the repository, and
  nothing to a job that doesn't, such as the gate job. Only private and internal
  repositories need it, for `actions/checkout`, but granting it everywhere keeps
  the workflows the same whatever the repository's visibility, and in a public
  repository it exposes nothing that isn't already public. No job needs more.
- Set `persist-credentials: false` on every `actions/checkout` step.
- Run [zizmor](https://docs.zizmor.sh/) in both CI and the pre-commit hooks.

`norms verify` leaves these rules to zizmor's pedantic persona, which flags
unpinned actions, missing or broad permissions and persisted credentials. It
checks only that the hook and the CI job run zizmor with that persona, the hook
with `--pedantic` and the job through `zizmorcore/zizmor-action` with `persona:
pedantic`, and that the action sets `advanced-security: false`.

## Pre-Commit Hooks

Pre-commit hooks are provided by [pre-commit](https://pre-commit.com/),
configured in `.pre-commit-config.yaml` at the repository root. The
configuration sets `default_install_hook_types` so that a plain `pre-commit
install` also installs the commit message hook, and `default_stages` so that
the other hooks run only once per commit:

```yaml
default_install_hook_types: [pre-commit, commit-msg]
default_stages: [pre-commit]
```

Without `default_stages`, a hook that doesn't list its own stages runs in every
installed stage, so the language hooks would run again on the commit message.
A project with hooks of another type, such as `pre-push`, adds that type to
`default_install_hook_types`; `default_stages` stays `[pre-commit]`, and those
hooks list their own stages.

Hook revisions are pinned to release tags and updated by Renovate.

### Common Hooks

Every project uses these hooks:

- [pre-commit-hooks](https://github.com/pre-commit/pre-commit-hooks):
  `check-added-large-files`, `check-case-conflict`,
  `check-illegal-windows-names`, `check-merge-conflict`, `check-symlinks`,
  `check-toml`, `check-vcs-permalinks`, `check-yaml`, `detect-private-key`,
  `end-of-file-fixer` and `trailing-whitespace`. `check-merge-conflict` looks
  for conflict markers only while a merge or rebase is in progress, which is
  when they can be committed by mistake, so in CI it always passes.
- [reuse](https://github.com/fsfe/reuse-tool): `reuse` (see
  [Licensing](#licensing)).
- [markdownlint-cli2](https://github.com/DavidAnson/markdownlint-cli2):
  `markdownlint-cli2` (see [Markdown](#markdown)).
- [Tombi](https://github.com/tombi-toml/tombi-pre-commit): `tombi-format`,
  which checks the formatting of every TOML file apart from the generated lock
  files. In `Cargo.toml` and `pyproject.toml`, that includes the order of keys
  their schemas define one for, such as those of `[package]`, as well as of
  dependencies (see [Dependency Order](#dependency-order)):

  ```yaml
  - id: tombi-format
    args: [--check]
    exclude: (^|/)(Cargo|uv)\.lock$
  ```

  CI runs Tombi in a `tombi` job instead, installed by `tombi-toml/setup-tombi`
  and run as `tombi format --check --diagnostics-format github`, which annotates
  each unformatted file. When given no files, Tombi formats only `*.toml` files,
  so it skips the lock files too. Any further exclusions go in `tombi.toml`'s
  `[files] exclude`, which applies to the hook as well.

- [zizmor](https://github.com/zizmorcore/zizmor-pre-commit): `zizmor`.
- [conventional-pre-commit](https://github.com/compilerla/conventional-pre-commit):
  `conventional-pre-commit` in the `commit-msg` stage, with the recognized
  commit types passed as arguments:

  ```yaml
  - id: conventional-pre-commit
    stages: [commit-msg]
    args: [feat, fix, perf, refactor, style, test, docs, build, ci, revert, chore]
  ```

- [norms](https://github.com/aexoden/norms): `norms-commit-msg`, also in the
  `commit-msg` stage, which rejects a subject over 72 characters or a body line
  over 72 characters, unless the body line holds a URL. Like git's default
  cleanup, it leaves out comment lines, which start with `#`, or instead with
  the `core.commentChar` (or `core.commentString`) git is configured with, and
  the diff `git commit --verbose` adds. When git opens no editor, as for
  `git commit -m`, it keeps comment lines, as git does. The subject is the first
  paragraph, its lines joined as `git log` shows them, less the `fixup!`,
  `squash!` and `amend!` prefixes that `git rebase --autosquash` removes; a
  merge subject git writes, starting with `Merge`, may be of any length. CI
  applies the same rules to pull request titles (see [Branches](#branches)).

  `norms-verify`, in the `pre-commit` stage, runs `norms verify` on the whole
  project, whichever files a commit changes. Like the other hooks that check
  the whole project, it also sees untracked files that `.gitignore` doesn't
  exclude. It fails if a check fails, and with `--deny-warnings` if one warns,
  too, which these standards require. pre-commit builds norms at the pinned
  revision with cargo, installing Rust first in a project without it:

  ```yaml
  - repo: https://github.com/aexoden/norms
    rev: vX.Y.Z
    hooks:
      - id: norms-commit-msg
      - id: norms-verify
        args: [--deny-warnings]
  ```

  `norms verify` fails a project without the `norms-commit-msg` hook in the
  `commit-msg` stage or the `norms-verify` hook, and warns when `norms-verify`
  doesn't pass `--deny-warnings`.

`norms verify` checks that every hook it checks, the language hooks included,
runs in the `pre-commit` stage (the `conventional-pre-commit` and
`norms-commit-msg` hooks in `commit-msg`), and warns when a language hook isn't
set up as the next section says. The `conventional-pre-commit` hook must list
the commit types itself, rather than rely on the hook's defaults, which a
release may change.

### Language Hooks

Language hooks are `local` hooks with `language: unsupported` and
`pass_filenames: false`. (`unsupported` is pre-commit 4.4's name for `system`,
which it still accepts but means to deprecate.) They invoke the project's own
tools directly (for example, `cargo clippy` or `uv run ruff check`), never
through Devbox. Running the project's tools, they use the versions its lock
files and toolchain pins select, as CI does. A remote hook is pinned by its own
`rev` instead, and one in a language pre-commit manages, such as Python or
Node.js, installs its own copy of the tool, without the project's dependencies
or plugins. Each language hook checks all of its component's files, not only
the staged ones, since a change to one file can break another, and checks
without modifying files, so the hooks and CI produce the same results. The
language sections below list the required hooks. Hook IDs are unique, across
languages too (for example, `pytest`, `cargo-test` and `vitest` rather than
`test`), so that `SKIP` can target one. `SKIP` and `pre-commit run` also match a
hook's `alias`, so a hook that runs twice under one ID, such as a language hook
run for two components, gives each copy an alias of its own (`eslint-admin`,
`eslint-web`).

## Dependency Management

### Automated Updates

Use [Renovate](https://docs.renovatebot.com/) for automated dependency updates.
The shared [renovate-config](https://github.com/aexoden/renovate-config) preset
builds on `config:best-practices` and provides the automerge, grouping, pinning
and release age policies, so `renovate.json` normally contains nothing else:

```json
{
  "$schema": "https://docs.renovatebot.com/renovate-schema.json",
  "extends": ["github>aexoden/renovate-config"]
}
```

Project-specific rules may be added when a project needs them.

### Version Pinning

Every project commits its lock files, as each tool recommends. Applications also
pin every direct dependency, including development dependencies, to an exact
version:

| Ecosystem | Manifest syntax                     | Lock file        |
|-----------|-------------------------------------|------------------|
| Cargo     | `serde = "=1.0.228"`                | `Cargo.lock`     |
| Python    | `"pydantic==2.12.0"`                | `uv.lock`        |
| pnpm      | `"react": "19.2.0"` (no `^` or `~`) | `pnpm-lock.yaml` |

Dependencies from a path, a Git repository, a URL or another workspace member
have no version to pin, so this applies to those from a registry.

Libraries instead declare compatible version ranges, so that downstream projects
can resolve shared dependencies to a single version. Their lock files are still
committed for reproducible development and CI.

### Dependency Order

Dependencies are listed in alphabetical order within each table, list or
object, such as `[dependencies]` in `Cargo.toml`, `[project] dependencies` and
each dependency group in `pyproject.toml`, and `devDependencies` in
`package.json`. Formatters keep the order, so it needs no manual care:

- `Cargo.toml` and `pyproject.toml`: [Tombi](https://tombi-toml.github.io/tombi/)
  sorts them using the schemas it ships with. The `tombi-format` hook checks
  the order, and formatting the file with Tombi (or its editor extension) fixes
  it.
- `package.json`: Prettier sorts them through prettier-plugin-packagejson (see
  [Formatting and Linting](#formatting-and-linting-1) under TypeScript), as pnpm
  also does whenever it writes the file.

### Release Age

A new release must be at least three days old before a project adopts it. The
delay gives the community time to catch and yank compromised or broken releases
before they reach a project. It is enforced at two levels:

1. **Renovate**, through `minimumReleaseAge` in the shared preset.
2. **The package manager**, which covers what Renovate cannot age-check, such as
   lock file maintenance and manual updates:
   - uv: `exclude-newer = "3 days"` under `[tool.uv]` in `pyproject.toml`.
   - pnpm: `minimumReleaseAge: 4320` (in minutes) in `pnpm-workspace.yaml`.
   - Cargo: `global-min-publish-age = "3 days"` under `[registry]` in
     `.cargo/config.toml`. This is stable as of Rust 1.100; earlier toolchains
     silently ignore it, so it takes effect when the pinned toolchain reaches
     1.100.

#### Urgent Updates

Security fixes may bypass the release age requirement. Renovate's vulnerability
alert PRs are exempt from `minimumReleaseAge` in the shared preset. To update a
single package early through the package manager, add a temporary per-package
exemption, with a comment naming the advisory:

- uv: `exclude-newer-package = { package-name = false }` under `[tool.uv]`.
- pnpm: `minimumReleaseAgeExclude: [package-name]` in `pnpm-workspace.yaml`.
- Cargo: there is no per-package setting. Run
  `CARGO_RESOLVER_INCOMPATIBLE_PUBLISH_AGE=allow cargo update -p package-name`,
  and the lock file keeps the newer version.

Remove a configured exemption once the release has passed the normal release
age.

## Language-Specific Standards

Python, Rust and TypeScript have the standards below. `norms verify` also
detects C++ components, by their `CMakeLists.txt`, but their standards, and so
its checks of them, are still to come.

### Python

#### Project Layout

Use [uv](https://docs.astral.sh/uv/) for all Python projects, with a `src/`
layout, in which `src/` holds the package, a directory with an `__init__.py`:

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

Ruff, mypy and uv read their settings from `pyproject.toml`, and nothing takes
their place: a `ruff.toml` or `.ruff.toml` beside it would replace `[tool.ruff]`,
a `mypy.ini` or `.mypy.ini` would replace `[tool.mypy]`, and a `uv.toml` would
replace uv's settings in `[tool.uv]`, such as `exclude-newer` and
`required-version`, though not the project's own, such as `cache-keys` and
`package`. `norms verify` warns on such a file and leaves out the checks of the
settings it replaces.

#### Python and uv Versions

- `.python-version`, beside `pyproject.toml`, names the exact Python version
  (`x.y.z`) used for development and CI.
- Applications set `requires-python` to that same exact version (for example,
  `==3.15.0`). Libraries set the range of versions they support, which
  includes that version.
- The `Programming Language :: Python :: 3.x` classifiers match
  `requires-python`: they list each minor version it allows, from its lower
  bound up to the version in `.python-version`.
- `[tool.uv] required-version` sets the minimum uv version the project needs,
  as a single `>=` (for example, `>=0.9.5`), so that newer releases of uv keep
  working. The CI setup step pins the exact uv version, which `required-version`
  allows.
- A library's CI also runs its tests on the oldest Python it supports, in a
  `pytest-min-python` job whose `astral-sh/setup-uv` step sets that version as
  `python-version`.

#### Dependencies

Development tools go in the `dev` dependency group, not in optional
dependencies:

```toml
[dependency-groups]
dev = ["mypy==...", "pytest==...", "pytest-cov==...", "ruff==..."]
```

#### Versioning

The package version comes from git tags through
[hatch-vcs](https://github.com/ofek/hatch-vcs):

```toml
[project]
dynamic = ["version"]

[build-system]
requires = ["hatch-vcs", "hatchling"]
build-backend = "hatchling.build"

[tool.hatch.version]
source = "vcs"

[tool.uv]
# Reinstall the project when HEAD or tags move, so the reported version stays current.
cache-keys = [{ file = "pyproject.toml" }, { git = { commit = true, tags = true } }]
```

A Python component in a subdirectory of the repository also sets `raw-options
= { search_parent_directories = true }` in `[tool.hatch.version]`, so that
hatch-vcs finds the repository above it. A project with `[tool.uv] package =
false` isn't built, so it has no version and needs none of this.

`.git_archival.txt`, marked `export-subst` in `.gitattributes`, lets source
archives without git history report their version. It sits beside
`pyproject.toml` and holds the placeholders git fills in when it creates an
archive, including the commit and the description from the nearest tag,
lightweight tags included:

```txt
node: $Format:%H$
node-date: $Format:%cI$
describe-name: $Format:%(describe:tags=true,match=*[0-9]*)$
```

Code reads the version at runtime with `importlib.metadata.version()`, in
`__about__.py`.

#### Formatting and Linting

Use [Ruff](https://docs.astral.sh/ruff/) for both. The baseline configuration
enables every rule, including preview rules:

```toml
[tool.ruff]
line-length = 120

[tool.ruff.format]
line-ending = "lf"

[tool.ruff.lint]
preview = true
select = ["ALL"]
ignore = ["missing-trailing-comma"]

[tool.ruff.lint.flake8-copyright]
notice-rgx = "(?i)SPDX-FileCopyrightText: \\d{4}"

[tool.ruff.lint.isort]
lines-between-types = 1

[tool.ruff.lint.per-file-ignores]
"tests/*.py" = ["assert", "private-member-access"]

[tool.ruff.lint.pydocstyle]
convention = "google"
```

`line-length` is 120, as is Prettier's `printWidth`. The ignored rule conflicts
with the Ruff formatter, which warns when it is enabled. Any further ignore
needs a comment in `pyproject.toml` explaining why.

#### Type Checking

Use [mypy](https://mypy.readthedocs.io/en/stable/) in strict mode, along with
the checks that strict mode leaves out:

```toml
[tool.mypy]
strict = true
strict_equality_for_none = true
warn_unreachable = true
enable_error_code = [
  "deprecated",
  "exhaustive-match",
  "explicit-override",
  "mutable-override",
  "possibly-undefined",
  "redundant-expr",
  "redundant-self",
  "truthy-bool",
  "truthy-iterable",
  "unimported-reveal",
  "unused-awaitable",
]
```

`explicit-override` requires `@override` on every method that overrides one in
a base class, imported from `typing`, or from `typing_extensions` in a library
that supports Python before 3.12. The other optional checks are left off.
Ruff's `blanket-type-ignore` already reports what `ignore-without-code` does,
`disallow_any_explicit` would reject every annotation with `Any`, even where it
is the honest type, and `disallow_any_unimported` matters only to a project that
ignores missing imports, which fail by default.

#### Testing

Use [pytest](https://docs.pytest.org/) with
[pytest-cov](https://pytest-cov.readthedocs.io/), reporting missing lines: `uv
run pytest --cov=src/package_name --cov-report=term-missing`.

pytest runs in strict mode, set in its native TOML table, which pytest reads
since version 9:

```toml
[tool.pytest]
strict = true
```

Strict mode turns its configuration warnings, unregistered markers, duplicate
parameter IDs and unexpectedly passing `xfail` tests into errors, and takes in
any strictness option a later pytest adds. A `pytest.toml`, `.pytest.toml`,
`pytest.ini` or `.pytest.ini` beside `pyproject.toml` would take the place of
`[tool.pytest]`, so there is none.

#### Hooks and CI

The pre-commit hooks and CI both run:

| Hook ID       | Command                                                          |
|---------------|------------------------------------------------------------------|
| `uv-lock`     | `uv lock --check`                                                |
| `ruff-format` | `uv run ruff format --check`                                     |
| `ruff-check`  | `uv run ruff check`                                              |
| `mypy`        | `uv run mypy .`                                                  |
| `pytest`      | `uv run pytest --cov=src/package_name --cov-report=term-missing` |

CI installs uv with `astral-sh/setup-uv` and runs `uv sync --locked` before the
checks.

### Rust

#### Toolchain

`rust-toolchain.toml` pins an exact channel and lists the components the hooks
need, so `rustup` installs everything on first use:

```toml
[toolchain]
channel = "1.99.1"
components = ["clippy", "rustfmt"]
```

rustup uses the nearest `rust-toolchain.toml` in the directory it runs in or a
parent, so a Rust component in a subdirectory shares the root's file unless it
has its own. Either way, the file the component uses must meet these rules.

Libraries also set `rust-version` (the MSRV) in `Cargo.toml`. In a workspace,
each member sets it, or inherits it with `rust-version.workspace = true`.
Applications do not need one, since the toolchain file already pins their
compiler. A library's CI checks the MSRV in an `msrv` job, which installs that
toolchain with `actions-rust-lang/setup-rust-toolchain`'s `toolchain` input,
overriding `rust-toolchain.toml`, and runs `cargo check --workspace
--all-features --locked`. It checks only the library, not its tests, since
development dependencies may need a newer Rust.

`.cargo/config.toml` sets the release age (see [Release Age](#release-age)):

```toml
[registry]
global-min-publish-age = "3 days"
```

#### Package Metadata

`Cargo.toml` sets `description`, `repository`, `license` and `edition` (the
latest edition, currently 2024). A package that may be published also sets
`keywords` and `categories`, which Clippy's `cargo_common_metadata` lint, in the
`cargo` group (see [Lints](#lints)), requires of it. In a workspace, a package
may inherit these fields from `[workspace.package]` instead, with
`license.workspace = true` and so on.

#### Lints

The baseline enables the `pedantic`, `nursery` and `cargo` Clippy groups, plus
a selection of restriction lints. The selection is interim and will be revised
as cases come up:

```toml
[lints.clippy]
absolute_paths = "warn"
allow_attributes = "warn"
allow_attributes_without_reason = "warn"
as_pointer_underscore = "warn"
cargo = { level = "warn", priority = -1 }
cfg_not_test = "warn"
clone_on_ref_ptr = "warn"
create_dir = "warn"
dbg_macro = "warn"
decimal_literal_representation = "warn"
default_union_representation = "warn"
definition_in_module_root = "warn"
doc_paragraphs_missing_punctuation = "warn"
empty_enum_variants_with_brackets = "warn"
empty_structs_with_brackets = "warn"
exit = "warn"
expect_used = "warn"
field_scoped_visibility_modifiers = "warn"
filetype_is_file = "warn"
float_cmp_const = "warn"
fn_to_numeric_cast_any = "warn"
if_then_some_else_none = "warn"
indexing_slicing = "warn"
infinite_loop = "warn"
inline_modules = "warn"
lossy_float_literal = "warn"
map_err_ignore = "warn"
map_with_unused_argument_over_ranges = "warn"
mem_forget = "warn"
mixed_read_write_in_expression = "warn"
multiple_crate_versions = "allow"
multiple_inherent_impl = "warn"
multiple_unsafe_ops_per_block = "warn"
needless_raw_strings = "warn"
non_zero_suggestions = "warn"
nursery = { level = "warn", priority = -1 }
panic = "warn"
panic_in_result_fn = "warn"
partial_pub_fields = "warn"
pathbuf_init_then_push = "warn"
pedantic = { level = "warn", priority = -1 }
pointer_format = "warn"
precedence_bits = "warn"
rc_buffer = "warn"
rc_mutex = "warn"
redundant_pub_crate = "allow"
redundant_test_prefix = "warn"
redundant_type_annotations = "warn"
renamed_function_params = "warn"
rest_pat_in_fully_bound_structs = "warn"
return_and_then = "warn"
same_name_method = "warn"
self_named_module_files = "warn"
semicolon_inside_block = "warn"
shadow_unrelated = "warn"
str_to_string = "warn"
string_add = "warn"
string_slice = "warn"
suspicious_xor_used_as_pow = "warn"
tests_outside_test_module = "warn"
todo = "warn"
try_err = "warn"
unimplemented = "warn"
unneeded_field_pattern = "warn"
unseparated_literal_suffix = "warn"
unused_result_ok = "warn"
unused_trait_names = "warn"
unwrap_used = "warn"
use_debug = "warn"
verbose_file_reads = "warn"
wildcard_enum_match_arm = "warn"

[lints.rust]
future_incompatible = { level = "warn", priority = -1 }
let_underscore = { level = "warn", priority = -1 }
unreachable_pub = "warn"
unsafe_code = "forbid"
unused_qualifications = "warn"
```

Groups are set to priority -1 so that individual lints, which default to 0, can
override them.

`norms verify` warns when a package doesn't set the `pedantic`, `nursery` and
`cargo` groups to `warn` or stricter, or any of the `[lints.rust]` entries
above to its level or stricter. The restriction lints aren't checked while the
selection is interim.

In a workspace, the lints may be set once in `[workspace.lints.clippy]` and
`[workspace.lints.rust]`. Cargo applies them only to packages that inherit them
with `[lints] workspace = true`, which inherits both tables, including the
package in the root manifest, if there is one.

`multiple_crate_versions` is allowed because transitive dependencies routinely
pull in several versions of the same crate, which a project cannot fix.

`unreachable_pub` asks for `pub(crate)` on items that are not reachable from
outside the crate. `redundant_pub_crate`, a nursery lint, asks for the opposite,
so it is allowed. Fields of `pub(crate)` types stay `pub`, since
`field_scoped_visibility_modifiers` forbids scoped visibility on fields.

`self_named_module_files` requires `foo/mod.rs` over `foo.rs` next to `foo/`.
`definition_in_module_root` keeps `mod.rs` to declaring and re-exporting its
submodules, so that each definition lives in a file named after its module, and
`inline_modules` gives every module apart from tests a file of its own.

Some restriction lints are left out deliberately because they work against
idiomatic Rust or do not suit applications:

- `shadow_reuse` and `shadow_same`: shadowing a binding to transform it, as in
  `let Some(x) = x else { ... }`, is idiomatic. `shadow_unrelated` is enabled.
- `arbitrary_source_item_ordering`: field and variant order carries meaning for
  derived `Ord`, serde output and clap help.
- `impl_trait_in_params`, `exhaustive_enums` and `exhaustive_structs`: these
  only matter for library APIs.
- `error_impl_error`: a type named `Error` implementing `Error` is the usual
  convention.
- `get_unwrap`: `unwrap_used` already covers it, and its suggestion trips
  `indexing_slicing`.
- `module_name_repetitions`: too noisy for its benefit.

`clippy.toml` allows `unwrap`, `expect` and `panic` in tests, and limits
absolute paths to three segments:

```toml
absolute-paths-max-segments = 3
allow-expect-in-tests = true
allow-panic-in-tests = true
allow-unwrap-in-tests = true
```

Clippy uses the nearest `clippy.toml` in the package's directory or a parent, so
a Rust component in a subdirectory shares the root's file unless it has its own,
and so does a workspace member. Whichever file a package uses must meet these
rules. The file is named `clippy.toml`, not `.clippy.toml`, which Clippy also
reads.

The hooks and CI pass `-D warnings`, so any warning fails the check.

#### Dependency Policy

`deny.toml` configures
[cargo-deny](https://embarkstudios.github.io/cargo-deny/):

- `[licenses] allow` specifies the licenses the project accepts, limited to
  those cargo-deny encounters. By default that excludes dev-only dependencies.
- `[bans] multiple-versions = "allow"`, for the same reason as the Clippy lint
  above.
- `[bans] std-replacements = {}`, which fails on a crate the workspace depends
  on directly that `std` or `core` now replaces, such as `once_cell`. Without
  the table, cargo-deny skips this check. A package's `rust-version`, which
  libraries set, limits it to the replacements its MSRV has. A crate the
  project keeps anyway goes in the table's `ignore`.
- Every entry in `[advisories] ignore` has a `reason`.

cargo-deny uses the nearest `deny.toml` in the directory of the manifest it
checks or a parent, so a Rust component in a subdirectory shares the root's file
unless it has its own. Whichever file a component uses must meet these rules.
The file is named `deny.toml`, not `.deny.toml` or `.cargo/deny.toml`, which
cargo-deny also reads.

The hooks and CI pass `-D warnings`, so any warning fails the check.

cargo-deny is installed separately by the developer. See the [cargo-deny
installation
documentation](https://embarkstudios.github.io/cargo-deny/#install-cargo-deny).

#### Hooks and CI

The pre-commit hooks and CI both run:

| Hook ID      | Command                                                                         |
|--------------|---------------------------------------------------------------------------------|
| `cargo-lock` | `cargo update --workspace --locked`                                             |
| `rustfmt`    | `cargo fmt --all --check`                                                       |
| `clippy`     | `cargo clippy --workspace --all-targets --all-features -- -D warnings`          |
| `cargo-test` | `cargo test --workspace --all-features`                                         |
| `cargo-deny` | `cargo deny --workspace --all-features check -D warnings bans licenses sources` |

`cargo-lock` fails, naming the entries that would change, if `Cargo.lock` is
out of date with `Cargo.toml`. Updating only the workspace's own packages,
`cargo update --workspace` changes nothing in a current lock file, and
`--locked` turns any change it would make into an error, so the lock file is
left untouched. The other Cargo commands would quietly update a stale lock file
instead.

`norms verify` fails a `clippy` hook or CI job that runs Clippy without
`-D warnings`, since every lint is set to `warn`, so Clippy would pass whatever
it finds, and warns on one without `--workspace`, `--all-targets` or
`--all-features`, or that denies warnings through `RUSTFLAGS` instead. It also
warns on a `cargo-deny` hook that passes other arguments than the table's,
apart from `--manifest-path`.

CI installs the toolchain with `actions-rust-lang/setup-rust-toolchain`, which
reads `rust-toolchain.toml` and caches builds with its built-in
`Swatinem/rust-cache`, so jobs don't add a separate cache step. Jobs that build
nothing, such as formatting, set `cache: false`. CI checks formatting with
`actions-rust-lang/rustfmt` and runs cargo-deny with
`EmbarkStudios/cargo-deny-action`, passing the arguments in the table above as
`arguments: --workspace --all-features` and `command: check -D warnings bans
licenses sources`. The advisory check runs in `audit.yaml` (see
[Continuous Integration](#continuous-integration)), through the same action
with `arguments: --workspace --all-features` and `command: check -D warnings
advisories`.

### TypeScript

#### Location

A TypeScript project lives at the repository root when it is the primary
language. Otherwise it lives in a named subdirectory, such as `frontend/`.
Paths below are relative to that directory.

#### Package Configuration

`package.json` sets `"type": "module"` and `"private": true` (for applications),
and pins Node.js and pnpm through `devEngines`. pnpm downloads the declared
versions automatically:

```json
"devEngines": {
  "runtime": { "name": "node", "version": "26.13.0", "onFail": "download" },
  "packageManager": { "name": "pnpm", "version": "12.18.0", "onFail": "download" }
}
```

Corepack's `packageManager` field is not set. Corepack and some CI actions read
it, so it would be a second pnpm pin that can drift from `devEngines`.

pnpm is the only package manager, so there is no `package-lock.json` or
`yarn.lock` beside `pnpm-lock.yaml`.

`package.json` defines these scripts:

| Script      | Command                     |
|-------------|-----------------------------|
| `build`     | `tsc -b && vite build`      |
| `format`    | `prettier --check .`        |
| `lint`      | `eslint . --max-warnings 0` |
| `test`      | `vitest run`                |
| `typecheck` | `tsc -b --noEmit`           |

Like the hook and CI job, the `lint` script fails on warnings as well as errors.

`pnpm-workspace.yaml` sets the release age, refuses trust downgrades, and
records in `permissions` (read since pnpm 12.11) what each dependency may do,
such as running build scripts:

```yaml
minimumReleaseAge: 4320
permissions:
  esbuild:
    build: true
trustPolicy: no-downgrade
```

A new project starts with `permissions: {}`, so that `pnpm approve` and
`pnpm approve-builds` record their decisions there. pnpm fails the install when
a dependency's build scripts haven't been approved or denied, so neither
`dangerouslyAllowAllBuilds: true` nor `strictDepBuilds: false` is set.
`allowBuilds`, which pnpm still reads for the dependencies `permissions` doesn't
list, isn't used, so that every decision is in one place. Nor are the settings
it replaced in pnpm 11, such as `onlyBuiltDependencies`, which pnpm ignores.

`trustPolicy: no-downgrade`, which is off by default, fails the install when a
version was published with weaker trust evidence than an earlier one, such as
without the provenance or trusted publisher its predecessors had, as a release
published with stolen credentials often is. A package that legitimately lost
that evidence is listed in `trustPolicyExclude`, with a comment saying why.

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
"verbatimModuleSyntax": true,
"erasableSyntaxOnly": true,
"moduleDetection": "force"
```

`verbatimModuleSyntax` implies `isolatedModules`, which therefore need not be
set. `erasableSyntaxOnly` rejects the TypeScript syntax that has effects at
runtime, such as enums, namespaces and parameter properties, so that removing
the types leaves working JavaScript, as Node's type stripping requires. The
application sets it too, so that all of the project's TypeScript is written the
same way, and any of it could run without a build step.

The module settings follow what loads each config's files. Vite bundles the
application, so `tsconfig.app.json` sets `"module": "esnext"` and
`"moduleResolution": "bundler"`. Node can load the tooling files itself,
stripping their types, so `tsconfig.node.json` sets `"module": "nodenext"`, which
holds their imports to Node's rules, and leaves `moduleResolution` unset to
follow it. Node finds a relative import by the name of the TypeScript file, as
in `import { x } from "./helper.ts"`, which TypeScript accepts only with
`"allowImportingTsExtensions": true`, so the tooling's config sets that too.

`target` and `lib` name the newest ECMAScript version that the code's runtime
fully supports. Vite lowers newer syntax to its build target, but adds no
polyfills, so `lib` decides which APIs type-check. For the application, that is
the newest version every browser of Vite's default build target supports:
ES2022 for Vite 8, since its Firefox 114 lacks ES2023's copying array methods,
such as `toSorted`. For the tooling files, it is the newest version TypeScript
knows that the pinned Node.js supports: ES2025 for Node.js 26. `norms verify`
can't tell the runtime, so it only warns when either names a version older than
ES2022.

#### Formatting and Linting

Use [Prettier](https://prettier.io/) for formatting, configured in `.prettierrc`,
along with a `.prettierignore`. The `.prettierrc` is:

```json
{
  "plugins": ["prettier-plugin-packagejson"],
  "printWidth": 120
}
```

Prettier reads a `prettier` key in `package.json` before `.prettierrc`, so
`package.json` has none, and no other Prettier configuration file takes the
place of `.prettierrc`.

[prettier-plugin-packagejson](https://github.com/matzkoh/prettier-plugin-packagejson),
a development dependency, formats `package.json` with
[sort-package-json](https://github.com/keithamus/sort-package-json): its fields
in a conventional order, and its scripts and dependencies sorted (see
[Dependency Order](#dependency-order)).

Use [ESLint](https://eslint.org/) for linting, configured in `eslint.config.ts`.
The configuration includes the `strictTypeChecked` and `stylisticTypeChecked`
configs from [typescript-eslint](https://typescript-eslint.io/), with
`projectService` enabled for type-aware rules, and `eslint-config-prettier` to
disable rules that conflict with Prettier. ESLint loads `eslint.config.ts`
through [jiti](https://github.com/unjs/jiti), a development dependency. ESLint
uses Node's own type stripping only behind its experimental
`unstable_native_nodejs_ts_config` flag, which every place ESLint runs, editors
included, would have to pass. ESLint looks for `eslint.config.js`, `.mjs` and
`.cjs` before `eslint.config.ts`, so there is no other `eslint.config.*` file.

`norms verify` warns when Prettier or ESLint would read another file than
`.prettierrc` or `eslint.config.ts`, and leaves out the checks of their
contents.

#### Testing

Use [Vitest](https://vitest.dev/), with
[@vitest/coverage-v8](https://vitest.dev/guide/coverage.html), a development
dependency, reporting missing lines: `pnpm exec vitest run --coverage`. The
`test` script leaves coverage out, for a quick run; the hook and CI report it.
`vite.config.ts` includes every source file in the report, not only those the
tests import, and prints it rather than writing it to files:

```ts
/// <reference types="vitest/config" />
import { defineConfig } from "vite";

export default defineConfig({
    test: {
        coverage: {
            include: ["src/**"],
            reporter: ["text"],
        },
    },
});
```

#### Hooks and CI

The pre-commit hooks and CI both run the following. When the project lives in a
subdirectory, the hooks prefix each command with `pnpm --dir <path>`, and CI
sets the job's working directory instead:

| Hook ID                  | Command                               |
|--------------------------|---------------------------------------|
| `pnpm-lock`              | `pnpm install --frozen-lockfile`      |
| `pnpm-peer-dependencies` | `pnpm peers check`                    |
| `prettier`               | `pnpm exec prettier --check .`        |
| `eslint`                 | `pnpm exec eslint . --max-warnings 0` |
| `tsc`                    | `pnpm exec tsc -b --noEmit`           |
| `vite-build`             | `pnpm exec vite build`                |
| `vitest`                 | `pnpm exec vitest run --coverage`     |

`tsc` runs in build mode (`-b`), here and in the `build` and `typecheck`
scripts, since only build mode follows the references of the solution-style
`tsconfig.json`, which lists no files of its own: without it, `tsc` checks
nothing, so `norms verify` fails a hook, CI step or script that runs it so. A
component whose `tsconfig.json` isn't solution-style is still checked without
`-b`, so there, and in hooks and CI when any component's isn't, it warns.

`vite-build` catches errors that only bundling reveals, such as a missing
stylesheet or asset, which `tsc` accepts through Vite's module declarations, or
a broken script in `index.html`. It writes the build to `dist/`, which is
ignored, so like the other hooks it changes no tracked file.

CI has no `pnpm-lock` job: `pnpm/setup` installs the dependencies, and pnpm
defaults to `--frozen-lockfile` in CI, so every job fails on a stale lock file.

pnpm switches to the version pinned in `devEngines` only when it runs in the
project's own directory, and `--dir` doesn't count. In a subdirectory, the hooks
therefore run pnpm's own commands through `exec`, as in `pnpm --dir <path> exec
pnpm install --frozen-lockfile`, so that the pinned pnpm runs them.
