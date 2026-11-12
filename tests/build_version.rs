// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Tests of the build script that sets `BUILD_VERSION`, which the template gives its Rust applications too.

#[expect(
    dead_code,
    reason = "only the build script's version mapping is tested"
)]
#[path = "../build.rs"]
mod build;

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::build::version;

    #[test]
    fn maps_git_describe_to_a_version() {
        for (describe, expected) in [
            (Some("v0.2.0"), "0.2.0"),
            (Some("v0.2.0-dirty"), "0.2.0-dirty"),
            (Some("v0.2.0-5-gf8a4d2b"), "0.2.0-5-gf8a4d2b"),
            (Some("v0.2.0-5-gf8a4d2b-dirty"), "0.2.0-5-gf8a4d2b-dirty"),
            (Some("f8a4d2b"), "0.1.0+gf8a4d2b"),
            (Some("f8a4d2b-dirty"), "0.1.0+gf8a4d2b-dirty"),
            (None, "0.1.0"),
        ] {
            assert_eq!(version("0.1.0", describe), expected, "{describe:?}");
        }
    }

    /// The template's build script is norms' own, below its SPDX headers.
    #[test]
    fn the_template_build_script_is_norms_own() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let body = |path: &Path| {
            let text = fs::read_to_string(path).unwrap();
            text.lines().skip(2).collect::<Vec<_>>().join("\n")
        };

        assert_eq!(
            body(&root.join(
                "template/{{ rust_dir }}/{% if project_type == 'application' %}build.rs{% endif %}.jinja"
            )),
            body(&root.join("build.rs")),
            "the template's build.rs must match norms' own below the SPDX headers"
        );
    }
}
