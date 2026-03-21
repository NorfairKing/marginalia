use crate::annotations::{Annotation, CheckKind};
use regex::Regex;
use std::sync::LazyLock;

/// Parse a `.marginalia` file into annotations.
///
/// Syntax:
/// ```text
/// # Lines starting with # are comments.
/// # Blank lines are ignored.
///
/// when src/**/*.rs changes:
///   Make sure the README examples still compile.
///
/// when *.proto changes:
///   Regenerate protobuf bindings.
///   Check that the migration guide is updated.
/// ```
///
/// A `when <pattern> changes:` line starts a rule. Subsequent indented
/// lines form the description. The description ends at a blank line,
/// a new `when` line, or end of file.
pub fn parse_watchfile(content: &str, file_path: &str) -> Vec<Annotation> {
    let mut annotations = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            i += 1;
            continue;
        }

        if let Some(pattern) = parse_when_line(trimmed) {
            let start_line = i + 1; // 1-based
            let mut description = String::new();

            i += 1;
            while i < lines.len() {
                let next = lines[i];
                let next_trimmed = next.trim();

                // Stop at blank lines, comments, or new when rules
                if next_trimmed.is_empty()
                    || next_trimmed.starts_with('#')
                    || parse_when_line(next_trimmed).is_some()
                {
                    break;
                }

                if !description.is_empty() {
                    description.push('\n');
                }
                description.push_str(next_trimmed);
                i += 1;
            }

            annotations.push(Annotation {
                file_path: file_path.to_string(),
                line: start_line,
                kind: CheckKind::All { pattern },
                description,
            });
            continue;
        }

        eprintln!(
            "warning: unrecognized line in {}: {}",
            file_path, trimmed
        );
        i += 1;
    }

    annotations
}

static WHEN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^when\s+(.+?)\s+changes:$").unwrap());

fn parse_when_line(line: &str) -> Option<String> {
    WHEN_RE
        .captures(line)
        .map(|caps| caps[1].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_rule() {
        let content = "\
when src/**/*.rs changes:
  Make sure the README examples still compile.
";
        let anns = parse_watchfile(content, ".marginalia");
        assert_eq!(anns.len(), 1);
        assert_eq!(
            anns[0].kind,
            CheckKind::All {
                pattern: "src/**/*.rs".to_string()
            }
        );
        assert_eq!(
            anns[0].description,
            "Make sure the README examples still compile."
        );
        assert_eq!(anns[0].line, 1);
    }

    #[test]
    fn parse_multiple_rules() {
        let content = "\
# This is a comment

when src/**/*.rs changes:
  Update the README examples.

when *.proto changes:
  Regenerate protobuf bindings.
  Check that the migration guide is updated.
";
        let anns = parse_watchfile(content, ".marginalia");
        assert_eq!(anns.len(), 2);
        assert_eq!(
            anns[0].kind,
            CheckKind::All {
                pattern: "src/**/*.rs".to_string()
            }
        );
        assert_eq!(anns[0].description, "Update the README examples.");
        assert_eq!(
            anns[1].kind,
            CheckKind::All {
                pattern: "*.proto".to_string()
            }
        );
        assert_eq!(
            anns[1].description,
            "Regenerate protobuf bindings.\nCheck that the migration guide is updated."
        );
    }

    #[test]
    fn parse_empty_file() {
        let anns = parse_watchfile("", ".marginalia");
        assert_eq!(anns.len(), 0);
    }

    #[test]
    fn parse_comments_only() {
        let content = "\
# Just comments
# Nothing to see here
";
        let anns = parse_watchfile(content, ".marginalia");
        assert_eq!(anns.len(), 0);
    }

    #[test]
    fn rule_without_description() {
        let content = "when docs/** changes:\n";
        let anns = parse_watchfile(content, ".marginalia");
        assert_eq!(anns.len(), 1);
        assert_eq!(anns[0].description, "");
    }
}
