// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Checks a commit message's line lengths, for the `norms-commit-msg` hook.

use std::sync::LazyLock;

use regex::Regex;

/// The longest a subject or body line may be, in characters.
pub(crate) const MAX_LINE_LENGTH: usize = 72;

/// The line below which `git commit --verbose` puts the diff, after the
/// comment string, which git removes from the message along with everything
/// after it.
const SCISSORS: &str = " ------------------------ >8 ------------------------";

/// The comment string git uses unless configured otherwise.
pub(crate) const DEFAULT_COMMENT: &str = "#";

/// The prefixes `git commit --fixup` and `--squash` put before the subject of
/// the commit they amend, which `git rebase --autosquash` removes.
const AUTOSQUASH_PREFIXES: [&str; 3] = ["fixup! ", "squash! ", "amend! "];

/// A URL, which can't be wrapped, so a body line holding one may be longer.
static URL: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r"(?i)\b[a-z][a-z0-9+.-]*://\S").unwrap()
});

/// A line of a commit message that is too long.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct LongLine {
    /// The line's number in the message file, from 1, or for the subject, the
    /// number of its first line.
    pub number: usize,
    /// Whether the line is the subject.
    pub subject: bool,
    /// The line's length in characters, less any autosquash prefixes of the
    /// subject.
    pub length: usize,
}

/// Returns the lines of a commit message, as git would record it, that are
/// longer than [`MAX_LINE_LENGTH`].
///
/// The message is read as git's default cleanup leaves it, with `comment` the
/// comment string. Lines starting with it are left out if the message was
/// `edited`, but kept if it wasn't, as with `git commit -m`. Everything from
/// the scissors line on is left out either way: git adds the line only to a
/// message it edits, but someone who sets `GIT_EDITOR=:` to skip the editor
/// makes an edited message look unedited to a hook. The subject is the first
/// paragraph, its lines joined by spaces as `git log` shows them. A subject's
/// autosquash prefixes don't count towards its length, a merge subject git
/// wrote (starting with `Merge `) may be of any length, as may a body line
/// holding a URL.
pub(crate) fn long_lines(message: &str, comment: &str, edited: bool) -> Vec<LongLine> {
    let mut long = Vec::new();
    // The number of the subject's first line and its lines, until it ends.
    let mut subject: Option<(usize, Vec<&str>)> = None;
    let mut in_body = false;
    let lines = message
        .lines()
        .enumerate()
        .take_while(|(_, line)| line.strip_prefix(comment) != Some(SCISSORS))
        .filter(|(_, line)| !(edited && line.starts_with(comment)));
    for (index, line) in lines {
        let number = index + 1;
        let line = line.trim_end();
        if in_body {
            let length = line.chars().count();
            if length > MAX_LINE_LENGTH && !URL.is_match(line) {
                long.push(LongLine {
                    number,
                    subject: false,
                    length,
                });
            }
        } else if let Some((_, subject_lines)) = &mut subject {
            if line.trim_start().is_empty() {
                in_body = true;
            } else {
                subject_lines.push(line);
            }
        } else if !line.trim_start().is_empty() {
            subject = Some((number, vec![line]));
        }
    }

    if let Some((number, subject_lines)) = subject {
        let joined = subject_lines.join(" ");
        let length = strip_autosquash_prefixes(&joined).chars().count();
        if length > MAX_LINE_LENGTH && !joined.starts_with("Merge ") {
            long.insert(
                0,
                LongLine {
                    number,
                    subject: true,
                    length,
                },
            );
        }
    }
    long
}

