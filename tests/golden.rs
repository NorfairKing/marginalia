use marginalia::annotations::{Annotation, CheckKind};
use marginalia::output::{render_text, CheckItem};
use marginalia::scope::Scope;
use std::fs;
use std::path::Path;

fn golden_test(name: &str, checks: Vec<CheckItem>) {
    let golden_path = Path::new("tests/golden").join(format!("{}.txt", name));
    let actual = render_text(&checks, "main");

    if std::env::var("GOLDEN_RESET").is_ok() {
        fs::write(&golden_path, &actual).unwrap();
        return;
    }

    if !golden_path.exists() {
        panic!(
            "golden file {} does not exist. Run with GOLDEN_RESET=1 to create it.\n\nActual output:\n{}",
            golden_path.display(),
            actual
        );
    }

    let expected = fs::read_to_string(&golden_path).unwrap();
    if actual != expected {
        panic!(
            "golden test '{}' failed.\n\n--- expected ---\n{}\n--- actual ---\n{}\n\nRun with GOLDEN_RESET=1 to update.",
            name, expected, actual
        );
    }
}

#[test]
fn no_checks() {
    golden_test("no_checks", vec![]);
}

#[test]
fn scoped_check_with_tree_sitter() {
    golden_test(
        "scoped_check_with_tree_sitter",
        vec![CheckItem {
            annotation: Annotation {
                file_path: "src/auth.rs".to_string(),
                line: 41,
                kind: CheckKind::Check,
                description: "Verify this unwrap is safe — input is validated on line 12"
                    .to_string(),
            },
            scope: Some(Scope {
                start: 42,
                end: 48,
                label: Some("fn parse_token".to_string()),
            }),
            changed_ranges: vec![(45, 46)],
            matched_files: vec![],
            matched_file_ranges: vec![],
        }],
    );
}

#[test]
fn scoped_check_without_tree_sitter() {
    golden_test(
        "scoped_check_without_tree_sitter",
        vec![CheckItem {
            annotation: Annotation {
                file_path: "src/auth.rs".to_string(),
                line: 42,
                kind: CheckKind::Check,
                description: "Verify this unwrap is safe".to_string(),
            },
            scope: None,
            changed_ranges: vec![(45, 46)],
            matched_files: vec![],
            matched_file_ranges: vec![],
        }],
    );
}

#[test]
fn file_check() {
    golden_test(
        "file_check",
        vec![CheckItem {
            annotation: Annotation {
                file_path: "src/api.rs".to_string(),
                line: 1,
                kind: CheckKind::File,
                description: "Every handler must check permissions".to_string(),
            },
            scope: None,
            changed_ranges: vec![(15, 20), (38, 38)],
            matched_files: vec![],
            matched_file_ranges: vec![],
        }],
    );
}

#[test]
fn all_check() {
    golden_test(
        "all_check",
        vec![CheckItem {
            annotation: Annotation {
                file_path: "README.md".to_string(),
                line: 33,
                kind: CheckKind::All {
                    pattern: "src/**/*.rs".to_string(),
                },
                description: "Make sure the README examples still compile".to_string(),
            },
            scope: None,
            changed_ranges: vec![],
            matched_files: vec!["src/auth.rs".to_string(), "src/lib.rs".to_string()],
            matched_file_ranges: vec![
                ("src/auth.rs".to_string(), vec![(42, 48)]),
                ("src/lib.rs".to_string(), vec![(5, 10)]),
            ],
        }],
    );
}

#[test]
fn multiline_description() {
    golden_test(
        "multiline_description",
        vec![CheckItem {
            annotation: Annotation {
                file_path: "src/access.rs".to_string(),
                line: 9,
                kind: CheckKind::Check,
                description: "Make sure this function handles all three cases:\n\
                              1. User has no subscription\n\
                              2. User has an expired subscription\n\
                              3. User has a valid subscription but rate-limited"
                    .to_string(),
            },
            scope: Some(Scope {
                start: 10,
                end: 25,
                label: Some("fn check_access".to_string()),
            }),
            changed_ranges: vec![(12, 14)],
            matched_files: vec![],
            matched_file_ranges: vec![],
        }],
    );
}

#[test]
fn multiple_checks() {
    golden_test(
        "multiple_checks",
        vec![
            CheckItem {
                annotation: Annotation {
                    file_path: "src/auth.rs".to_string(),
                    line: 41,
                    kind: CheckKind::Check,
                    description: "Verify this unwrap is safe".to_string(),
                },
                scope: Some(Scope {
                    start: 42,
                    end: 48,
                    label: Some("fn parse_token".to_string()),
                }),
                changed_ranges: vec![(45, 46)],
                matched_files: vec![],
                matched_file_ranges: vec![],
            },
            CheckItem {
                annotation: Annotation {
                    file_path: "src/auth.rs".to_string(),
                    line: 1,
                    kind: CheckKind::File,
                    description: "Every handler must check permissions".to_string(),
                },
                scope: None,
                changed_ranges: vec![(45, 46)],
                matched_files: vec![],
                matched_file_ranges: vec![],
            },
            CheckItem {
                annotation: Annotation {
                    file_path: "README.md".to_string(),
                    line: 33,
                    kind: CheckKind::All {
                        pattern: "src/**/*.rs".to_string(),
                    },
                    description: "Make sure the README examples still compile".to_string(),
                },
                scope: None,
                changed_ranges: vec![],
                matched_files: vec!["src/auth.rs".to_string()],
                matched_file_ranges: vec![("src/auth.rs".to_string(), vec![(45, 46)])],
            },
        ],
    );
}

#[test]
fn file_and_all_mix() {
    golden_test(
        "file_and_all_mix",
        vec![
            CheckItem {
                annotation: Annotation {
                    file_path: "src/api.rs".to_string(),
                    line: 1,
                    kind: CheckKind::File,
                    description: "This module is used by the billing service.\n\
                                  Any signature change needs a corresponding change in\n\
                                  billing/client.py."
                        .to_string(),
                },
                scope: None,
                changed_ranges: vec![(15, 20), (38, 38)],
                matched_files: vec![],
                matched_file_ranges: vec![],
            },
            CheckItem {
                annotation: Annotation {
                    file_path: ".marginalia".to_string(),
                    line: 1,
                    kind: CheckKind::All {
                        pattern: "*.proto".to_string(),
                    },
                    description: "Regenerate protobuf bindings.\n\
                                  Check that the migration guide is updated."
                        .to_string(),
                },
                scope: None,
                changed_ranges: vec![],
                matched_files: vec!["api.proto".to_string()],
                matched_file_ranges: vec![("api.proto".to_string(), vec![(1, 10)])],
            },
        ],
    );
}

#[test]
fn single_line_scope() {
    golden_test(
        "single_line_scope",
        vec![CheckItem {
            annotation: Annotation {
                file_path: "src/config.rs".to_string(),
                line: 5,
                kind: CheckKind::Check,
                description: "This constant must match the server config".to_string(),
            },
            scope: Some(Scope {
                start: 6,
                end: 6,
                label: Some("const MAX_RETRIES".to_string()),
            }),
            changed_ranges: vec![(6, 6)],
            matched_files: vec![],
            matched_file_ranges: vec![],
        }],
    );
}
