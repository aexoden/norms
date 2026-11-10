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
`--vcs-ref HEAD`, so commit first; answers are passed with `--data`). The
render's `norms-verify` hook would build the released norms the template pins,
so the job skips it and runs this checkout's norms instead. pre-commit still
clones the pinned release, which in a release commit isn't tagged yet, so the
job points the render's norms hooks at this checkout first:

```sh
git init /tmp/demo
uvx copier copy --trust --defaults --vcs-ref HEAD \
  --data project_name=demo-project --data "description=A demo." \
  --data primary_language=rust . /tmp/demo
git -C /tmp/demo add --all
(cd /tmp/demo && SKIP=norms-verify uvx pre-commit run --all-files)
cargo run -- verify --deny-warnings /tmp/demo
```

## Architecture of the CLI

`cli::verify` builds a `ProjectContext`, runs `checks::run_all_checks`, and
prints a `Report`, exiting 1 if any check failed, or with `--deny-warnings` if
any check failed or warned.

- `context.rs`: `ProjectContext` lazily parses each config file at most once
  (`OnceLock`) and returns a `ConfigStatus<T>` (`NotFound`, `ParseError` with
  the error, or `Ok`). Checks share parsed data through it rather than reading
  files themselves. It holds the repository-wide files and the detected
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
  `checks/runner.rs`. `common/` runs for every project: `files.rs` checks the
  repository's own files, `precommit.rs` and `ci.rs` the hooks and workflows
  every project has. For each detected language, the component checks run once
  per component of that language (inside `Report::with_component`, which labels
  their results with the component's path), then the checks of the language's
  place in repository-wide files (pre-commit hooks, CI) run once.
- `report.rs`: `report.must(id, name, outcome)` records a check whose issue is
  a failure, `report.should(...)` one whose issue is a warning, and
  `report.check(...)` one whose issues differ in severity, each an `Issue` that
  fails or warns (its Severity section gives the rule for choosing);
  `require_parsed` unwraps a `ConfigStatus` while recording a failure if it's
  missing or malformed. Every check has a stable kebab-case ID (subject, then
  property: `cargo-toml-license`; see `report.rs`), so don't rename an ID once
  projects may depend on it; the name beside it is free text.
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
  norms' own tags. It also reads the comment string for `norms commit-msg`,
  leaving those variables as git set them for the hook. `cli.rs` tells
  `commit_msg.rs` whether git opened an editor (it runs the hook with
  `GIT_EDITOR=:` when it didn't), which decides whether comment lines are kept.

CI's `template` job runs `norms verify --deny-warnings` on each render, so a
render must have no failures or warnings.

## Tests

Unit tests sit beside the code they test. `tests/verify.rs` checks
`norms verify`'s exit codes on a copy of a fixture. `tests/fixtures.rs` runs
`norms verify --json` on each project in `tests/fixtures/` and compares what it
reports with `tests/snapshots/<name>.txt`, so any change in findings shows up as
a snapshot diff. When the change is intended, rerun with `NORMS_BLESS=1`, review
`git diff tests/snapshots`, and commit the snapshots with the change.

- `tests/README.md` records what each fixture is for and how the language
  fixtures are rendered from the template. Read it before adding or changing
  one, to find the fixture that already covers the area, and update it in the
  same commit.
- Re-render the language fixtures when the template changes in a way the checks
  see.
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
- `CHANGELOG.md` is maintained by hand in Keep a Changelog format, as the
- `CHANGELOG.md` is maintained by hand in Keep a Changelog format, as the
  Changelog section of `STANDARDS.md` describes. A change that users of norms
  or the template would notice gets an entry under `Unreleased`, in the commit
  that makes it and under the type of change it is, written for those users
  rather than as the commit's subject. When a later change alters or reverts
  something still under `Unreleased`, update or drop its entry; a released
  version's entries stay as they are, and the later change gets an entry of its
  own. A change that breaks what the version numbers cover (see `README.md`)
  starts with `**Breaking:**` and says briefly how to upgrade.
- At a release, check `Unreleased` against the commits since the last tag for
  anything it missed, rename it to the new version with the date, add an empty
  `Unreleased` above it, and update the link definitions: the new version
  compares the previous tag with the new one, and `Unreleased` the new tag with
  `HEAD`. In the same commit, set `hook_norms` in `versions.yml` to the new tag,
  so that projects rendered from the release run its hooks (`tests/versions.rs`
  fails until it matches the latest release in `CHANGELOG.md`).
