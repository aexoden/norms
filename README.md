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
uvx copier copy --trust --vcs-ref main gh:aexoden/norms <path>
```

Run `uvx copier update --trust --vcs-ref main` in a project to bring it up to
date with the latest template. Every version the template uses is kept in
`versions.yml`.

The `--vcs-ref main` option is needed until norms tags a release that includes
the template; without it, Copier uses the latest tag, `v0.1.0`, which predates
the template.

## License

Licensed under either of

- Apache License, Version 2.0, ([LICENSE-APACHE-2.0](LICENSE-APACHE-2.0) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
