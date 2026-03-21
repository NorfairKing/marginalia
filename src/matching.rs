use crate::annotations::{Annotation, CheckKind};
use crate::diff::ChangedFile;
use crate::output::CheckItem;
use crate::scope::{self, Scope};
use tree_sitter::Tree;

/// Default fallback proximity in lines, used when tree-sitter has no grammar
/// for the language.
const FALLBACK_PROXIMITY: usize = 10;

/// Filter annotations to only those that are "active" given the changed hunks.
///
/// When a parsed tree is available (tree-sitter supports the language), uses
/// semantic scoping: the annotation covers the AST node it's attached to.
/// Otherwise, falls back to line proximity.
///
/// [check:all] annotations are not handled here — use `active_all_annotations` instead.
pub fn active_annotations(
    annotations: &[Annotation],
    changed_file: &ChangedFile,
    source: &str,
    tree: Option<&Tree>,
) -> Vec<CheckItem> {
    let all_ranges: Vec<(usize, usize)> = changed_file
        .hunks
        .iter()
        .map(|h| (h.new_start, h.new_end))
        .collect();

    annotations
        .iter()
        .filter_map(|ann| match &ann.kind {
            CheckKind::File => Some(CheckItem {
                annotation: ann.clone(),
                scope: None,
                changed_ranges: all_ranges.clone(),
                matched_files: vec![],
                matched_file_ranges: vec![],
            }),
            CheckKind::Check => {
                let scope = tree.and_then(|t| scope::find_scope(t, ann.line, source));
                let overlapping = overlapping_ranges(&all_ranges, ann, scope.as_ref());
                if !overlapping.is_empty() {
                    Some(CheckItem {
                        annotation: ann.clone(),
                        scope,
                        changed_ranges: overlapping,
                        matched_files: vec![],
                        matched_file_ranges: vec![],
                    })
                } else {
                    None
                }
            }
            CheckKind::All { .. } => None,
        })
        .collect()
}

