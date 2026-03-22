use clap::Parser;
use marginalia::annotations::extract_annotations;
use marginalia::comment_tokens;
use marginalia::comments::extract_comments;
use marginalia::diff;
use marginalia::matching::{active_all_annotations, active_annotations, active_tag_annotations, dead_patterns, orphan_refs};
use marginalia::optparse::{Cli, OutputFormat};
use marginalia::output::{render_json, render_text, CheckItem};
use marginalia::scope;
use marginalia::watchfile;
use git2::Repository;
use std::fs;
use std::path::Path;
use std::process;

fn main() {
    let cli = Cli::parse();
    let repo_path = Path::new(".");

    let watchfile_path = repo_path.join(".marginalia");
    let watchfile_content = fs::read_to_string(&watchfile_path).unwrap_or_default();
    let config = watchfile::parse_config(&watchfile_content);

    let base = match &cli.base {
        Some(b) => b.clone(),
        None => match &config.base {
            Some(b) => b.clone(),
            None => match diff::detect_base_branch(repo_path) {
                Ok(b) => b,
                Err(_) => "HEAD".to_string(),
            },
        },
    };
    let changed_files = match diff::changed_files(repo_path, &base) {
        Ok(files) => files,
        Err(e) => {
            eprintln!("Could not diff against '{}': {}", base, e);
            return;
        }
    };

    if changed_files.is_empty() {
        println!("No changed files found.");
        return;
    }

    let mut all_checks: Vec<CheckItem> = Vec::new();

    // Phase 1: scan changed files for [check] and [check:file] annotations
    // We also store parsed data per file so tag annotations can be activated in phase 3.
    struct ParsedFile {
        annotations: Vec<marginalia::annotations::Annotation>,
        source: String,
        tree: Option<tree_sitter::Tree>,
    }
    let mut parsed_files: Vec<(usize, ParsedFile)> = Vec::new();

    for (idx, changed_file) in changed_files.iter().enumerate() {
        let path = repo_path.join(&changed_file.path);
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        let tokens = match comment_tokens(extension) {
            Some(t) => t,
            None => continue,
        };

        let source = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("warning: could not read {}: {}", path.display(), e);
                continue;
            }
        };

        let comments = extract_comments(&source, &tokens);
        let annotations = extract_annotations(&comments, &changed_file.path);

        let tree = scope::language_for_extension(extension)
            .and_then(|lang| scope::parse(&source, lang));
        let active = active_annotations(
            &annotations,
            changed_file,
            &source,
            tree.as_ref(),
        );

        all_checks.extend(active);
        parsed_files.push((idx, ParsedFile { annotations, source, tree }));
    }

    // Phase 2: find [check:all] annotations
    let all_annotations = collect_all_annotations(repo_path);

    // Validate that every pattern matches at least one tracked file.
    let tracked = tracked_file_paths(repo_path);
    let dead = dead_patterns(&all_annotations, &tracked);
    if !dead.is_empty() {
        for (ann, pattern) in &dead {
            eprintln!(
                "error: pattern '{}' in {}:{} matches no files in the repository",
                pattern, ann.file_path, ann.line,
            );
        }
        process::exit(1);
    }

    let active = active_all_annotations(&all_annotations, &changed_files);
    all_checks.extend(active);

    // Phase 3: find [check:tag] and [check:ref] annotations
    let tag_annotations = collect_tag_annotations(repo_path);

    // Validate that every [check:ref] has a matching [check:tag].
    let orphans = orphan_refs(&tag_annotations);
    if !orphans.is_empty() {
        for (ann, name) in &orphans {
            eprintln!(
                "error: [check:ref {}] in {}:{} has no matching [check:tag {}]",
                name, ann.file_path, ann.line, name,
            );
        }
        process::exit(1);
    }

    let per_file: Vec<_> = parsed_files
        .iter()
        .map(|(idx, pf)| {
            (
                pf.annotations.as_slice(),
                &changed_files[*idx],
                pf.source.as_str(),
                pf.tree.as_ref(),
            )
        })
        .collect();
    let active_tags = active_tag_annotations(&tag_annotations, &per_file);
    all_checks.extend(active_tags);

    let output = match cli.format {
        OutputFormat::Text => render_text(&all_checks, &base),
        OutputFormat::Json => render_json(&all_checks),
    };

    print!("{}", output);
}

