use crate::annotations::{Annotation, CheckKind};
use crate::scope::Scope;

/// A check to be rendered.
#[derive(Debug, serde::Serialize)]
pub struct CheckItem {
    #[serde(flatten)]
    pub annotation: Annotation,
    /// The scope of the annotation (line range + label), if resolved via tree-sitter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    /// The changed line ranges that triggered this check (from the diff hunks).
    pub changed_ranges: Vec<(usize, usize)>,
    /// For [check:all]: the changed file paths that matched the pattern.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub matched_files: Vec<String>,
    /// For [check:all]: the changed line ranges in each matched file.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub matched_file_ranges: Vec<(String, Vec<(usize, usize)>)>,
    /// For [check:tag]: all locations sharing the tag (file_path, line).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tag_counterparts: Vec<(String, usize)>,
}

/// Render checks as plain text.
pub fn render_text(checks: &[CheckItem], base: &str) -> String {
    if checks.is_empty() {
        return "No checks found near changed code.\n".to_string();
    }

    let mut out = String::from("DO NOT IGNORE THIS MESSAGE\n\n");
    out.push_str("marginalia found the following checks near changed code.\n");
    out.push_str("Each check shows what changed, where to look, and what to check.\n");
    out.push_str(&format!(
        "Reproduce this message by running: marginalia --base {}\n",
        base
    ));

    for (i, item) in checks.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str("\n---\n\n");

        match &item.annotation.kind {
            CheckKind::Check => render_scoped_check(&mut out, item),
            CheckKind::File => render_file_check(&mut out, item),
            CheckKind::All { pattern } => render_all_check(&mut out, item, pattern),
            CheckKind::Tag { name } | CheckKind::Ref { name } => {
                render_tag_check(&mut out, item, name)
            }
        }
    }

    out
}

fn render_scoped_check(out: &mut String, item: &CheckItem) {
    // "src/auth.rs:45-46 changed (in fn parse_token)"
    let changed = format_ranges(&item.annotation.file_path, &item.changed_ranges);
    if let Some(scope) = &item.scope {
        let label = scope
            .label
            .as_deref()
            .map(|l| format!(" (in {})", l))
            .unwrap_or_default();
        out.push_str(&format!("{} changed{}\n", changed, label));

        // "check src/auth.rs:42-48"
        let scope_range = if scope.start == scope.end {
            format!(":{}", scope.start)
        } else {
            format!(":{}-{}", scope.start, scope.end)
        };
        out.push_str(&format!("check {}{}\n", item.annotation.file_path, scope_range));
    } else {
        out.push_str(&format!("{} changed\n", changed));
        let check_range = check_range_for(item.annotation.line, &item.changed_ranges);
        out.push_str(&format!("check {}{}\n", item.annotation.file_path, check_range));
    }

    out.push('\n');
    for line in item.annotation.description.lines() {
        out.push_str(line);
        out.push('\n');
    }
}

fn render_file_check(out: &mut String, item: &CheckItem) {
    // "src/api.rs:15-20,38 changed"
    let changed = format_ranges(&item.annotation.file_path, &item.changed_ranges);
    out.push_str(&format!("{} changed\n", changed));
    out.push_str(&format!("check {} entirely\n", item.annotation.file_path));

    out.push('\n');
    for line in item.annotation.description.lines() {
        out.push_str(line);
        out.push('\n');
    }
}

fn render_all_check(out: &mut String, item: &CheckItem, pattern: &str) {
    let file_parts: Vec<String> = item
        .matched_file_ranges
        .iter()
        .map(|(file, ranges)| format_ranges(file, ranges))
        .collect();
    let files: Vec<&str> = if file_parts.is_empty() {
        item.matched_files.iter().map(|s| s.as_str()).collect()
    } else {
        file_parts.iter().map(|s| s.as_str()).collect()
    };

    if files.len() == 1 {
        out.push_str(&format!("{} changed (matching {})\n", files[0], pattern));
    } else {
        out.push_str(&format!("changed (matching {}):\n", pattern));
        for file in &files {
            out.push_str(&format!("  {}\n", file));
        }
    }
    if item.annotation.file_path != ".marginalia" {
        out.push_str(&format!("check {}\n", item.annotation.file_path));
    }

    out.push('\n');
    for line in item.annotation.description.lines() {
        out.push_str(line);
        out.push('\n');
    }
}

