// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! `.editorconfig`, parsed by ec4rs so that the properties a file gets follow `EditorConfig`'s own rules for sections
//! and globs.

use std::fmt;

use ec4rs::{ConfigParser, Properties, PropertiesSource as _, Section};

/// A parsed `.editorconfig`.
pub(crate) struct EditorConfig {
    /// Whether the preamble sets `root = true`.
    root: bool,
    /// The sections, in the order they apply.
    sections: Vec<Section>,
}

impl EditorConfig {
    /// Parse the text of an `.editorconfig`.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let mut parser = ConfigParser::new_buffered(text.as_bytes())
            .map_err(|error| format!("{error} before the first section"))?;
        let root = parser.is_root;
        let sections = parser
            .by_ref()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("{error} at line {}", parser.line_no()))?;
        Ok(Self { root, sections })
    }

    /// Whether the preamble sets `root = true`, so that editors look for no `.editorconfig` above it.
    pub(crate) const fn is_root(&self) -> bool {
        self.root
    }

    /// The value, lowercased as `EditorConfig` reads known properties' values, that `key` has for a file at `path`,
    /// relative to the `.editorconfig`, after every section that applies to it, if any sets it.
    pub(crate) fn value(&self, path: &str, key: &str) -> Option<String> {
        let mut properties = Properties::new();
        // A section adds its properties infallibly.
        self.sections
            .iter()
            .try_for_each(|section| section.apply_to(&mut properties, path))
            .ok()?;
        properties
            .get_raw_for_key(key)
            .into_option()
            .map(str::to_ascii_lowercase)
    }
}

impl fmt::Debug for EditorConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EditorConfig")
            .field("root", &self.root)
            .field("sections", &self.sections.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_follow_sections_and_globs() {
        let config = EditorConfig::parse(
            "root = true\n\n[*]\nindent_size = 4\ncharset = UTF-8\n\n[*.{yml,yaml,json,toml}]\nindent_size = 2\n\n[src/**.rs]\nindent_size = 8\n",
        )
        .unwrap();
        assert!(config.is_root());
        assert_eq!(config.value("file", "indent_size").as_deref(), Some("4"));
        assert_eq!(config.value("file", "charset").as_deref(), Some("utf-8"));
        assert_eq!(config.value("a.yaml", "indent_size").as_deref(), Some("2"));
        assert_eq!(
            config.value("dir/a.toml", "indent_size").as_deref(),
            Some("2")
        );
        assert_eq!(
            config.value("src/a.rs", "indent_size").as_deref(),
            Some("8")
        );
        assert_eq!(config.value("a.rs", "indent_size").as_deref(), Some("4"));
        assert_eq!(config.value("file", "tab_width"), None);

        assert!(
            !EditorConfig::parse("[*]\nindent_size = 4\n")
                .unwrap()
                .is_root()
        );
        assert_eq!(
            EditorConfig::parse("[*]\nindent_size = 4\nstray\n")
                .err()
                .as_deref(),
            Some("invalid line at line 3")
        );
    }
}
