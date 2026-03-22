use crate::comments::Comment;
use regex::Regex;
use std::sync::LazyLock;

/// The kind of check annotation.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum CheckKind {
    /// A check scoped to the nearby code (function, block, etc.).
    Check,
    /// A file-level check — applies whenever the file has any changes.
    File,
    /// An all-files check — activates when files matching the glob pattern change.
    All {
        pattern: String,
    },
    /// A tagged check — activates when any other [check:tag] with the same name activates.
    Tag {
        name: String,
    },
    /// A tag reference — like Tag but carries no description of its own.
    Ref {
        name: String,
    },
}

/// A parsed [check] annotation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Annotation {
    /// The file this annotation was found in.
    pub file_path: String,
    /// 1-based line number.
    pub line: usize,
    /// What kind of check this is.
    pub kind: CheckKind,
    /// The check description.
    pub description: String,
}

/// Matches `[check]`, `[check:file]`, or `[check:all <pattern>]`
/// at the start of the comment text, followed by optional description text.
static CHECK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[check(?::(\w+))?\]\s*(.*)").unwrap());

/// Matches `[check:all <pattern>]` at the start of the comment text.
static ALL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[check:all\s+([^\]]+)\]\s*(.*)").unwrap());

/// Matches `[check:tag TagName]` at the start of the comment text.
static TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[check:tag\s+(\S+)\]\s*(.*)").unwrap());

/// Matches `[check:ref TagName]` at the start of the comment text.
static REF_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[check:ref\s+(\S+)\]").unwrap());

/// Extract annotations from a list of comments.
///
/// Consecutive comments immediately following a `[check]` line are treated as
/// continuation lines and appended to the description. A blank comment line
/// is preserved as a newline. The annotation ends when the next comment is
/// non-consecutive (gap in line numbers), contains a new `[check]`, or when
/// a non-comment line intervenes (which means the comments are
/// non-consecutive).
pub fn extract_annotations(comments: &[Comment], file_path: &str) -> Vec<Annotation> {
    let mut annotations = Vec::new();
    let mut i = 0;

    while i < comments.len() {
        let comment = &comments[i];

        // Try [check:all] pattern first (more specific regex)
        if let Some(caps) = ALL_RE.captures(&comment.text) {
            let pattern = caps[1].trim().to_string();
            let first_line_desc = caps[2].trim().to_string();
            let mut description = first_line_desc;
            let mut prev_line = comment.line;

            i += 1;
            consume_continuation(comments, &mut i, &mut prev_line, &mut description);

            annotations.push(Annotation {
                file_path: file_path.to_string(),
                line: comment.line,
                kind: CheckKind::All { pattern },
                description,
            });
            continue;
        }

        // Try [check:tag Name] before generic [check] regex
        if let Some(caps) = TAG_RE.captures(&comment.text) {
            let name = caps[1].to_string();
            let first_line_desc = caps[2].trim().to_string();
            let mut description = first_line_desc;
            let mut prev_line = comment.line;

            i += 1;
            consume_continuation(comments, &mut i, &mut prev_line, &mut description);

            annotations.push(Annotation {
                file_path: file_path.to_string(),
                line: comment.line,
                kind: CheckKind::Tag { name },
                description,
            });
            continue;
        }

        // Try check:ref before generic check regex
        if let Some(caps) = REF_RE.captures(&comment.text) {
            let name = caps[1].to_string();

            i += 1;

            annotations.push(Annotation {
                file_path: file_path.to_string(),
                line: comment.line,
                kind: CheckKind::Ref { name },
                description: String::new(),
            });
            continue;
        }

        if let Some(caps) = CHECK_RE.captures(&comment.text) {
            let kind = match caps.get(1).map(|m| m.as_str()) {
                Some("file") => CheckKind::File,
                Some("all") => {
                    // This shouldn't happen since ALL_RE should match first,
                    // but handle it gracefully
                    eprintln!(
                        "warning: [check:all] without pattern in {}:{}",
                        file_path, comment.line
                    );
                    i += 1;
                    continue;
                }
                None => CheckKind::Check,
                Some(other) => {
                    eprintln!(
                        "warning: unknown [check] variant '{}' in {}:{}",
                        other, file_path, comment.line
                    );
                    i += 1;
                    continue;
                }
            };

            let first_line_desc = caps[2].trim().to_string();
            let mut description = first_line_desc;
            let mut prev_line = comment.line;

            i += 1;
            consume_continuation(comments, &mut i, &mut prev_line, &mut description);

            annotations.push(Annotation {
                file_path: file_path.to_string(),
                line: comment.line,
                kind,
                description,
            });
            continue;
        }
        i += 1;
    }

    annotations
}

