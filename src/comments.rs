use crate::CommentTokens;

/// A comment extracted from a source file.
#[derive(Debug, Clone)]
pub struct Comment {
    /// 1-based line number where the comment starts.
    pub line: usize,
    /// The comment text with delimiters stripped and trimmed.
    pub text: String,
}

/// Extract all comments from the given source text using the provided comment tokens.
pub fn extract_comments(source: &str, tokens: &CommentTokens) -> Vec<Comment> {
    let lines: Vec<&str> = source.lines().collect();
    let mut comments = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        // Try block comments first (they can span multiple lines)
        if let Some(comment) = try_block_comment(&lines, &mut i, tokens) {
            comments.push(comment);
            continue;
        }

        // Try line comments
        if let Some(comment) = try_line_comment(trimmed, i, tokens) {
            comments.push(comment);
        }

        i += 1;
    }

    comments
}

fn try_line_comment(trimmed: &str, line_idx: usize, tokens: &CommentTokens) -> Option<Comment> {
    for prefix in tokens.line {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return Some(Comment {
                line: line_idx + 1,
                text: rest.trim().to_string(),
            });
        }
        // Also handle inline comments: code // comment
        if let Some(pos) = trimmed.find(prefix) {
            if pos > 0 {
                let rest = &trimmed[pos + prefix.len()..];
                return Some(Comment {
                    line: line_idx + 1,
                    text: rest.trim().to_string(),
                });
            }
        }
    }
    None
}

fn try_block_comment(
    lines: &[&str],
    i: &mut usize,
    tokens: &CommentTokens,
) -> Option<Comment> {
    let line = lines[*i].trim();

    for (open, close) in tokens.block {
        if let Some(open_pos) = line.find(open) {
            let start_line = *i + 1;
            let after_open = &line[open_pos + open.len()..];

            // Single-line block comment
            if let Some(close_pos) = after_open.find(close) {
                let text = after_open[..close_pos].trim().to_string();
                *i += 1;
                return Some(Comment {
                    line: start_line,
                    text,
                });
            }

            // Multi-line block comment
            let mut text = after_open.trim().to_string();
            *i += 1;
            while *i < lines.len() {
                let current = lines[*i].trim();
                if let Some(close_pos) = current.find(close) {
                    let before_close = current[..close_pos].trim();
                    if !before_close.is_empty() {
                        if !text.is_empty() {
                            text.push(' ');
                        }
                        text.push_str(before_close);
                    }
                    *i += 1;
                    return Some(Comment {
                        line: start_line,
                        text,
                    });
                }
                let cleaned = current.strip_prefix('*').unwrap_or(current).trim();
                if !cleaned.is_empty() {
                    if !text.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(cleaned);
                }
                *i += 1;
            }

            // Unterminated block comment — return what we have
            *i += 1;
            return Some(Comment {
                line: start_line,
                text,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c_tokens() -> CommentTokens {
        CommentTokens {
            line: &["//"],
            block: &[("/*", "*/")],
        }
    }

    fn py_tokens() -> CommentTokens {
        CommentTokens {
            line: &["#"],
            block: &[],
        }
    }

    fn hs_tokens() -> CommentTokens {
        CommentTokens {
            line: &["--"],
            block: &[("{-", "-}")],
        }
    }

    #[test]
    fn line_comments_c_style() {
        let source = "int x = 1;\n// [check] verify bounds\nint y = 2;\n";
        let comments = extract_comments(source, &c_tokens());
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line, 2);
        assert_eq!(comments[0].text, "[check] verify bounds");
    }

    #[test]
    fn line_comments_python() {
        let source = "x = 1\n# [check] ensure rate limit\ny = 2\n";
        let comments = extract_comments(source, &py_tokens());
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].line, 2);
        assert_eq!(comments[0].text, "[check] ensure rate limit");
    }

    #[test]
    fn block_comment_single_line() {
        let source = "/* [check] verify this */ int x = 1;\n";
        let comments = extract_comments(source, &c_tokens());
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].text, "[check] verify this");
    }

    #[test]
    fn block_comment_multi_line() {
        let source = "/*\n * [check] verify\n * this thing\n */\nint x = 1;\n";
        let comments = extract_comments(source, &c_tokens());
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].text, "[check] verify this thing");
    }

    #[test]
    fn haskell_comments() {
        let source = "-- [check] verify purity\nfoo :: Int -> Int\n{- [check:file] check exports -}\n";
        let comments = extract_comments(source, &hs_tokens());
        assert_eq!(comments.len(), 2);
        assert_eq!(comments[0].text, "[check] verify purity");
        assert_eq!(comments[1].text, "[check:file] check exports");
    }

    #[test]
    fn inline_comment() {
        let source = "let x = dangerous_call(); // [check] ensure error handling\n";
        let comments = extract_comments(source, &c_tokens());
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].text, "[check] ensure error handling");
    }
}
