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
                tag_counterparts: vec![],
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
                        tag_counterparts: vec![],
                    })
                } else {
                    None
                }
            }
            CheckKind::All { .. } | CheckKind::Tag { .. } | CheckKind::Ref { .. } => None,
        })
        .collect()
}

/// Validate that every [check:all] pattern matches at least one tracked file.
///
/// Returns a list of (annotation, pattern) pairs for patterns that match nothing.
/// A pattern that matches no files in the entire repo is almost certainly stale or
/// contains a typo.
pub fn dead_patterns<'a>(
    annotations: &'a [Annotation],
    tracked_files: &[String],
) -> Vec<(&'a Annotation, &'a str)> {
    annotations
        .iter()
        .filter_map(|ann| {
            if let CheckKind::All { pattern } = &ann.kind {
                let pat = match glob::Pattern::new(pattern) {
                    Ok(p) => p,
                    Err(_) => return None, // invalid patterns are reported elsewhere
                };
                let has_match = tracked_files.iter().any(|f| pat.matches(f));
                if !has_match {
                    Some((ann, pattern.as_str()))
                } else {
                    None
                }
            } else {
                None
            }
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
                    tag_counterparts: vec![],
                })
            } else {
                None
            }
        })
        .collect()
}

/// Validate that every [check:ref] has a matching [check:tag].
///
/// A ref without a corresponding tag is an error. A tag without any ref is fine.
pub fn orphan_refs(annotations: &[Annotation]) -> Vec<(&Annotation, &str)> {
    use std::collections::HashSet;
    let tag_names: HashSet<&str> = annotations
        .iter()
        .filter_map(|ann| match &ann.kind {
            CheckKind::Tag { name } => Some(name.as_str()),
            _ => None,
        })
        .collect();

    let mut result: Vec<(&Annotation, &str)> = annotations
        .iter()
        .filter_map(|ann| match &ann.kind {
            CheckKind::Ref { name } if !tag_names.contains(name.as_str()) => {
                Some((ann, name.as_str()))
            }
            _ => None,
        })
        .collect();
    result.sort_by_key(|(ann, _)| (&ann.file_path, ann.line));
    result
}