/// Filter [check:all] annotations against the full list of changed files.
pub fn active_all_annotations(
    annotations: &[Annotation],
    changed_files: &[ChangedFile],
) -> Vec<CheckItem> {
    annotations
        .iter()
        .filter_map(|ann| {
            if let CheckKind::All { pattern } = &ann.kind {
                let pat = match glob::Pattern::new(pattern) {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!(
                            "warning: invalid glob pattern '{}' in {}:{}: {}",
                            pattern, ann.file_path, ann.line, e
                        );
                        return None;
                    }
                };

                let matched: Vec<&ChangedFile> = changed_files
                    .iter()
                    .filter(|f| pat.matches(&f.path))
                    .collect();

                if matched.is_empty() {
                    return None;
                }

                let matched_files: Vec<String> =
                    matched.iter().map(|f| f.path.clone()).collect();
                let matched_file_ranges: Vec<(String, Vec<(usize, usize)>)> = matched
                    .iter()
                    .map(|f| {
                        let ranges: Vec<(usize, usize)> =
                            f.hunks.iter().map(|h| (h.new_start, h.new_end)).collect();
                        (f.path.clone(), ranges)
                    })
                    .collect();

                Some(CheckItem {
                    annotation: ann.clone(),
                    scope: None,
                    changed_ranges: vec![],
                    matched_files,
                    matched_file_ranges,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Return the changed ranges that overlap with the annotation's scope (or proximity).
fn overlapping_ranges(
    all_ranges: &[(usize, usize)],
    ann: &Annotation,
    scope: Option<&Scope>,
) -> Vec<(usize, usize)> {
    match scope {
        Some(scope) => all_ranges
            .iter()
            .filter(|(start, end)| *start <= scope.end && *end >= scope.start)
            .copied()
            .collect(),
        None => {
            let ann_start = ann.line.saturating_sub(FALLBACK_PROXIMITY);
            let ann_end = ann.line + FALLBACK_PROXIMITY;
            all_ranges
                .iter()
                .filter(|(start, end)| *start <= ann_end && *end >= ann_start)
                .copied()
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::Hunk;

    fn check_ann(line: usize, desc: &str) -> Annotation {
        Annotation {
            file_path: "test.rs".to_string(),
            line,
            kind: CheckKind::Check,
            description: desc.to_string(),
        }
    }

    fn file_ann(desc: &str) -> Annotation {
        Annotation {
            file_path: "test.rs".to_string(),
            line: 1,
            kind: CheckKind::File,
            description: desc.to_string(),
        }
    }

    fn changed(hunks: Vec<Hunk>) -> ChangedFile {
        ChangedFile {
            path: "test.rs".to_string(),
            hunks,
        }
    }

    #[test]
    fn file_annotation_always_active() {
        let anns = vec![file_ann("check exports")];
        let cf = changed(vec![Hunk {
            new_start: 100,
            new_end: 105,
        }]);
        let active = active_annotations(&anns, &cf, "", None);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].changed_ranges, vec![(100, 105)]);
    }

    #[test]
    fn check_within_proximity() {
        let anns = vec![check_ann(15, "check bounds")];
        let cf = changed(vec![Hunk {
            new_start: 20,
            new_end: 25,
        }]);
        let active = active_annotations(&anns, &cf, "", None);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].changed_ranges, vec![(20, 25)]);
    }

    #[test]
    fn check_outside_proximity() {
        let anns = vec![check_ann(5, "check bounds")];
        let cf = changed(vec![Hunk {
            new_start: 50,
            new_end: 55,
        }]);
        let active = active_annotations(&anns, &cf, "", None);
        assert_eq!(active.len(), 0);
    }

    #[test]
    fn semantic_scope_activates() {
        let ann = check_ann(1, "check this function");
        let scope = Some(Scope {
            start: 2,
            end: 10,
            label: None,
        });
        let overlapping = overlapping_ranges(&[(8, 9)], &ann, scope.as_ref());
        assert_eq!(overlapping, vec![(8, 9)]);
    }

    #[test]
    fn semantic_scope_does_not_activate() {
        let ann = check_ann(1, "check this function");
        let scope = Some(Scope {
            start: 2,
            end: 10,
            label: None,
        });
        let overlapping = overlapping_ranges(&[(20, 25)], &ann, scope.as_ref());
        assert!(overlapping.is_empty());
    }

    #[test]
    fn semantic_with_real_tree() {
        let source = "\
// [check] Verify bounds
fn process(x: usize) -> usize {
    if x > 0 {
        x - 1
    } else {
        0
    }
}

fn other() {
    let y = 42;
}
";
        let lang = scope::language_for_extension("rs").unwrap();
        let tree = scope::parse(source, lang).unwrap();

        let anns = vec![check_ann(1, "Verify bounds")];

        // Change inside process() — should activate
        let cf = changed(vec![Hunk {
            new_start: 4,
            new_end: 4,
        }]);
        let active = active_annotations(&anns, &cf, source, Some(&tree));
        assert_eq!(active.len(), 1);

        // Change inside other() — should NOT activate
        let cf2 = changed(vec![Hunk {
            new_start: 11,
            new_end: 11,
        }]);
        let active2 = active_annotations(&anns, &cf2, source, Some(&tree));
        assert_eq!(active2.len(), 0);
    }

    #[test]
    fn all_matches_changed_files() {
        let anns = vec![Annotation {
            file_path: "README.md".to_string(),
            line: 1,
            kind: CheckKind::All {
                pattern: "src/**/*.rs".to_string(),
            },
            description: "update examples".to_string(),
        }];
        let files = vec![
            ChangedFile {
                path: "src/lib.rs".to_string(),
                hunks: vec![Hunk {
                    new_start: 5,
                    new_end: 10,
                }],
            },
            ChangedFile {
                path: "src/main.rs".to_string(),
                hunks: vec![Hunk {
                    new_start: 1,
                    new_end: 3,
                }],
            },
        ];
        let active = active_all_annotations(&anns, &files);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].matched_files, vec!["src/lib.rs", "src/main.rs"]);
        assert_eq!(active[0].matched_file_ranges.len(), 2);
    }

    #[test]
    fn all_does_not_match() {
        let anns = vec![Annotation {
            file_path: "README.md".to_string(),
            line: 1,
            kind: CheckKind::All {
                pattern: "src/**/*.rs".to_string(),
            },
            description: "update examples".to_string(),
        }];
        let files = vec![ChangedFile {
            path: "docs/guide.md".to_string(),
            hunks: vec![Hunk {
                new_start: 1,
                new_end: 5,
            }],
        }];
        let active = active_all_annotations(&anns, &files);
        assert_eq!(active.len(), 0);
    }

    #[test]
    fn all_star_pattern() {
        let anns = vec![Annotation {
            file_path: ".marginalia".to_string(),
            line: 1,
            kind: CheckKind::All {
                pattern: "*.proto".to_string(),
            },
            description: "regenerate bindings".to_string(),
        }];
        let files = vec![ChangedFile {
            path: "api.proto".to_string(),
            hunks: vec![Hunk {
                new_start: 1,
                new_end: 10,
            }],
        }];
        let active = active_all_annotations(&anns, &files);
        assert_eq!(active.len(), 1);
    }
}
