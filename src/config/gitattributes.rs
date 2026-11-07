// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Reading attributes from a `.gitattributes` file.

use std::path::Path;

use ignore::gitignore::GitignoreBuilder;

/// The file that lets a source archive without git history report its version, through the placeholders git fills in
/// when it creates the archive, if `.gitattributes` marks it `export-subst`.
pub(crate) const GIT_ARCHIVAL: &str = ".git_archival.txt";

/// Whether the `.gitattributes` file in `dir`, whose text is `content`, sets the attribute `name` on the file at
/// `path`, which lies under `dir`.
///
/// As in git, each line is a pattern followed by attributes, the pattern matches as in `.gitignore` (a pattern without a
/// slash matches a file of that name in any directory), and the last line that matches the file and names the attribute
/// decides: `name` or `name=value` sets it, `-name` unsets it and `!name` leaves it unspecified. Quoted patterns, macro
/// attributes and the negative patterns git rejects aren't read, and a pattern matching a directory doesn't apply to the
/// files inside it, as in git.
pub(crate) fn sets_attribute(content: &str, dir: &Path, path: &Path, name: &str) -> bool {
    let mut set = false;
    for line in content.lines() {
        let mut fields = line.split_whitespace();
        let Some(pattern) = fields.next() else {
            continue;
        };
        if pattern.starts_with(['#', '"', '!']) {
            continue;
        }
        let Some(state) = fields
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
            set = state;
        }
    }
    set
}

/// Whether `attribute`, one of a line's attributes, sets the attribute `name`, or `None` if it isn't about `name`.
fn attribute_state(attribute: &str, name: &str) -> Option<bool> {
    if let Some(rest) = attribute.strip_prefix(['-', '!']) {
        return (rest == name).then_some(false);
    }
    let attribute_name = attribute.split_once('=').map_or(attribute, |(n, _)| n);
    (attribute_name == name).then_some(true)
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
}
