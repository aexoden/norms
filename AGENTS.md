# AGENTS.md

norms is two things: `STANDARDS.md`, the standards for the author's projects,
and the `norms` Rust CLI that checks a project against them
(`norms verify <path> [--json] [--deny-warnings]`), which other projects run as
the `norms-verify` hook from `.pre-commit-hooks.yaml`. The CLI also checks a
commit message's line lengths (`norms commit-msg <file>`, in `commit_msg.rs`),
which other projects run as the `norms-commit-msg` hook. norms runs both hooks
on itself as local hooks (`tests/hooks.rs` keeps them in step with the shipped
ones), and CI's `norms-verify` job runs `norms verify --deny-warnings` on it.
The repository also ships a Copier template (`copier.yml`, `template/`) that
renders new projects following the standards.
The repository follows its own standards, so `STANDARDS.md` is the reference for
conventions here too.

## Commands

The Rust toolchain is pinned in `rust-toolchain.toml`. These match the
pre-commit hooks and CI:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test <name>                         # a single test, by name substring
NORMS_BLESS=1 cargo test --test fixtures  # rewrite the fixture snapshots
# cargo-deny is installed separately
cargo deny --workspace --all-features check -D warnings bans licenses sources
cargo run -- verify <path>                # run norms against a project
cargo run -- verify --deny-warnings .     # run norms against itself
pre-commit run --all-files                # every hook, incl. reuse, markdownlint, tombi, zizmor
```

Clippy runs `pedantic`, `nursery` and `cargo` with `-D warnings`, plus the
restriction lints listed in `Cargo.toml` (see the Lints section of
`STANDARDS.md`). Among them, `unwrap`/`expect`/`panic` and indexing are flagged
outside tests, so return errors or handle `None` instead, and crate-internal
items are `pub(crate)`.

Rendering the template locally, as the CI `template` job does (it needs
`--vcs-ref HEAD`, so commit first; answers are passed with `--data`):

```sh
git init /tmp/demo
uvx copier copy --trust --defaults --vcs-ref HEAD \
  --data project_name=demo-project --data "description=A demo." \
  --data primary_language=rust . /tmp/demo
git -C /tmp/demo add --all && (cd /tmp/demo && uvx pre-commit run --all-files)
```

## Architecture of the CLI

`cli::verify` builds a `ProjectContext`, runs `checks::run_all_checks`, and
prints a `Report`, exiting 1 if any check failed, or with `--deny-warnings` if
any check failed or warned.

- `context.rs`: `ProjectContext` lazily parses each config file at most once
  (`OnceLock`) and returns a `ConfigStatus<T>` (`NotFound`, `ParseError` with
  the raw text, or `Ok`). Checks share parsed data through it rather than
  reading files themselves. It holds the repository-wide files and the detected
  components; each component's own files (`Cargo.toml`, `package.json`, ...)
  are in a `ComponentContext` rooted at the component's directory, one per
  component, which `ProjectContext::component_contexts` caches so that every
  check shares them.
- `config/`: serde models of only the fields checks inspect, with
  `#[serde(default)]` so missing keys don't fail parsing.
