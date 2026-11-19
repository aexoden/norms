# norms

norms is a set of standards for software projects, and a tool that checks
projects against them:

- [`STANDARDS.md`](STANDARDS.md) describes the standards: repository layout,
  licensing, commit messages, Git hooks, CI, dependency management, and the
  tooling for Rust, Python and TypeScript.
- The `norms` command line checks a project, and its GitHub repository, against
  those standards.
- A [Copier](https://copier.readthedocs.io/) template renders new projects that
  already follow them.

The standards are opinionated, written for the author's own projects, and still
evolving.

## Usage

### Running norms in a Project

Projects run norms through [prek](https://prek.j178.dev/), which builds it from
source at the pinned release (installing Rust first if needed). Add the hooks to
`prek.toml`, with the settings that install the commit message hook type and
keep the other hooks out of its stage, as the standards'
[Git hooks](STANDARDS.md#git-hooks) do:

```toml
default_install_hook_types = ["pre-commit", "commit-msg"]
default_stages = ["pre-commit"]

[priorities]
lock = 100
check = 200

[[repos]]
repo = "https://github.com/aexoden/norms"
rev = "vX.Y.Z"

[[repos.hooks]]
id = "norms-commit-msg"

[[repos.hooks]]
id = "norms-verify"
args = ["--deny-warnings"]
priority = "check"
```

- `norms-verify` runs `norms verify` on the whole project before each commit.
- `norms-commit-msg` checks the commit message.

Projects rendered from the template already have both. To run norms directly,
install it with Cargo:

```sh
cargo install --locked --git https://github.com/aexoden/norms --tag vX.Y.Z
```

### Commands

`norms verify <path>` checks the project at `<path>`:

```sh
norms verify .
norms verify --deny-warnings --json .
```

`norms github <path>` checks the GitHub repository of the checkout at `<path>`
against the [repository standards](STANDARDS.md#github-repository). It reads
the settings through the [GitHub CLI](https://cli.github.com/), so `gh` must be
installed and logged in, and most settings are readable only with admin access.
`--repo OWNER/NAME` checks another repository. Nothing runs it automatically,
since CI's token can't read the settings.

`norms commit-msg <file>` checks a commit message: the type must be lowercase,
and the subject and body lines at most 72 characters (body lines holding a URL
are exempt). It exits with 1 if the message breaks either rule. With
`--long-lines warn` (or `NORMS_COMMIT_MSG_LENGTH=warn`), long lines only warn.

### Results

Each check has a stable ID, such as `cargo-toml-license`, and reports one of:

- **Failed**: a safeguard the standards require is missing or can't work, such
  as a missing file, an unparsable configuration, or a tool that never runs.
- **Warned**: the safeguard is there but differs from the standard (weaker
  settings, other flags or versions), norms can't be sure, or configuration is
  stale, such as an unneeded waiver.
- **Skipped**: the check is waived, or couldn't run, such as a GitHub setting
  the token can't read.
- **Passed**: the project meets the standard.

`norms github` fails on any setting that differs, since each setting is itself
the safeguard, except that Wikis or Projects being on, or a ruleset whose bypass
actors the token isn't shown, only warns. It skips what GitHub doesn't offer a
private repository without GitHub Pro, and the `Development` ruleset unless the
repository has a `dev` branch.

`norms verify` and `norms github` exit with 1 if any check failed, or, with
`--deny-warnings`, if any check failed or warned. `norms verify` also exits
with 1 if the path isn't a directory. `norms github` exits with 2 if it couldn't
check the repository at all: the path isn't a directory, `norms.toml` is
invalid, or `gh` can't read the repository.

`--json` prints the results as JSON instead: an object with the `path`, a
`summary` of the counts, and the `results`, each with its check's `id` and
`name`, the `component` it inspected if any, its `status` and any `message`.
`norms verify` adds the `languages` and `components` it found, and
`norms github` the `repository`.

### Configuration

A project that departs from a standard records why in `norms.toml`, at the
repository root. Most projects don't need one.

```toml
[components]
exclude = ["tests/fixtures"]

[[waivers]]
check = "gitattributes-binary"
reason = "The repository holds no binary files."
```

- `[components] exclude` lists directories, relative to the root, that hold
  marker files but no real components, such as test fixtures. norms searches
  them neither for components nor for `.yml` files, though a pnpm workspace's
  packages there are still found, as pnpm finds them. Each entry is a plain
  relative path, without `.` or `..`, and one that names no directory warns.
- Each `[[waivers]]` entry names a check ID and gives a reason, which is
  required. The check is then reported as skipped, with the reason; a check
  that inspects several files or components is waived for all of them. A check
  may be waived only once, and the checks of `norms.toml` itself not at all.
- A waiver whose check passes or doesn't run warns, so waivers don't outlive
  their need.
- Waivers of `github-*` checks apply to `norms github`; the rest apply to
  `norms verify`.
- Unknown keys are errors.

### Components

norms finds a project's components by their marker files, at the root or in
any subdirectory: `Cargo.toml` (Rust), `pyproject.toml` (Python), `package.json`
(TypeScript) and `CMakeLists.txt` (C++, detected but not yet checked). Each
component's own files are checked in its directory, and repository-wide files,
such as the workflows, once.

- A marker inside a component of the same language, such as a Cargo workspace
  member or a pnpm workspace package, belongs to that component. Components of
  different languages may nest.
- The search skips hidden directories, `.gitignore`d paths, `node_modules`,
  `target`, `.venv`, and the directories `[components] exclude` lists. It
  doesn't follow symlinked directories, or symlinked markers that lead outside
  the project, and a directory it can't read warns.
- The members of a Cargo workspace are found as Cargo finds them, and the
  packages of a pnpm workspace as pnpm does. A member outside the workspace, or
  a pattern norms can't follow, warns.

Whether a component is an [application or a
library](STANDARDS.md#applications-and-libraries) decides which standards
apply to it.

## Project Template

Create a project for any combination of Rust, Python and TypeScript:

```sh
uvx copier copy --trust gh:aexoden/norms <path>
```

The template asks for the primary language, which lives at the repository root,
any secondary languages and their subdirectories, and whether the project is an
application or a library. `--trust` lets it run each language's tool to generate
the lock files.

To bring a project up to date with the latest norms release, run this in the
project:

```sh
uvx copier update --trust
```

It merges the changes between the template version recorded in
`.copier-answers.yml` and the latest one. Template versions are norms release
tags (`vX.Y.Z`).

The template renders only the repository's files. Configure the GitHub
repository as [GitHub Repository](STANDARDS.md#github-repository) describes,
then check it with `norms github .`.

## Platform Support

norms is built and tested only on Linux, which includes WSL on Windows. That
covers the `norms` command line, its hooks, and the development tooling of the
projects the template renders. It may work on macOS or natively on Windows, but
nothing checks that it does, and on Windows some of what it reports names paths
with backslashes.

If you'd like to use norms on macOS or Windows,
[open an issue](https://github.com/aexoden/norms/issues), and it will get the
support and CI testing it needs.

## Versioning

norms follows [Semantic Versioning](https://semver.org/), applied to a linter
as [ESLint](https://github.com/eslint/eslint#semantic-versioning-policy) and
[Stylelint](https://stylelint.io/about/semantic-versioning/) apply it. Its
version numbers cover the `norms` command line (its commands, options, exit
codes, and the structure of its JSON output, but not the wording of a check's
`name` or `message`), the hooks, `norms.toml`, the check IDs, the template's
questions and what their answers mean, and which projects pass.

Every check currently runs by default, so:

- A major release may make a project that passed fail: through a new check that
  can fail, a stricter requirement in an existing one, a warning that becomes a
  failure, or a renamed check ID, which leaves the check's waivers behind. It
  may also change the command line, `norms.toml` or the template's questions
  incompatibly.
- A minor release may make a project fail through a bug fix, where a check
  didn't enforce what its standard says, and may add checks that only warn, as
  well as features.
- A patch release fixes bugs, and makes no project fail that passed.

A project passes when `norms verify` exits 0 without `--deny-warnings`, and its
GitHub repository when `norms github` does. A project that denies warnings, as
the standards' hook does, takes on the risk that a minor release adds one.
Removing a check isn't a breaking change, though a waiver of it then warns.
Changes to the files the template renders aren't breaking either, since
`copier update` presents them for review.

Before 1.0, as is common in Rust, a minor release (`0.X.0`) takes the place of a
major release, and a patch release takes the place of the others.

`CHANGELOG.md` marks each breaking change with **Breaking:**.

## Development

Install [prek](https://prek.j178.dev/installation/), then the hooks:

```sh
prek install
```

Rust uses [rustup](https://rustup.rs/), which installs the toolchain pinned in
`rust-toolchain.toml`. Install
[cargo-deny](https://embarkstudios.github.io/cargo-deny/#install-cargo-deny) for
the dependency policy hook, and
[cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov#installation) for the
test coverage hook.

`AGENTS.md` describes the repository's layout and commands in more detail.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE-2.0](LICENSE-APACHE-2.0) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <https://opensource.org/licenses/MIT>)

at your option.
