// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! The project's license texts: the REUSE texts in `LICENSES/` and their copies at the root.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// The license files of a project, each keyed by file name and holding its contents.
#[derive(Debug, Default)]
pub(crate) struct LicenseFiles {
    /// The files in `LICENSES/`, such as `MIT.txt`. Hidden files and subdirectories are left out.
    pub texts: BTreeMap<String, Vec<u8>>,
    /// The root copies: `LICENSE` and each `LICENSE-*` file.
    pub copies: BTreeMap<String, Vec<u8>>,
    /// The license files that couldn't be read.
    pub errors: Vec<String>,
}

/// The name of the root copy when the project has a single license.
const SINGLE_COPY: &str = "LICENSE";

/// The prefix of each root copy when the project has several licenses.
const COPY_PREFIX: &str = "LICENSE-";

impl LicenseFiles {
    /// Read the license files of the project at `root`. A missing `LICENSES/` directory holds no texts.
    pub(crate) fn read(root: &Path) -> Self {
        let mut files = Self::default();

        files.texts = read_files(
            root,
            "LICENSES/",
            |name| !name.starts_with('.'),
            &mut files.errors,
        );
        files.copies = read_files(
            root,
            "",
            |name| name == SINGLE_COPY || name.starts_with(COPY_PREFIX),
            &mut files.errors,
        );

        files
    }

    /// Whether the root has a license copy.
    pub(crate) fn has_copy(&self) -> bool {
        !self.copies.is_empty()
    }

    /// Check that each text in `LICENSES/` has an identical copy at the root and each copy matches a text, as
    /// STANDARDS.md "License Files" requires: `LICENSE` for a single license, or one `LICENSE-*` file for each of
    /// several. Returns a description of each problem, so an empty list is a pass.
    pub(crate) fn copy_problems(&self) -> Vec<String> {
        let mut problems = self.errors.clone();

        // With no texts, the copies can't be named for them; each is reported as matching none below.
        if !self.texts.is_empty() {
            let single = self.texts.len() == 1;
            for name in self.copies.keys() {
                if single && name != SINGLE_COPY {
                    problems.push(format!(
                        "{name}: a single license's copy is named {SINGLE_COPY}"
                    ));
                } else if !single && name == SINGLE_COPY {
                    problems.push(format!(
                        "{name}: with several licenses, each copy is named {COPY_PREFIX}<description>"
                    ));
                }
            }
        }

        for (name, text) in &self.texts {
            if !self.copies.values().any(|copy| copy == text) {
                problems.push(format!("LICENSES/{name}: no identical copy at the root"));
            }
        }

        for (name, copy) in &self.copies {
            if !self.texts.values().any(|text| text == copy) {
                problems.push(format!("{name}: matches no text in LICENSES/"));
            }
        }

        problems
    }
}

/// Read each regular file directly in the directory `relative` (empty for the root itself, or ending in `/`) of the
/// project at `root` whose name `include` accepts, recording the files that can't be read by their paths relative to
/// the root. A directory that doesn't exist holds no files.
fn read_files(
    root: &Path,
    relative: &str,
    include: impl Fn(&str) -> bool,
    errors: &mut Vec<String>,
) -> BTreeMap<String, Vec<u8>> {
    let dir = root.join(relative);
    let label = if relative.is_empty() { "." } else { relative };
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return BTreeMap::new(),
        Err(e) => {
            errors.push(format!("{label}: could not read directory: {e}"));
            return BTreeMap::new();
        }
    };

    let mut files = BTreeMap::new();

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                errors.push(format!("{label}: could not read directory: {e}"));
                continue;
            }
        };

        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let path = entry.path();

        if !include(&name) || !path.is_file() {
            continue;
        }

        match fs::read(&path) {
            Ok(content) => {
                files.insert(name, content);
            }
            Err(e) => errors.push(format!("{relative}{name}: could not read file: {e}")),
        }
    }

    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(texts: &[(&str, &str)], copies: &[(&str, &str)]) -> LicenseFiles {
        let map = |entries: &[(&str, &str)]| {
            entries
                .iter()
                .map(|(name, content)| ((*name).to_owned(), content.as_bytes().to_vec()))
                .collect()
        };

        LicenseFiles {
            texts: map(texts),
            copies: map(copies),
            errors: Vec::new(),
        }
    }

    #[test]
    fn identical_copies_pass() {
        let none = Vec::<String>::new();
        assert_eq!(
            files(&[("MIT.txt", "mit")], &[("LICENSE", "mit")]).copy_problems(),
            none
        );
        assert_eq!(
            files(
                &[("Apache-2.0.txt", "apache"), ("MIT.txt", "mit")],
                &[("LICENSE-APACHE-2.0", "apache"), ("LICENSE-MIT", "mit")]
            )
            .copy_problems(),
            none
        );
    }

    #[test]
    fn copies_are_named_by_the_number_of_licenses() {
        assert_eq!(
            files(&[("MIT.txt", "mit")], &[("LICENSE-MIT", "mit")]).copy_problems(),
            ["LICENSE-MIT: a single license's copy is named LICENSE"]
        );
        assert_eq!(
            files(
                &[("Apache-2.0.txt", "apache"), ("MIT.txt", "mit")],
                &[("LICENSE", "mit"), ("LICENSE-APACHE-2.0", "apache")]
            )
            .copy_problems(),
            ["LICENSE: with several licenses, each copy is named LICENSE-<description>"]
        );
    }

    #[test]
    fn copies_must_match_the_texts() {
        assert_eq!(
            files(
                &[("Apache-2.0.txt", "apache"), ("MIT.txt", "mit")],
                &[("LICENSE-MIT", "mit\n"), ("LICENSE-OTHER", "other")]
            )
            .copy_problems(),
            [
                "LICENSES/Apache-2.0.txt: no identical copy at the root",
                "LICENSES/MIT.txt: no identical copy at the root",
                "LICENSE-MIT: matches no text in LICENSES/",
                "LICENSE-OTHER: matches no text in LICENSES/",
            ]
        );
        assert_eq!(
            files(&[], &[("LICENSE", "mit")]).copy_problems(),
            ["LICENSE: matches no text in LICENSES/"]
        );
    }

    #[test]
    fn reads_only_license_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("LICENSES/nested")).unwrap();
        for (path, content) in [
            ("LICENSES/MIT.txt", "mit"),
            ("LICENSES/.gitkeep", ""),
            ("LICENSES/nested/Other.txt", "other"),
            ("LICENSE", "mit"),
            ("LICENSE-MIT", "mit"),
            ("LICENSE.md", "mit"),
            ("README.md", "readme"),
        ] {
            fs::write(root.join(path), content).unwrap();
        }
        fs::create_dir_all(root.join("LICENSE-DIR")).unwrap();

        let files = LicenseFiles::read(root);
        assert_eq!(files.texts.keys().collect::<Vec<_>>(), ["MIT.txt"]);
        assert_eq!(
            files.copies.keys().collect::<Vec<_>>(),
            ["LICENSE", "LICENSE-MIT"]
        );
        assert_eq!(files.errors, Vec::<String>::new());
    }

    #[test]
    fn a_missing_licenses_directory_holds_no_texts() {
        let dir = tempfile::tempdir().unwrap();
        let files = LicenseFiles::read(dir.path());
        assert!(files.texts.is_empty());
        assert!(!files.has_copy());
        assert_eq!(files.errors, Vec::<String>::new());
    }
}