fn consume_continuation(
    comments: &[Comment],
    i: &mut usize,
    prev_line: &mut usize,
    description: &mut String,
) {
    while *i < comments.len() {
        let next = &comments[*i];
        if next.line != *prev_line + 1 {
            break;
        }
        if CHECK_RE.is_match(&next.text) || ALL_RE.is_match(&next.text) || TAG_RE.is_match(&next.text) || REF_RE.is_match(&next.text) {
            break;
        }
        if !description.is_empty() {
            description.push('\n');
        }
        description.push_str(&next.text);
        *prev_line = next.line;
        *i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment(line: usize, text: &str) -> Comment {
        Comment {
            line,
            text: text.to_string(),
        }
    }

    #[test]
    fn parse_check() {
        let comments = vec![comment(5, "[check] verify bounds are checked")];
        let anns = extract_annotations(&comments, "src/lib.rs");
        assert_eq!(anns.len(), 1);
        assert_eq!(anns[0].kind, CheckKind::Check);
        assert_eq!(anns[0].description, "verify bounds are checked");
        assert_eq!(anns[0].line, 5);
    }

    #[test]
    fn parse_file_check() {
        let comments = vec![comment(1, "[check:file] all exports must have docs")];
        let anns = extract_annotations(&comments, "src/lib.rs");
        assert_eq!(anns.len(), 1);
        assert_eq!(anns[0].kind, CheckKind::File);
        assert_eq!(anns[0].description, "all exports must have docs");
    }

    #[test]
    fn parse_all_check() {
        let comments = vec![comment(3, "[check:all src/**/*.rs] Update examples")];
        let anns = extract_annotations(&comments, "README.md");
        assert_eq!(anns.len(), 1);
        assert_eq!(
            anns[0].kind,
            CheckKind::All {
                pattern: "src/**/*.rs".to_string()
            }
        );
        assert_eq!(anns[0].description, "Update examples");
        assert_eq!(anns[0].file_path, "README.md");
    }

    #[test]
    fn parse_all_multiline() {
        let comments = vec![
            comment(1, "[check:all *.proto] Regenerate protobuf bindings."),
            comment(2, "Check that the migration guide is updated."),
        ];
        let anns = extract_annotations(&comments, "docs/proto.md");
        assert_eq!(anns.len(), 1);
        assert_eq!(
            anns[0].description,
            "Regenerate protobuf bindings.\nCheck that the migration guide is updated."
        );
    }

    #[test]
    fn ignores_non_check_comments() {
        let comments = vec![
            comment(1, "this is a regular comment"),
            comment(2, "TODO: fix this"),
            comment(3, "[check] verify something"),
        ];
        let anns = extract_annotations(&comments, "foo.rs");
        assert_eq!(anns.len(), 1);
        assert_eq!(anns[0].line, 3);
    }

    #[test]
    fn unknown_variant_warns_and_skips() {
        let comments = vec![comment(1, "[check:bogus] something")];
        let anns = extract_annotations(&comments, "foo.rs");
        assert_eq!(anns.len(), 0);
    }

    #[test]
    fn multiline_continuation() {
        let comments = vec![
            comment(5, "[check] Make sure this handles all three cases:"),
            comment(6, "1. User has no subscription"),
            comment(7, "2. User has an expired subscription"),
            comment(8, "3. User has a valid subscription but rate-limited"),
        ];
        let anns = extract_annotations(&comments, "src/lib.rs");
        assert_eq!(anns.len(), 1);
        assert_eq!(anns[0].line, 5);
        assert_eq!(
            anns[0].description,
            "Make sure this handles all three cases:\n\
             1. User has no subscription\n\
             2. User has an expired subscription\n\
             3. User has a valid subscription but rate-limited"
        );
    }

    #[test]
    fn multiline_with_blank_comment_line() {
        let comments = vec![
            comment(1, "[check:file] This module is used by billing."),
            comment(2, "Any signature change needs a corresponding"),
            comment(3, "change in billing/client.py."),
            comment(4, ""),
            comment(5, "Also bump the version in the API schema."),
        ];
        let anns = extract_annotations(&comments, "src/api.rs");
        assert_eq!(anns.len(), 1);
        assert_eq!(
            anns[0].description,
            "This module is used by billing.\n\
             Any signature change needs a corresponding\n\
             change in billing/client.py.\n\
             \n\
             Also bump the version in the API schema."
        );
    }

    #[test]
    fn multiline_stops_at_gap() {
        let comments = vec![
            comment(5, "[check] first check"),
            comment(6, "continuation"),
            comment(10, "unrelated comment"),
        ];
        let anns = extract_annotations(&comments, "foo.rs");
        assert_eq!(anns.len(), 1);
        assert_eq!(anns[0].description, "first check\ncontinuation");
    }

    #[test]
    fn multiline_stops_at_new_check() {
        let comments = vec![
            comment(5, "[check] first check"),
            comment(6, "continuation of first"),
            comment(7, "[check] second check"),
            comment(8, "continuation of second"),
        ];
        let anns = extract_annotations(&comments, "foo.rs");
        assert_eq!(anns.len(), 2);
        assert_eq!(anns[0].description, "first check\ncontinuation of first");
        assert_eq!(anns[0].line, 5);
        assert_eq!(anns[1].description, "second check\ncontinuation of second");
        assert_eq!(anns[1].line, 7);
    }

    #[test]
    fn single_line_still_works() {
        let comments = vec![
            comment(1, "some comment"),
            comment(5, "[check] simple check"),
            comment(10, "another comment"),
        ];
        let anns = extract_annotations(&comments, "foo.rs");
        assert_eq!(anns.len(), 1);
        assert_eq!(anns[0].description, "simple check");
    }

    #[test]
    fn description_on_next_line() {
        let comments = vec![
            comment(5, "[check]"),
            comment(6, "The description starts here"),
            comment(7, "and continues here."),
        ];
        let anns = extract_annotations(&comments, "foo.rs");
        assert_eq!(anns.len(), 1);
        assert_eq!(
            anns[0].description,
            "The description starts here\nand continues here."
        );
    }

    #[test]
    fn does_not_match_tagref() {
        let comments = vec![
            comment(1, "[tag:FooBar] This is a tagref, not a check"),
            comment(2, "[ref:FooBar]"),
            comment(3, "[check] This is a check"),
        ];
        let anns = extract_annotations(&comments, "foo.rs");
        assert_eq!(anns.len(), 1);
        assert_eq!(anns[0].line, 3);
    }

    #[test]
    fn parse_tag_check() {
        let comments = vec![comment(5, "[check:tag FooBar] Keep in sync")];
        let anns = extract_annotations(&comments, "src/lib.rs");
        assert_eq!(anns.len(), 1);
        assert_eq!(
            anns[0].kind,
            CheckKind::Tag {
                name: "FooBar".to_string()
            }
        );
        assert_eq!(anns[0].description, "Keep in sync");
        assert_eq!(anns[0].line, 5);
    }

    #[test]
    fn parse_tag_multiline() {
        let comments = vec![
            comment(1, "[check:tag SyncPoint] These must match."),
            comment(2, "Update both sides when changing either."),
        ];
        let anns = extract_annotations(&comments, "src/a.rs");
        assert_eq!(anns.len(), 1);
        assert_eq!(
            anns[0].kind,
            CheckKind::Tag {
                name: "SyncPoint".to_string()
            }
        );
        assert_eq!(
            anns[0].description,
            "These must match.\nUpdate both sides when changing either."
        );
    }

    #[test]
    fn tag_stops_continuation_at_new_check() {
        let comments = vec![
            comment(1, "[check:tag Foo] first"),
            comment(2, "continuation"),
            comment(3, "[check:tag Bar] second"),
        ];
        let anns = extract_annotations(&comments, "foo.rs");
        assert_eq!(anns.len(), 2);
        assert_eq!(
            anns[0].kind,
            CheckKind::Tag {
                name: "Foo".to_string()
            }
        );
        assert_eq!(anns[0].description, "first\ncontinuation");
        assert_eq!(
            anns[1].kind,
            CheckKind::Tag {
                name: "Bar".to_string()
            }
        );
        assert_eq!(anns[1].description, "second");
    }

    #[test]
    fn parse_ref_check() {
        let comments = vec![comment(5, "[check:ref FooBar]")];
        let anns = extract_annotations(&comments, "src/lib.rs");
        assert_eq!(anns.len(), 1);
        assert_eq!(
            anns[0].kind,
            CheckKind::Ref {
                name: "FooBar".to_string()
            }
        );
        assert_eq!(anns[0].description, "");
        assert_eq!(anns[0].line, 5);
    }

    #[test]
    fn ref_ignores_continuation() {
        let comments = vec![
            comment(1, "[check:ref Sync]"),
            comment(2, "This comment is not part of the ref"),
            comment(3, "[check] This is a separate check"),
        ];
        let anns = extract_annotations(&comments, "foo.rs");
        assert_eq!(anns.len(), 2);
        assert_eq!(
            anns[0].kind,
            CheckKind::Ref {
                name: "Sync".to_string()
            }
        );
        assert_eq!(anns[0].description, "");
        assert_eq!(anns[1].kind, CheckKind::Check);
        assert_eq!(anns[1].line, 3);
    }

    #[test]
    fn ref_stops_tag_continuation() {
        let comments = vec![
            comment(1, "[check:tag Foo] description"),
            comment(2, "continuation"),
            comment(3, "[check:ref Bar]"),
        ];
        let anns = extract_annotations(&comments, "foo.rs");
        assert_eq!(anns.len(), 2);
        assert_eq!(anns[0].description, "description\ncontinuation");
        assert_eq!(
            anns[1].kind,
            CheckKind::Ref {
                name: "Bar".to_string()
            }
        );
    }
}
