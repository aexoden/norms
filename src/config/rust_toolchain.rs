// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `rust-toolchain.toml`.

use serde::Deserialize;

/// The components every toolchain installs, which the hooks need, each with the other names rustup knows it by:
/// cargo-llvm-cov, which measures test coverage, needs `llvm-tools`, once named `llvm-tools-preview`.
const REQUIRED_COMPONENTS: [(&str, &[&str]); 3] = [
    ("clippy", &[]),
    ("rustfmt", &[]),
    ("llvm-tools", &["llvm-tools-preview"]),
];

/// Top-level `rust-toolchain.toml` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct RustToolchain {
    pub toolchain: Option<ToolchainSection>,
}

/// The `[toolchain]` table. Its fields are read leniently, so that one of the wrong type fails only its check.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct ToolchainSection {
    pub channel: Option<toml::Value>,
    pub components: Option<toml::Value>,
}

impl RustToolchain {
    /// Check that the channel is an exact release, `x.y.z`, rather than `stable`, a nightly or a minor version such as
    /// `1.99`, which rustup resolves to its latest patch release.
    pub(crate) fn check_channel(&self) -> Result<(), String> {
        let channel = self
            .toolchain
            .as_ref()
            .and_then(|t| t.channel.as_ref())
            .ok_or("No [toolchain] channel")?;

        match channel.as_str() {
            Some(version)
                if semver::Version::parse(version)
                    .is_ok_and(|v| v.pre.is_empty() && v.build.is_empty()) =>
            {
                Ok(())
            }
            _ => Err(format!("channel is {channel}, not an exact x.y.z release")),
        }
    }

    /// Check that the components include those the hooks need.
    pub(crate) fn check_components(&self) -> Result<(), String> {
        let components = self.toolchain.as_ref().and_then(|t| t.components.as_ref());
        let listed: Vec<&str> = match components {
            None => Vec::new(),
            Some(toml::Value::Array(items)) => {
                items.iter().filter_map(toml::Value::as_str).collect()
            }
            Some(other) => return Err(format!("components is {other}, not an array")),
        };

        let missing: Vec<&str> = REQUIRED_COMPONENTS
            .into_iter()
            .filter(|(component, aliases)| {
                !listed
                    .iter()
                    .any(|name| name == component || aliases.contains(name))
            })
            .map(|(component, _)| component)
            .collect();

        if missing.is_empty() {
            Ok(())
        } else {
            Err(format!("Missing components: {}", missing.join(", ")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toolchain(toml: &str) -> RustToolchain {
        toml::from_str(toml).unwrap()
    }

    #[test]
    fn channels_must_be_exact_releases() {
        assert_eq!(
            toolchain("[toolchain]\nchannel = \"1.99.1\"\n").check_channel(),
            Ok(())
        );

        for (channel, message) in [
            (
                "\"stable\"",
                "channel is \"stable\", not an exact x.y.z release",
            ),
            (
                "\"1.99\"",
                "channel is \"1.99\", not an exact x.y.z release",
            ),
            (
                "\"nightly-2026-11-01\"",
                "channel is \"nightly-2026-11-01\", not an exact x.y.z release",
            ),
            (
                "\"1.99.1-beta.1\"",
                "channel is \"1.99.1-beta.1\", not an exact x.y.z release",
            ),
            ("1", "channel is 1, not an exact x.y.z release"),
        ] {
            assert_eq!(
                toolchain(&format!("[toolchain]\nchannel = {channel}\n")).check_channel(),
                Err(message.to_owned())
            );
        }

        assert_eq!(
            toolchain("").check_channel(),
            Err("No [toolchain] channel".to_owned())
        );
    }

    #[test]
    fn components_must_include_clippy_rustfmt_and_llvm_tools() {
        for components in [
            r#"["rustfmt", "clippy", "llvm-tools", "rust-src"]"#,
            r#"["llvm-tools-preview", "clippy", "rustfmt"]"#,
        ] {
            assert_eq!(
                toolchain(&format!("[toolchain]\ncomponents = {components}\n")).check_components(),
                Ok(()),
                "{components}"
            );
        }
        assert_eq!(
            toolchain("[toolchain]\ncomponents = [\"rustfmt\", \"llvm-tools\"]\n")
                .check_components(),
            Err("Missing components: clippy".to_owned())
        );
        assert_eq!(
            toolchain("[toolchain]\ncomponents = [\"clippy\", \"rustfmt\"]\n").check_components(),
            Err("Missing components: llvm-tools".to_owned())
        );
        assert_eq!(
            toolchain("[toolchain]\nchannel = \"1.99.1\"\n").check_components(),
            Err("Missing components: clippy, rustfmt, llvm-tools".to_owned())
        );
        assert_eq!(
            toolchain("[toolchain]\ncomponents = \"clippy\"\n").check_components(),
            Err("components is \"clippy\", not an array".to_owned())
        );
    }
}