/// Activate [check:tag] and [check:ref] annotations.
///
/// A tag/ref annotation is active if any annotation with the same name
/// is near changed code. When one activates, they all activate.
/// The emitted CheckItem uses the first [check:tag] as the representative
/// (for its description). All tag and ref locations are listed as counterparts.
///
/// `per_file_annotations` contains (annotations, changed_file, source, tree)
/// tuples for each changed file that was already parsed.
pub fn active_tag_annotations(
    all_tag_annotations: &[Annotation],
    per_file_annotations: &[(
        &[Annotation],
        &ChangedFile,
        &str,
        Option<&Tree>,
    )],
) -> Vec<CheckItem> {
    use std::collections::HashMap;

    fn tag_name(ann: &Annotation) -> Option<&str> {
        match &ann.kind {
            CheckKind::Tag { name } | CheckKind::Ref { name } => Some(name.as_str()),
            _ => None,
        }
    }

    // Group all tag/ref annotations by name
    let mut by_name: HashMap<&str, Vec<&Annotation>> = HashMap::new();
    for ann in all_tag_annotations {
        if let Some(name) = tag_name(ann) {
            by_name.entry(name).or_default().push(ann);
        }
    }

    // For each changed file, find which tag/ref annotations are near changed code.
    // Track the triggering file and overlapping ranges per tag name.
    type TriggerRanges = Vec<(String, Vec<(usize, usize)>)>;
    let mut triggered: HashMap<&str, TriggerRanges> = HashMap::new();
    for (annotations, changed_file, source, tree) in per_file_annotations {
        let all_ranges: Vec<(usize, usize)> = changed_file
            .hunks
            .iter()
            .map(|h| (h.new_start, h.new_end))
            .collect();

        for ann in *annotations {
            if let Some(name) = tag_name(ann) {
                let scope = tree.and_then(|t| scope::find_scope(t, ann.line, source));
                let overlapping = overlapping_ranges(&all_ranges, ann, scope.as_ref());
                if !overlapping.is_empty() {
                    triggered
                        .entry(name)
                        .or_default()
                        .push((changed_file.path.clone(), overlapping));
                }
            }
        }
    }

    if triggered.is_empty() {
        return Vec::new();
    }

    // Emit one CheckItem per activated tag name, listing all counterpart locations.
    let mut result = Vec::new();
    for (name, trigger_ranges) in &triggered {
        if let Some(anns) = by_name.get(name) {
            let matched_files: Vec<String> =
                trigger_ranges.iter().map(|(f, _)| f.clone()).collect();
            let counterparts: Vec<(String, usize)> = anns
                .iter()
                .map(|a| (a.file_path.clone(), a.line))
                .collect();
            // Use the first [check:tag] as the representative (for its description).
            let representative = anns
                .iter()
                .find(|a| matches!(a.kind, CheckKind::Tag { .. }))
                .unwrap_or(&anns[0]);
            result.push(CheckItem {
                annotation: (*representative).clone(),
                scope: None,
                changed_ranges: vec![],
                matched_files,
                matched_file_ranges: trigger_ranges.clone(),
                tag_counterparts: counterparts,
            });
        }
    }

    result
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
    fn dead_pattern_matches_nothing() {
        let anns = vec![Annotation {
            file_path: "README.md".to_string(),
            line: 1,
            kind: CheckKind::All {
                pattern: "src/**/*.go".to_string(),
            },
            description: "update examples".to_string(),
        }];
        let tracked = vec![
            "src/lib.rs".to_string(),
            "src/main.rs".to_string(),
            "README.md".to_string(),
        ];
        let dead = dead_patterns(&anns, &tracked);
        assert_eq!(dead.len(), 1);
        assert_eq!(dead[0].1, "src/**/*.go");
    }

    #[test]
    fn live_pattern_matches_something() {
        let anns = vec![Annotation {
            file_path: "README.md".to_string(),
            line: 1,
            kind: CheckKind::All {
                pattern: "src/**/*.rs".to_string(),
            },
            description: "update examples".to_string(),
        }];
        let tracked = vec![
            "src/lib.rs".to_string(),
            "src/main.rs".to_string(),
        ];
        let dead = dead_patterns(&anns, &tracked);
        assert_eq!(dead.len(), 0);
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

    fn tag_ann(file: &str, line: usize, name: &str, desc: &str) -> Annotation {
        Annotation {
            file_path: file.to_string(),
            line,
            kind: CheckKind::Tag {
                name: name.to_string(),
            },
            description: desc.to_string(),
        }
    }

    fn ref_ann(file: &str, line: usize, name: &str) -> Annotation {
        Annotation {
            file_path: file.to_string(),
            line,
            kind: CheckKind::Ref {
                name: name.to_string(),
            },
            description: String::new(),
        }
    }

    #[test]
    fn orphan_ref_detected() {
        let anns = vec![ref_ann("a.rs", 1, "Foo")];
        let orphans = orphan_refs(&anns);
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].1, "Foo");
    }

    #[test]
    fn ref_with_matching_tag_not_orphan() {
        let anns = vec![
            tag_ann("a.rs", 1, "Foo", "keep in sync"),
            ref_ann("b.rs", 5, "Foo"),
        ];
        let orphans = orphan_refs(&anns);
        assert_eq!(orphans.len(), 0);
    }

    #[test]
    fn tag_alone_is_fine() {
        let anns = vec![tag_ann("a.rs", 1, "Foo", "keep in sync")];
        let orphans = orphan_refs(&anns);
        assert_eq!(orphans.len(), 0);
    }

    #[test]
    fn tag_activates_all_counterparts() {
        // Two tag annotations with name "Sync", one in a changed file near a change
        let tag_anns = vec![
            tag_ann("a.rs", 5, "Sync", "keep in sync"),
            tag_ann("b.rs", 10, "Sync", "keep in sync"),
        ];

        let cf = ChangedFile {
            path: "a.rs".to_string(),
            hunks: vec![Hunk {
                new_start: 6,
                new_end: 8,
            }],
        };

        // The annotations from the changed file (a.rs) include the tag
        let file_anns = vec![tag_ann("a.rs", 5, "Sync", "keep in sync")];

        let per_file = vec![(
            file_anns.as_slice(),
            &cf,
            "" as &str,
            None::<&tree_sitter::Tree>,
        )];

        let active = active_tag_annotations(&tag_anns, &per_file);
        assert_eq!(active.len(), 1);
        assert_eq!(
            active[0].tag_counterparts,
            vec![
                ("a.rs".to_string(), 5),
                ("b.rs".to_string(), 10),
            ]
        );
    }

    #[test]
    fn tag_does_not_activate_when_no_change_nearby() {
        let tag_anns = vec![
            tag_ann("a.rs", 5, "Sync", "keep in sync"),
            tag_ann("b.rs", 10, "Sync", "keep in sync"),
        ];

        let cf = ChangedFile {
            path: "a.rs".to_string(),
            hunks: vec![Hunk {
                new_start: 50,
                new_end: 55,
            }],
        };

        let file_anns = vec![tag_ann("a.rs", 5, "Sync", "keep in sync")];

        let per_file = vec![(
            file_anns.as_slice(),
            &cf,
            "" as &str,
            None::<&tree_sitter::Tree>,
        )];

        let active = active_tag_annotations(&tag_anns, &per_file);
        assert_eq!(active.len(), 0);
    }

    #[test]
    fn ref_triggers_tag_activation() {
        // A tag in a.rs and a ref in b.rs — change near the ref should activate
        let all_anns = vec![
            tag_ann("a.rs", 5, "Sync", "keep in sync"),
            ref_ann("b.rs", 10, "Sync"),
        ];

        let cf = ChangedFile {
            path: "b.rs".to_string(),
            hunks: vec![Hunk {
                new_start: 11,
                new_end: 13,
            }],
        };

        let file_anns = vec![ref_ann("b.rs", 10, "Sync")];

        let per_file = vec![(
            file_anns.as_slice(),
            &cf,
            "" as &str,
            None::<&tree_sitter::Tree>,
        )];

        let active = active_tag_annotations(&all_anns, &per_file);
        assert_eq!(active.len(), 1);
        // Representative should be the tag, not the ref
        assert_eq!(active[0].annotation.file_path, "a.rs");
        assert!(matches!(active[0].annotation.kind, CheckKind::Tag { .. }));
        assert_eq!(
            active[0].tag_counterparts,
            vec![
                ("a.rs".to_string(), 5),
                ("b.rs".to_string(), 10),
            ]
        );
    }
}
