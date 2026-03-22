use clap::Parser;
use marginalia::annotations::extract_annotations;
use marginalia::comment_tokens;
use marginalia::comments::extract_comments;
use marginalia::diff;
use marginalia::matching::{active_all_annotations, active_annotations};
use marginalia::optparse::{Cli, OutputFormat};
use marginalia::output::{render_json, render_text, CheckItem};
use marginalia::scope;
use marginalia::watchfile;
use git2::Repository;
use std::fs;
use std::path::Path;

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
    for changed_file in &changed_files {
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
    }

    // Phase 2: find [check:all] annotations
    let all_annotations = collect_all_annotations(repo_path);
    let active = active_all_annotations(&all_annotations, &changed_files);
    all_checks.extend(active);

    let output = match cli.format {
        OutputFormat::Text => render_text(&all_checks),
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

/// Search tracked files for the string `[check:all` using libgit2's index.
/// Returns paths of files that contain the marker, excluding `.marginalia`.
fn find_files_with_check_all(repo_path: &Path) -> Vec<String> {
    let repo = match Repository::open(repo_path) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let index = match repo.index() {
        Ok(i) => i,
        Err(_) => return Vec::new(),
    };

    let needle = b"[check:all";
    let mut results = Vec::new();

    for entry in index.iter() {
        let path = match std::str::from_utf8(&entry.path) {
            Ok(p) => p.to_string(),
            Err(_) => continue,
        };

        if path == ".marginalia" {
            continue;
        }

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