fn render_tag_check(out: &mut String, item: &CheckItem, name: &str) {
    let file_parts: Vec<String> = item
        .matched_file_ranges
        .iter()
        .map(|(file, ranges)| format_ranges(file, ranges))
        .collect();
    let files: Vec<&str> = if file_parts.is_empty() {
        item.matched_files.iter().map(|s| s.as_str()).collect()
    } else {
        file_parts.iter().map(|s| s.as_str()).collect()
    };

    if files.len() == 1 {
        out.push_str(&format!("{} changed (tag {})\n", files[0], name));
    } else if files.len() > 1 {
        out.push_str(&format!("changed (tag {}):\n", name));
        for file in &files {
            out.push_str(&format!("  {}\n", file));
        }
    } else {
        out.push_str(&format!("tag {} activated\n", name));
    }
    if item.tag_counterparts.len() <= 1 {
        let ranges = ranges_for_file(&item.annotation.file_path, &item.matched_file_ranges);
        let check_range = check_range_for(item.annotation.line, &ranges);
        out.push_str(&format!(
            "check {}{}\n",
            item.annotation.file_path, check_range
        ));
    } else {
        out.push_str("check:\n");
        for (file, line) in &item.tag_counterparts {
            let ranges = ranges_for_file(file, &item.matched_file_ranges);
            let check_range = check_range_for(*line, &ranges);
            out.push_str(&format!("  {}{}\n", file, check_range));
        }
    }

    out.push('\n');
    for line in item.annotation.description.lines() {
        out.push_str(line);
        out.push('\n');
    }
}

/// Look up the changed ranges for a specific file from matched_file_ranges.
fn ranges_for_file(file: &str, matched_file_ranges: &[(String, Vec<(usize, usize)>)]) -> Vec<(usize, usize)> {
    matched_file_ranges
        .iter()
        .find(|(f, _)| f == file)
        .map(|(_, ranges)| ranges.clone())
        .unwrap_or_default()
}

/// Compute the range string to check: from the annotation line through the changed lines.
/// E.g. ":42-46" or ":42" if no changed ranges expand beyond the annotation line.
fn check_range_for(annotation_line: usize, changed_ranges: &[(usize, usize)]) -> String {
    let mut min_line = annotation_line;
    let mut max_line = annotation_line;
    for &(start, end) in changed_ranges {
        if start < min_line {
            min_line = start;
        }
        if end > max_line {
            max_line = end;
        }
    }
    if min_line == max_line {
        format!(":{}", min_line)
    } else {
        format!(":{}-{}", min_line, max_line)
    }
}

/// Format line ranges for a file, e.g. "src/auth.rs:15-20,38"
fn format_ranges(file: &str, ranges: &[(usize, usize)]) -> String {
    if ranges.is_empty() {
        return file.to_string();
    }
    let parts: Vec<String> = ranges
        .iter()
        .map(|(start, end)| {
            if start == end {
                format!("{}", start)
            } else {
                format!("{}-{}", start, end)
            }
        })
        .collect();
    format!("{}:{}", file, parts.join(","))
}

/// Render checks as JSON.
pub fn render_json(checks: &[CheckItem]) -> String {
    serde_json::to_string_pretty(checks).unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_checks() {
        let out = render_text(&[], "main");
        assert_eq!(out, "No checks found near changed code.\n");
    }

    #[test]
    fn json_output() {
        let checks = vec![CheckItem {
            annotation: Annotation {
                file_path: "foo.py".to_string(),
                line: 1,
                kind: CheckKind::File,
                description: "check imports".to_string(),
            },
            scope: None,
            changed_ranges: vec![(10, 15)],
            matched_files: vec![],
            matched_file_ranges: vec![],
            tag_counterparts: vec![],
        }];
        let out = render_json(&checks);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed[0]["description"], "check imports");
    }
}
