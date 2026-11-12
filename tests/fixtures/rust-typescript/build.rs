// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Sets `BUILD_VERSION`, the version the program reports: `git describe`'s, from the release tags, so that a build
//! between releases names its commit, or else the package's own.
//!
//! - On a release tag, `0.2.0`.
//! - Past one, `0.2.0-5-gf8a4d2b`.
//! - In a clone without tags, such as a shallow one, the package's version and the commit: `0.2.0+gf8a4d2b`.
//! - Outside git, or from a package (`cargo package`, crates.io), the package's version: `0.2.0`.
//!
//! In git, the version ends in `-dirty` if the working tree had changes when the script last ran, which Cargo does
//! when the commit, the tags, the index, `Cargo.toml` or `src/` change, but not for other files.

use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    let package = env::var("CARGO_PKG_VERSION").unwrap_or_default();
    // A package carries its version, but a directory it is unpacked in may belong to another repository.
    let describe = if Path::new(".cargo_vcs_info.json").exists() {
        None
    } else {
        git_describe()
    };
    println!(
        "cargo::rustc-env=BUILD_VERSION={}",
        version(&package, describe.as_deref())
    );
}

/// The version to report, from the package's version, `package`, and `git describe --always`'s output, `describe`, if
/// git gave one: a release tag's description without its `v`, or else the commit it gives in place of one.
pub(crate) fn version(package: &str, describe: Option<&str>) -> String {
    describe.map_or_else(
        || package.to_owned(),
        |describe| {
            describe
                .strip_prefix('v')
                .map_or_else(|| format!("{package}+g{describe}"), str::to_owned)
        },
    )
}

/// `git describe`'s description of the checkout, from the release tags, or the commit if none is reachable. Cargo is
/// told to rerun the build script when that may change.
fn git_describe() -> Option<String> {
    let dirs = git(&[
        "rev-parse",
        "--path-format=absolute",
        "--git-dir",
        "--git-common-dir",
    ])?;
    let mut dirs = dirs.lines();
    let (git_dir, common_dir) = (Path::new(dirs.next()?), Path::new(dirs.next()?));

    // A path that doesn't exist, such as packed-refs before git packs any, would rerun the build script every time.
    // Repositories using the reftable format keep their refs in reftable/ rather than refs/ and packed-refs.
    for path in [
        git_dir.join("HEAD"),
        git_dir.join("index"),
        common_dir.join("refs"),
        common_dir.join("packed-refs"),
        common_dir.join("reftable"),
        "Cargo.toml".into(),
        "src".into(),
    ] {
        if path.exists() {
            println!("cargo::rerun-if-changed={}", path.display());
        }
    }

    git(&[
        "describe",
        "--tags",
        "--match=v[0-9]*",
        "--always",
        "--dirty",
    ])
    .map(|describe| describe.trim().to_owned())
}

/// The output of `git` run with `args`, if it succeeds. Git doesn't write the index it refreshes, which would rerun
/// the build script on the next build.
fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}