/// Returns a subject without the prefixes `git rebase --autosquash` removes,
/// which may be repeated.
fn strip_autosquash_prefixes(mut subject: &str) -> &str {
    while let Some(rest) = AUTOSQUASH_PREFIXES
        .iter()
        .find_map(|prefix| subject.strip_prefix(prefix))
    {
        subject = rest;
    }
    subject.trim_end()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The long lines of an edited message, with git's default comment string.
    fn edited(message: &str) -> Vec<LongLine> {
        long_lines(message, DEFAULT_COMMENT, true)
    }

    fn subject(number: usize, length: usize) -> LongLine {
        LongLine {
            number,
            subject: true,
            length,
        }
    }

    fn body(number: usize, length: usize) -> LongLine {
        LongLine {
            number,
            subject: false,
            length,
        }
    }

    #[test]
    fn accepts_lines_up_to_the_limit() {
        let message = format!("feat: {}\n\n{}\n", "a".repeat(66), "b".repeat(72));
        assert_eq!(edited(&message), []);
        assert_eq!(edited(""), []);
        assert_eq!(edited("\n\n"), []);
    }

    #[test]
    fn rejects_a_long_subject_and_body_lines() {
        let message = format!(
            "feat: {}\n\nshort\n{}\n{}\n",
            "a".repeat(67),
            "b".repeat(73),
            "c".repeat(80)
        );
        assert_eq!(edited(&message), [subject(1, 73), body(4, 73), body(5, 80)]);
    }

    #[test]
    fn counts_characters_rather_than_bytes() {
        let message = format!("docs: {}\n\n{}\n", "é".repeat(66), "—".repeat(72));
        assert_eq!(edited(&message), []);
    }

    #[test]
    fn ignores_trailing_whitespace_and_crlf() {
        let message = format!("feat: {}  \r\n\r\n{}\t\r\n", "a".repeat(66), "b".repeat(72));
        assert_eq!(edited(&message), []);
    }

    #[test]
    fn skips_leading_blank_lines_and_comments() {
        let message = format!(
            "\n# {}\n\nfeat: {}\n# {}\n",
            "x".repeat(80),
            "a".repeat(67),
            "y".repeat(80)
        );
        assert_eq!(edited(&message), [subject(4, 73)]);
    }

    #[test]
    fn stops_at_the_scissors_line() {
        let message = format!(
            "feat: add\n\n#{SCISSORS}\ndiff --git a/x b/x\n+{}\n",
            "a".repeat(80)
        );
        assert_eq!(edited(&message), []);
    }

    #[test]
    fn keeps_comment_lines_of_a_message_not_edited() {
        let message = format!(
            "#{} subject\n\n# {}\n#{SCISSORS}\n+{}\n",
            "1".repeat(70),
            "b".repeat(80),
            "c".repeat(80)
        );
        assert_eq!(
            long_lines(&message, DEFAULT_COMMENT, false),
            [subject(1, 79), body(3, 82)]
        );
    }

    #[test]
    fn uses_the_comment_string_given() {
        let message = format!(
            "feat: add\n\n# {}\n; {}\n;{SCISSORS}\n+{}\n",
            "a".repeat(80),
            "b".repeat(80),
            "c".repeat(80)
        );
        assert_eq!(long_lines(&message, ";", true), [body(3, 82)]);
        // A comment string may be longer than a character.
        let slashes = format!("feat: add\n\n// {}\n/ {}\n", "a".repeat(80), "b".repeat(80));
        assert_eq!(long_lines(&slashes, "//", true), [body(4, 82)]);
        // The scissors line is only git's when it starts with the comment string.
        let hash_scissors = format!("feat: add\n\n#{SCISSORS}\n{}\n", "c".repeat(80));
        assert_eq!(long_lines(&hash_scissors, ";", true), [body(4, 80)]);
    }

    #[test]
    fn exempts_body_lines_with_urls() {
        let url = format!("https://example.com/{}", "a".repeat(80));
        let message = format!(
            "fix: handle it\n\nSee {url}\n[1]: <{url}>\nftp://{}\n",
            "b".repeat(80)
        );
        assert_eq!(edited(&message), []);

        let not_url = format!("fix: handle it\n\nnot a url: :// {}\n", "c".repeat(70));
        assert_eq!(edited(&not_url), [body(3, 85)]);
    }

    #[test]
    fn joins_the_lines_of_the_subject() {
        let long = format!("feat: {}\n{}\n\nbody\n", "a".repeat(40), "b".repeat(40));
        assert_eq!(edited(&long), [subject(1, 87)]);
        let short = format!(
            "\nfeat: {}\n# comment\n{}\n",
            "a".repeat(30),
            "b".repeat(30)
        );
        assert_eq!(edited(&short), []);
    }

    #[test]
    fn exempts_merge_subjects() {
        let merge = format!(
            "Merge branch 'dev' of https://example.com/{} into dev\n",
            "a".repeat(60)
        );
        assert_eq!(edited(&merge), []);
        let body_line = format!("Merge branch 'dev'\n\n{}\n", "b".repeat(73));
        assert_eq!(edited(&body_line), [body(3, 73)]);
    }

    #[test]
    fn checks_subjects_with_urls() {
        let message = format!("docs: link https://example.com/{}\n", "a".repeat(60));
        assert_eq!(edited(&message), [subject(1, 91)]);
    }

    #[test]
    fn leaves_out_autosquash_prefixes() {
        let original = format!("feat: {}", "a".repeat(66));
        for prefix in [
            "fixup! ",
            "squash! ",
            "amend! ",
            "fixup! fixup! ",
            "amend! squash! ",
        ] {
            assert_eq!(edited(&format!("{prefix}{original}\n")), [], "{prefix}");
        }
        assert_eq!(edited(&format!("fixup! {original}a\n")), [subject(1, 73)]);
        // Only a prefix git adds is removed.
        assert_eq!(edited(&format!("Fixup! {original}\n")), [subject(1, 79)]);
    }
}
