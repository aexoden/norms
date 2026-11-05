// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `.prettierrc`, which Prettier reads as YAML or JSON.
//!
//! Only the subset of fields inspected by checks is modeled. All structs use `#[serde(default)]` so that missing keys
//! deserialize to `None` / empty rather than causing a parse error.

use serde::Deserialize;
use serde_json::Value;

/// Top-level `.prettierrc` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct PrettierConfig {
    /// The plugins Prettier loads, by package name or path. Kept unparsed, so that a malformed value fails only the
    /// check reading it.
    pub plugins: Option<Value>,
}

impl PrettierConfig {
    /// Whether `plugins` lists the given package by name.
    pub(crate) fn has_plugin(&self, name: &str) -> bool {
        matches!(&self.plugins, Some(Value::Array(plugins)) if plugins.iter().any(|p| p.as_str() == Some(name)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> PrettierConfig {
        serde_saphyr::from_str(text).unwrap()
    }

    #[test]
    fn plugins_are_matched_by_name() {
        let json = parse(r#"{ "plugins": ["prettier-plugin-packagejson"], "printWidth": 120 }"#);
        assert!(json.has_plugin("prettier-plugin-packagejson"));

        let yaml = parse("plugins:\n  - other\n  - prettier-plugin-packagejson\n");
        assert!(yaml.has_plugin("prettier-plugin-packagejson"));

        for text in [
            "printWidth: 120\n",
            "plugins: prettier-plugin-packagejson\n",
            "plugins: [./prettier-plugin-packagejson]\n",
            "plugins: [1]\n",
        ] {
            assert!(
                !parse(text).has_plugin("prettier-plugin-packagejson"),
                "{text}"
            );
        }
    }
}
