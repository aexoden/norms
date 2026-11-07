// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Reading attributes from a `.gitattributes` file.

use std::path::Path;

use ignore::gitignore::GitignoreBuilder;

/// The file that lets a source archive without git history report its version, through the placeholders git fills in
/// when it creates the archive, if `.gitattributes` marks it `export-subst`.
pub(crate) const GIT_ARCHIVAL: &str = ".git_archival.txt";

/// The state the last line of a `.gitattributes` file that names an attribute gives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttributeState<'a> {
    /// `!name`, or no line naming the attribute: it is unspecified.
    Unspecified,
    /// `name`: the attribute is set.
    Set,
    /// `-name`: the attribute is unset.
    Unset,
    /// `name=value`: the attribute is set to a value.
    Value(&'a str),
}

/// The state the `.gitattributes` file in `dir`, whose text is `content`, gives the attribute `name` on the file at
/// `path`, which lies under `dir`.
///
/// As in git, each line is a pattern followed by attributes, the pattern matches as in `.gitignore` (a pattern without a
/// slash matches a file of that name in any directory), and the last line that matches the file and names the attribute
/// decides: `name` sets it, `name=value` sets it to `value`, `-name` unsets it and `!name` leaves it unspecified. Quoted
/// patterns, macro attributes other than git's own `binary`, and the negative patterns git rejects aren't read, and a
/// pattern matching a directory doesn't apply to the files inside it, as in git.
pub(crate) fn attribute<'a>(
    content: &'a str,
    dir: &Path,
    path: &Path,
    name: &str,
) -> AttributeState<'a> {
    let mut state = AttributeState::Unspecified;
    for line in content.lines() {
        let mut fields = line.split_whitespace();
        let Some(pattern) = fields.next() else {
            continue;
        };
        if pattern.starts_with(['#', '"', '!']) {
            continue;
        }
        let Some(line_state) = fields
            .rev()
            .find_map(|attribute| attribute_state(attribute, name))
        else {
            continue;
        };
        let mut builder = GitignoreBuilder::new(dir);
        if builder.add_line(None, pattern).is_err() {
            continue;
        }
        if builder
            .build()
            .is_ok_and(|matcher| matcher.matched(path, false).is_ignore())
        {
            state = line_state;
        }
    }
    state
}

/// Whether the `.gitattributes` file in `dir`, whose text is `content`, sets the attribute `name` on the file at
/// `path`, which lies under `dir`, with or without a value, as [`attribute`] reads it.
pub(crate) fn sets_attribute(content: &str, dir: &Path, path: &Path, name: &str) -> bool {
    matches!(
        attribute(content, dir, path, name),
        AttributeState::Set | AttributeState::Value(_)
    )
}

/// The state `attribute`, one of a line's attributes, gives the attribute `name`, or `None` if it isn't about `name`.
fn attribute_state<'a>(attribute: &'a str, name: &str) -> Option<AttributeState<'a>> {
    // `binary`, the one macro git defines itself, stands for `-diff -merge -text`.
    if attribute == "binary" && matches!(name, "diff" | "merge" | "text") {
        return Some(AttributeState::Unset);
    }
    if let Some(rest) = attribute.strip_prefix('-') {
        return (rest == name).then_some(AttributeState::Unset);
    }
    if let Some(rest) = attribute.strip_prefix('!') {
        return (rest == name).then_some(AttributeState::Unspecified);
    }
    match attribute.split_once('=') {
        Some((attribute_name, value)) => {
            (attribute_name == name).then_some(AttributeState::Value(value))
        }
        None => (attribute == name).then_some(AttributeState::Set),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn export_subst(content: &str, path: &str) -> bool {
        let dir = Path::new("/project");
        sets_attribute(content, dir, &dir.join(path), "export-subst")
    }

    #[test]
    fn last_matching_line_decides() {
        let archival = ".git_archival.txt";
        assert!(export_subst(".git_archival.txt  export-subst\n", archival));
        assert!(export_subst(
            ".git_archival.txt  export-subst\n",
            "api/.git_archival.txt"
        ));
        assert!(export_subst(
            "**/.git_archival.txt export-subst\n",
            "api/.git_archival.txt"
        ));
        assert!(export_subst(
            "/api/.git_archival.txt export-subst\n",
            "api/.git_archival.txt"
        ));
        assert!(export_subst(
            "api/* export-subst\n",
            "api/.git_archival.txt"
        ));
        assert!(export_subst(
            "* text=auto\n*.txt export-subst diff\n",
            archival
        ));
        assert!(export_subst(
            ".git_archival.txt -export-subst\n.git_archival.txt export-subst\n",
            archival
        ));

        assert!(!export_subst("", archival));
        assert!(!export_subst(
            "# .git_archival.txt export-subst\n",
            archival
        ));
        assert!(!export_subst(".git_archival.txt text\n", archival));
        assert!(!export_subst(
            ".git_archival.txt export-subst-other\n",
            archival
        ));
        assert!(!export_subst(
            "/api/.git_archival.txt export-subst\n",
            archival
        ));
        assert!(!export_subst(
            "/.git_archival.txt export-subst\n",
            "api/.git_archival.txt"
        ));
        assert!(!export_subst(
            "api/ export-subst\n",
            "api/.git_archival.txt"
        ));
        assert!(!export_subst(
            ".git_archival.txt export-subst\n*.txt -export-subst\n",
            archival
        ));
        assert!(!export_subst(
            ".git_archival.txt export-subst\n*.txt !export-subst\n",
            archival
        ));
        assert!(export_subst(
            ".git_archival.txt export-subst\n*.txt text\n",
            archival
        ));
    }

    fn diff<'a>(content: &'a str, path: &str) -> AttributeState<'a> {
        let dir = Path::new("/project");
        attribute(content, dir, &dir.join(path), "diff")
    }

    #[test]
    fn attribute_values() {
        assert_eq!(
            diff("*.py text diff=python\n", "src/main.py"),
            AttributeState::Value("python")
        );
        assert_eq!(
            diff("*.py diff=python\n*.py diff\n", "main.py"),
            AttributeState::Set
        );
        assert_eq!(
            diff("*.py diff=python\n*.py -diff\n", "main.py"),
            AttributeState::Unset
        );
        assert_eq!(
            diff("*.py diff=python\n*.py !diff\n", "main.py"),
            AttributeState::Unspecified
        );
        assert_eq!(
            diff("*.py diff=python\n", "main.pyi"),
            AttributeState::Unspecified
        );
        assert_eq!(
            diff("*.py difftool=python\n", "main.py"),
            AttributeState::Unspecified
        );
        assert_eq!(
            diff("*.py diff=python\n*.py binary\n", "main.py"),
            AttributeState::Unset
        );
        assert_eq!(
            diff("*.py binary\n*.py diff=python\n", "main.py"),
            AttributeState::Value("python")
        );
    }
}