/// Collect all [check:all] annotations from:
/// 1. The `.marginalia` file at the repo root
/// 2. Any tracked file containing `[check:all` (found via git2 index)
fn collect_all_annotations(repo_path: &Path) -> Vec<marginalia::annotations::Annotation> {
    let mut annotations = Vec::new();

    // Parse .marginalia file
    let watchfile_path = repo_path.join(".marginalia");
    if let Ok(content) = fs::read_to_string(&watchfile_path) {
        annotations.extend(watchfile::parse_watchfile(&content, ".marginalia"));
    }

    // Find tracked files containing [check:all
    let files = find_files_with_check_all(repo_path);
    for file_path in files {
        let full_path = repo_path.join(&file_path);
        let extension = full_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        let tokens = match comment_tokens(extension) {
            Some(t) => t,
            None => continue,
        };

        let source = match fs::read_to_string(&full_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let comments = extract_comments(&source, &tokens);
        let file_annotations = extract_annotations(&comments, &file_path);
        annotations.extend(
            file_annotations
                .into_iter()
                .filter(|a| matches!(a.kind, marginalia::annotations::CheckKind::All { .. })),
        );
    }

    annotations
}

/// Collect all [check:tag] and [check:ref] annotations from tracked files.
fn collect_tag_annotations(repo_path: &Path) -> Vec<marginalia::annotations::Annotation> {
    use marginalia::annotations::CheckKind;

    // Search for files containing either needle.
    let mut file_set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    file_set.extend(find_files_with_needle(repo_path, b"[check:tag"));
    file_set.extend(find_files_with_needle(repo_path, b"[check:ref"));

    let mut annotations = Vec::new();

    for file_path in file_set {
        let full_path = repo_path.join(&file_path);
        let extension = full_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        let tokens = match comment_tokens(extension) {
            Some(t) => t,
            None => continue,
        };

        let source = match fs::read_to_string(&full_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let comments = extract_comments(&source, &tokens);
        let file_annotations = extract_annotations(&comments, &file_path);
        annotations.extend(
            file_annotations
                .into_iter()
                .filter(|a| matches!(a.kind, CheckKind::Tag { .. } | CheckKind::Ref { .. })),
        );
    }

    annotations
}

/// Return all tracked file paths from the git index.
fn tracked_file_paths(repo_path: &Path) -> Vec<String> {
    let repo = match Repository::open(repo_path) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let index = match repo.index() {
        Ok(i) => i,
        Err(_) => return Vec::new(),
    };

    index
        .iter()
        .filter_map(|entry| std::str::from_utf8(&entry.path).ok().map(|p| p.to_string()))
        .collect()
}

/// Search tracked files for the string `[check:all` using libgit2's index.
/// Returns paths of files that contain the marker, excluding `.marginalia`.
fn find_files_with_check_all(repo_path: &Path) -> Vec<String> {
    find_files_with_needle(repo_path, b"[check:all")
        .into_iter()
        .filter(|p| p != ".marginalia")
        .collect()
}

/// Search tracked files for a byte needle using libgit2's index.
/// Returns paths of files that contain the needle.
fn find_files_with_needle(repo_path: &Path, needle: &[u8]) -> Vec<String> {
    let repo = match Repository::open(repo_path) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let index = match repo.index() {
        Ok(i) => i,
        Err(_) => return Vec::new(),
    };

    let mut results = Vec::new();

    for entry in index.iter() {
        let path = match std::str::from_utf8(&entry.path) {
            Ok(p) => p.to_string(),
            Err(_) => continue,
        };

        let full_path = repo_path.join(&path);
        let content = match fs::read(&full_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if content.windows(needle.len()).any(|w| w == needle) {
            results.push(path);
        }
    }

    results
}
