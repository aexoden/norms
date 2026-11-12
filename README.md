# norms

`norms` is both a set of guidelines and standards for programming projects as
well as a tool to help implement and enforce them.

At this time, it is a highly opinionated tool primarily intended to help me
standardize the tooling on my various projects. It is also experimental, in that
I have not yet determined the final standards for my projects.

Standards intended to be verified and enforced are listed in `STANDARDS.md`.

## Project Template

norms also provides a [Copier](https://copier.readthedocs.io/) template that
renders a new project following the standards, for Rust, Python and TypeScript
in any combination:

```sh
uvx copier copy --trust gh:aexoden/norms <path>
```

Run `uvx copier update --trust` in a project to bring it up to date with the
template of the latest norms release. Every version the template uses is kept in
`versions.yml`.

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

A project passes when `norms verify` exits 0 without `--deny-warnings`. A
project that denies warnings, as the standards' hook does, takes on the risk
that a minor release adds one. Removing a check isn't a breaking change, though
a waiver of it then warns. Changes to the files the template renders aren't
breaking either, since `copier update` presents them for review.

Before 1.0, as is common in Rust, a minor release (`0.X.0`) takes the place of a
major release, and a patch release takes the place of the others.

`CHANGELOG.md` marks each breaking change with **Breaking:**.

## Development

Install [pre-commit](https://pre-commit.com/#install), then the hooks:

```sh
pre-commit install
```

Rust uses [rustup](https://rustup.rs/), which installs the toolchain pinned in
`rust-toolchain.toml`. Install
[cargo-deny](https://embarkstudios.github.io/cargo-deny/#install-cargo-deny) for
the dependency policy hook, and
[cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov#installation) for the
test coverage hook.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE-2.0](LICENSE-APACHE-2.0) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or
  <https://opensource.org/licenses/MIT>)

at your option.