- `checks/`: plain functions `fn(&ProjectContext, &mut Report)`, or
  `fn(&ComponentContext, &mut Report)` for a check of a component's own files
  (also given the `ProjectContext` when a repository-wide file applies to that
  component alone, such as the CI job testing a library's MSRV).
  Registration is explicit: a new check must be added to the list in
  `checks/mod.rs`. `common.rs` runs for every project. For each detected
  language, the component checks run once per component of that language
  (inside `Report::with_component`, which labels their results with the
  component's path), then the checks of the language's place in repository-wide
  files (pre-commit hooks, CI) run once.
- `report.rs`: `report.must(id, name, outcome)` records a check whose issue is
  a failure, `report.should(...)` one whose issue is a warning (its Severity
  section gives the rule for choosing);
  `require_parsed`/`recommend_parsed` unwrap a `ConfigStatus` while recording
  the problem if it's missing or malformed. Every check has a stable kebab-case
  ID (subject, then property: `cargo-toml-license`; see `report.rs`), so don't
  rename an ID once projects may depend on it; the name beside it is free text.
- `norms.toml` (`config/norms.rs`, documented in `STANDARDS.md`) is optional
  and holds the project's waivers and the directories excluded from component
  detection; norms' own excludes `tests/fixtures`. Unlike the other models it
  denies unknown fields. `run_all_checks` applies the waivers last
  (`Report::apply_waivers`), turning waived failures and warnings into skips.
- `detection.rs`: components come from marker files (`Cargo.toml`,
  `pyproject.toml`, `package.json`, `CMakeLists.txt`) anywhere in the project,
  found with the `ignore` crate's walker: it honors the project's own
  `.gitignore` files, skips hidden directories, build output and installed
  dependencies (`node_modules`, `target`, `.venv`) and the directories
  `norms.toml` excludes, never descends into symlinked
  directories, and counts a symlinked marker only if it resolves inside the
  root. A marker nested in a component of the same language is folded into it.
  A second walk, which skips only what `.gitignore` excludes, finds
  markdownlint's configuration files, for the check that
  `.markdownlint-cli2.jsonc` is the only one.
  C++ is detected but has no checks yet.
- `cargo_workspace.rs`: a Rust component that is a Cargo workspace has its
  members found by Cargo's rules (`members` globs with the `glob` crate,
  `exclude`, path dependencies inside the workspace), which
  `ComponentContext::cargo_members` caches. They aren't components: their
  results belong to the workspace's component and name the member's manifest
  (`crates/cli/Cargo.toml: ...`).
- `git.rs`: reads the project's release tags, for the changelog check, by
  running `git` with the variables that select a repository cleared. Only a
  project at the top of its repository has any, so the fixtures don't see
  norms' own tags.

CI's `template` job runs `norms verify --deny-warnings` on each render, so a
render must have no failures or warnings.

## Tests

Unit tests sit beside the code they test. `tests/verify.rs` checks
`norms verify`'s exit codes on a copy of a fixture. `tests/fixtures.rs` runs
`norms verify --json` on each project in `tests/fixtures/` and compares what it
reports with `tests/snapshots/<name>.txt`, so any change in findings shows up as
a snapshot diff. When the change is intended, rerun with `NORMS_BLESS=1`, review
`git diff tests/snapshots`, and commit the snapshots with the change.

- The language fixtures are renders of the template, made as in the Template
  section below with `--skip-tasks`: `rust`, `python` and `typescript` with
  `primary_language=<language>` and `project_name=demo-<language>`,
  `rust-typescript` with `project_name=demo`, `primary_language=rust`,
  `secondary_languages=["typescript"]` and `typescript_path=frontend`, and
  `python-rust-library`, the only library render, with
  `project_name=demo-library`, `primary_language=python`,
  `project_type=library` and `secondary_languages=["rust"]`. Each drops
  `.copier-answers.yml` and has stub lock files apart from `Cargo.lock`.
  Re-render them when the template changes in a way the checks see.
- The other fixtures are written by hand: `empty` has no files, `devbox`
  has a services-only `devbox.json` beside a stub marker file for each
  language, and `invalid-ci` has a malformed `ci.yaml` beside a stub
  `Cargo.toml`, with a malformed and a valid workflow beside it and files
  that aren't workflows, which norms skips. `components` has stub markers
  laid out to exercise detection: a Cargo workspace member and a nested
  pnpm package to fold, two TypeScript components, a Python one, and markers
  under `node_modules/`, a hidden directory and an excluded `vendor/` to
  skip; its `norms.toml` also excludes a missing directory. `unpinned` has
  applications whose manifests break the pinning standards (one also pins pnpm
  through Corepack's `packageManager`), beside a Python library that isn't
  checked for them, and release ages other than 3 days.
  `reuse` breaks the licensing standards: two license texts whose root copies
  are misnamed or differ, a `REUSE.toml` of the wrong version, hooks and CI
  that don't run `reuse lint` on the whole project, and a Rust, Python and
  TypeScript manifest whose licenses lack a text or aren't SPDX expressions.
  `toolchain` has two Rust components in subdirectories, one with a
  conforming `rust-toolchain.toml` and `.clippy.toml` of its own and one using
  the root's, whose toolchain pins no exact channel and lacks clippy, and whose
  `clippy.toml` doesn't allow `panic` in tests and allows four path segments.
  `classifiers` has Python components whose classifiers are checked against
  `requires-python`: an application and a library with gaps in its range,
  each with its own `.python-version`, one capped by `requires-python`
  without a pin, one open-ended whose only pin is in its parent directory,
  which uv ignores, and one capped whose `.python-version` isn't a plain
  version.
  `python-pin` breaks the Python pinning standards: one component's
  `.python-version` pins only a minor version, two applications'
  `requires-python` name another version or a range, and a library's range
  excludes its pin.
  `workspace` is a virtual Cargo workspace with a binary and a library member
  matched by a glob, an excluded crate, a malformed member, a member found
  only as a path dependency, and an unrelated crate that isn't a member.
  `msrv` is a virtual Cargo workspace of libraries whose members set
  `rust-version` themselves, inherit it from `[workspace.package]`, or lack it.
  `workflows` breaks the Actions standards: a zizmor hook and CI steps that
  don't run its pedantic persona or don't fail on findings; a job without
  `timeout-minutes`, beside a call to a reusable workflow, which can't set one;
  workflows whose `concurrency` is missing or doesn't cancel; a `ci.yaml` that
  filters pull requests by branch and path and runs on pushes to `main` but not
  `dev`; a gate job and a zizmor-action job on `ubuntu-latest`, the gate without
  `if: always()` that misses a job and doesn't fail on every result, and that,
  like another job, sets `continue-on-error`; and an unscheduled `audit.yaml`
  that runs `uv audit` but not cargo-deny's advisories check; and a
  `release.yaml` job that only checks formatting but leaves
  setup-rust-toolchain's cache on, beside jobs that build or turn it off, and
  stub `Cargo.toml` and `pyproject.toml` files.
  `precommit` has only a `.pre-commit-config.yaml`, which breaks the pre-commit
  standards: its `default_install_hook_types` lacks `pre-commit`, it runs only
  some of the pre-commit-hooks set, its `reuse` hook runs in the wrong stage,
  its conventional-pre-commit hook runs in the wrong stage with other commit
  types than the standard's, and among
  remote and local hooks whose IDs or aliases collide, some have aliases of
  their own and some can't be selected alone.
  `markdown` breaks the Markdown standards: its `.markdownlint-cli2.jsonc`
  names rules by alias and tag, turns some off and lets others only warn, and
  sets other options than the standard's; its markdownlint-cli2 hook has
  another ID, and its CI action lints only the root's files. Its `CLAUDE.md`
  names `AGENTS.md` beside it, but only in prose and a code span, which don't
  import it.
  `versioning` has Python components in subdirectories, checked against the
  versioning standards: one that conforms but lacks the `raw-options` a
  component below the root needs, one with a static version and another build
  backend, whose `.git_archival.txt` has the older `ref-names` form and isn't
  marked `export-subst`, and one uv doesn't build (`package = false`), which
  the versioning checks skip. Its `ci.yaml` has setup-uv steps that pin uv
  exactly, that pin no version in a job running `uv`, and that pin none in a
  job only running `uvx`, which needn't, and the components' `required-version`
  is a minimum the pin satisfies, an exact version, or a minimum above the pin.
  Its `.gitattributes` sets `diff=python` on `*.py` but not on `*.pyi`.
  `hooks` has language hooks and a `ci.yaml` that come close to the standards'
  hook and CI tables, beside stub `pyproject.toml` and `package.json` files: a
  `uv-lock` hook and CI step run through uv's global options, and of the jobs
  that `uv run`, one syncs with `--frozen`, one syncs with `--locked` only
  afterwards, and the others sync or run with `--locked`; neither the `pytest`
  hook nor the job running pytest passes `--cov`; `pnpm peers check` runs
  through `pnpm --dir . exec`; and the `eslint` hook passes `--max-warnings 10`
  while CI lints through the `lint` script, which runs plain `eslint .`. Its
  `pre-commit` job's `SKIP` lists some of the hooks with jobs of their own,
  but not `pytest` or `eslint`.
  `typescript-config` breaks the TypeScript configuration standards: its
  `package.json` sets `private` to the string `"true"`, and its two
  referenced tsconfigs lack, turn off or set to another value some of the
  required compiler options, one spelling `moduleDetection` in another case,
  which TypeScript accepts. Its `pnpm-workspace.yaml` lists `allowBuilds` as
  a sequence, keeps the older `onlyBuiltDependencies`, and lets unlisted
  dependencies' build scripts through. Its `eslint.config.ts` includes
  typescript-eslint's `recommendedTypeChecked` and an `Only` variant of
  `stylisticTypeChecked` rather than the standard's configs, comments out
  `strictTypeChecked`, sets `project` rather than `projectService`, and takes
  Prettier's config from `eslint-plugin-prettier`.
  Symlinks and `.gitignore` handling are covered by the unit tests in
  `detection.rs` instead: a fixture's `.gitignore` would also apply to this
  repository, so the files it ignores couldn't be committed.
- Fixtures are kept exactly as written: pre-commit, markdownlint and Tombi
  (`tombi.toml`) skip them, and `REUSE.toml` covers them, so one may be
  deliberately broken or lack headers. A fix or new check that depends on a
  project's layout gets a fixture.

## Template

- `copier.yml` holds the questions and post-copy `_tasks` (lock file
  generation); it `!include`s `versions.yml`, so it is excluded from the
  `check-yaml` hook.
- `versions.yml` holds every version the template pins, as hidden variables.
  Each entry's `# renovate:` comment feeds the regex managers in
  `renovate.json`; keep that comment format when adding entries. Actions are
  pinned as `"<sha> # <version>"` with a full `vX.Y.Z` tag, and use the
  `github-actions` versioning so Renovate keeps that tag in step with the
  workflows' pins instead of picking a floating `vX` tag on the same commit.
  `tests/versions.rs` fails if an action entry doesn't match the actions
  manager's pattern, lacks a full tag, or isn't pinned identically in
  `.github/workflows/`, so every template action must also be used there.
  A unit test in `src/config/markdownlint.rs` fails when
  `hook_markdownlint_cli2` changes: compare its `RULES` table with the rules of
  the markdownlint the new release depends on, then update
  `RULES_MARKDOWNLINT_CLI2` there. Nothing catches an update of
  `action_markdownlint_cli2` alone, whose markdownlint may differ, so review
  the rules for that too.
- Template paths use Jinja names for conditional files and per-language
  directories (`{{ rust_dir }}`, `{{ python_dir }}`, `{{ typescript_dir }}`),
  since any language can be primary (at the root) or secondary (in a
  subdirectory).
- When changing a standard, update `STANDARDS.md`, the template and, where
  relevant, the checks in `src/checks/` together.

## Conventions

- Every source file starts with SPDX headers
  (`MIT OR Apache-2.0`, `Jason Lynch <jason@aexoden.com>`, year created);
  Markdown and files without comments are covered by `REUSE.toml`.
- Markdown follows `.markdownlint-cli2.jsonc`: dash lists, prose wrapped at 80
  columns.
- Commits use Conventional Commits with the types listed in `STANDARDS.md`;
  the subject is imperative and lowercase, aiming for 50 characters and never
  over 72.
- TOML files are formatted by Tombi (the `tombi-format` hook), which also keeps
  dependencies in alphabetical order.
- Dependencies in `Cargo.toml` are pinned exactly (`=x.y.z`). New dependencies
  must have licenses allowed by `deny.toml`.
- `CHANGELOG.md` is maintained by hand in Keep a Changelog format, with no
  `Unreleased` section.
