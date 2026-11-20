# TODO

Open work on norms, in no particular order. Most of it waits on a change
elsewhere; each entry says what it waits on and what to do once it lands.

## Waiting on Upstream

- **Drop `jiti`.** ESLint loads `eslint.config.ts` through
  [jiti](https://github.com/unjs/jiti), which the template adds as a development
  dependency. Once ESLint loads it with Node's type stripping without a flag
  ([eslint/eslint#20903](https://github.com/eslint/eslint/issues/20903), likely
  in ESLint's next major release), drop `jiti` from the template and
  `STANDARDS.md`.
- **Check the TypeScript standard against TypeScript 7.** The template stays on
  TypeScript 6 until typescript-eslint supports TypeScript 7, which isn't
  expected before TypeScript 7.1. When the template moves to it, check the
  tsconfig standard and its checks for removed or renamed options and changed
  defaults, and whether the minimum ES year (`MIN_ES_YEAR`, ES2022) and the ES
  years that follow Vite and Node need to move.
- **Rename the Copier answers file to `.copier-answers.yaml`.** The standards
  name YAML files `.yaml`, except where a tool requires otherwise, and
  `copier update` finds the answers file only by its default name unless given
  `-a`. Once it finds `.copier-answers.yaml` on its own
  ([copier-org/copier#2196](https://github.com/copier-org/copier/pull/2196)),
  change `_answers_file` in `copier.yaml`, and rename the file wherever norms
  names it (`git grep -F .copier-answers.yml`), including its exemption from
  the `yaml-extension` check and from `STANDARDS.md`. Raise
  `_min_copier_version` to that release, and tell projects already rendered to
  rename their answers file, since `yaml-extension` would then warn on it.
- **Allow auto-merge again, if Renovate can use it safely.** The standards turn
  GitHub's **Allow auto-merge** off, and `norms github` checks it
  (`github-auto-merge`), because platform automerge waits only for required
  checks, not for statuses such as `renovate/artifacts` that Renovate waits for
  itself, so Renovate merges its own pull requests. If the shared Renovate
  preset goes back to platform automerge, once Renovate can hold a merge until
  its own statuses pass, turn the setting on in `STANDARDS.md` and the check.

## Tools to Watch

- **`uv audit`** is experimental. The standards' `audit.yaml` runs it
  (`uv audit --locked`), and it works against [OSV](https://osv.dev/). If it
  changes incompatibly before it is stable, update the template's `audit.yaml`,
  `STANDARDS.md` and the `python-audit-uv` check to match.
- **License detection in `LICENSES/`.** Projects keep a copy of each license
  text at the root, where GitHub looks for it, besides the REUSE copy in
  `LICENSES/`. If GitHub learns to detect licenses in `LICENSES/`, drop the root
  copies from `STANDARDS.md`, the template and the checks.

## Later

These matter little while norms checks only its author's projects, but would if
others adopt it.

- **Default and optional checks.** Every check runs by default, and a project
  opts out with waivers. Other linters enable a basic set by default, perhaps
  with a recommended set, and let a project enable more. Moving to that model
  would change what the version numbers promise (see Versioning in
  `README.md`).
- **Windows support.** When someone asks for it, add a `windows-latest` test
  job and fix what it finds. Known so far:
  - `tests/commit_msg.rs`'s `git` helper is used only under `cfg(unix)`, so it
    is dead code elsewhere, which fails clippy with `build.warnings="deny"`.
  - Messages that name a path through `Path::display()` show backslashes, such
    as those of `norms-toml-exclude-exists`, `gitattributes-git-archival` and
    a Cargo workspace member outside the workspace.
