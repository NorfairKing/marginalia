use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("failed to run git");
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn setup_repo(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    git(dir, &["init"]);
    git(dir, &["config", "user.email", "test@test.com"]);
    git(dir, &["config", "user.name", "Test"]);
    git(dir, &["checkout", "-b", "main"]);

    // Initial commit on main with an annotated file
    fs::write(
        dir.join("lib.rs"),
        "// [check] Ensure bounds are checked\nfn process(x: usize) {\n    println!(\"{}\", x);\n}\n",
    )
    .unwrap();
    git(dir, &["add", "lib.rs"]);
    git(dir, &["commit", "-m", "initial"]);

    // Create a feature branch
    git(dir, &["checkout", "-b", "feature"]);
}

#[test]
fn detects_committed_changes() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_repo(repo);

    // Make a committed change near the annotation
    fs::write(
        repo.join("lib.rs"),
        "// [check] Ensure bounds are checked\nfn process(x: usize) {\n    let y = x + 1;\n    println!(\"{}\", y);\n}\n",
    )
    .unwrap();
    git(repo, &["add", "lib.rs"]);
    git(repo, &["commit", "-m", "modify process"]);

    let files = marginalia::diff::changed_files(repo, "main").unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "lib.rs");
    assert!(!files[0].hunks.is_empty());
}

#[test]
fn detects_uncommitted_changes() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_repo(repo);

    // Make an uncommitted (unstaged) change near the annotation
    fs::write(
        repo.join("lib.rs"),
        "// [check] Ensure bounds are checked\nfn process(x: usize) {\n    let y = x + 1;\n    println!(\"{}\", y);\n}\n",
    )
    .unwrap();

    // Do NOT commit — this is the bug scenario
    let files = marginalia::diff::changed_files(repo, "main").unwrap();
    assert_eq!(files.len(), 1, "should detect uncommitted changes");
    assert_eq!(files[0].path, "lib.rs");
    assert!(!files[0].hunks.is_empty());
}

#[test]
fn detects_staged_changes() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_repo(repo);

    // Make a staged but uncommitted change
    fs::write(
        repo.join("lib.rs"),
        "// [check] Ensure bounds are checked\nfn process(x: usize) {\n    let y = x + 1;\n    println!(\"{}\", y);\n}\n",
    )
    .unwrap();
    git(repo, &["add", "lib.rs"]);

    // Staged but not committed
    let files = marginalia::diff::changed_files(repo, "main").unwrap();
    assert_eq!(files.len(), 1, "should detect staged changes");
    assert_eq!(files[0].path, "lib.rs");
    assert!(!files[0].hunks.is_empty());
}

fn marginalia_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_marginalia"))
}

fn run_marginalia(repo: &Path) -> std::process::Output {
    Command::new(marginalia_bin())
        .arg("--base")
        .arg("main")
        .current_dir(repo)
        .output()
        .expect("failed to run marginalia")
}

#[test]
fn dead_pattern_in_marginalia_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_repo(repo);

    // Add a .marginalia file with a pattern that matches no tracked files
    fs::write(
        repo.join(".marginalia"),
        "when src/**/*.go changes:\n  Check Go bindings.\n",
    )
    .unwrap();
    git(repo, &["add", ".marginalia"]);
    git(repo, &["commit", "-m", "add marginalia config"]);

    // Make a change so there are changed files
    fs::write(
        repo.join("lib.rs"),
        "// [check] Ensure bounds are checked\nfn process(x: usize) {\n    let y = x + 1;\n}\n",
    )
    .unwrap();

    let output = run_marginalia(repo);
    assert!(
        !output.status.success(),
        "marginalia should exit with error for dead pattern"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("matches no files"),
        "stderr should mention the dead pattern: {}",
        stderr
    );
    assert!(
        stderr.contains("src/**/*.go"),
        "stderr should include the pattern: {}",
        stderr
    );
}

#[test]
fn dead_pattern_in_source_file() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_repo(repo);

    // Add a file with a check-all that has a pattern matching no tracked files.
    let check_all = format!("// [check:{} nonexistent/**/*.xyz] Update the docs\nfn foo() {{}}\n", "all");
    fs::write(repo.join("notes.rs"), &check_all).unwrap();
    git(repo, &["add", "notes.rs"]);
    git(repo, &["commit", "-m", "add notes"]);

    // Make a change so there are changed files
    fs::write(
        repo.join("lib.rs"),
        "// [check] Ensure bounds are checked\nfn process(x: usize) {\n    let y = x + 1;\n}\n",
    )
    .unwrap();

    let output = run_marginalia(repo);
    assert!(
        !output.status.success(),
        "marginalia should exit with error for dead pattern in source file"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("nonexistent/**/*.xyz"),
        "stderr should include the pattern: {}",
        stderr
    );
}

#[test]
fn orphan_ref_errors() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_repo(repo);

    // Add a file with [check:ref] but no matching [check:tag]
    fs::write(
        repo.join("decoder.rs"),
        "// [check:ref WireFormat]\nfn decode() {}\n",
    )
    .unwrap();
    git(repo, &["add", "decoder.rs"]);
    git(repo, &["commit", "-m", "add decoder"]);

    // Make a change so there are changed files
    fs::write(
        repo.join("lib.rs"),
        "// [check] Ensure bounds are checked\nfn process(x: usize) {\n    let y = x + 1;\n}\n",
    )
    .unwrap();

    let output = run_marginalia(repo);
    assert!(
        !output.status.success(),
        "marginalia should exit with error for orphan ref"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no matching [check:tag WireFormat]"),
        "stderr should mention the orphan ref: {}",
        stderr
    );
}

#[test]
fn lone_tag_is_fine() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_repo(repo);

    // A [check:tag] without any ref or other tag is fine
    fs::write(
        repo.join("encoder.rs"),
        "// [check:tag WireFormat] Keep in sync\nfn encode() {}\n",
    )
    .unwrap();
    git(repo, &["add", "encoder.rs"]);
    git(repo, &["commit", "-m", "add encoder"]);

    // Make a change so there are changed files
    fs::write(
        repo.join("lib.rs"),
        "// [check] Ensure bounds are checked\nfn process(x: usize) {\n    let y = x + 1;\n}\n",
    )
    .unwrap();

    let output = run_marginalia(repo);
    assert!(
        output.status.success(),
        "marginalia should succeed with a lone tag: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn ref_activates_tag() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_repo(repo);

    // A tag in encoder.rs and a ref in decoder.rs
    fs::write(
        repo.join("encoder.rs"),
        "// [check:tag WireFormat] Keep in sync\nfn encode() {}\n",
    )
    .unwrap();
    fs::write(
        repo.join("decoder.rs"),
        "// [check:ref WireFormat]\nfn decode() {}\n",
    )
    .unwrap();
    git(repo, &["add", "encoder.rs", "decoder.rs"]);
    git(repo, &["commit", "-m", "add encoder and decoder"]);

    // Change near the ref — should activate the tag
    fs::write(
        repo.join("decoder.rs"),
        "// [check:ref WireFormat]\nfn decode() {\n    todo!()\n}\n",
    )
    .unwrap();

    let output = run_marginalia(repo);
    assert!(
        output.status.success(),
        "marginalia should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("tag WireFormat"),
        "stdout should show activated tag: {}",
        stdout
    );
    assert!(
        stdout.contains("encoder.rs"),
        "stdout should mention encoder.rs: {}",
        stdout
    );
    assert!(
        stdout.contains("decoder.rs"),
        "stdout should mention decoder.rs: {}",
        stdout
    );
}

#[test]
fn tag_activates_counterpart() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_repo(repo);

    // Add two files with matching [check:tag]
    fs::write(
        repo.join("encoder.rs"),
        "// [check:tag WireFormat] Keep in sync\nfn encode() {}\n",
    )
    .unwrap();
    fs::write(
        repo.join("decoder.rs"),
        "// [check:tag WireFormat] Keep in sync\nfn decode() {}\n",
    )
    .unwrap();
    git(repo, &["add", "encoder.rs", "decoder.rs"]);
    git(repo, &["commit", "-m", "add encoder and decoder"]);

    // Change the encoder — should activate both tags
    fs::write(
        repo.join("encoder.rs"),
        "// [check:tag WireFormat] Keep in sync\nfn encode() {\n    todo!()\n}\n",
    )
    .unwrap();

    let output = run_marginalia(repo);
    assert!(
        output.status.success(),
        "marginalia should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("tag WireFormat"),
        "stdout should show activated tag: {}",
        stdout
    );
    // Both counterparts should appear
    assert!(
        stdout.contains("encoder.rs"),
        "stdout should mention encoder.rs: {}",
        stdout
    );
    assert!(
        stdout.contains("decoder.rs"),
        "stdout should mention decoder.rs: {}",
        stdout
    );
}

#[test]
fn two_tags_produce_one_message() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    setup_repo(repo);

    // Two files with the same [check:tag], each with its own description
    fs::write(
        repo.join("encoder.rs"),
        "// [check:tag WireFormat] Encoder side\nfn encode() {}\n",
    )
    .unwrap();
    fs::write(
        repo.join("decoder.rs"),
        "// [check:tag WireFormat] Decoder side\nfn decode() {}\n",
    )
    .unwrap();
    git(repo, &["add", "encoder.rs", "decoder.rs"]);
    git(repo, &["commit", "-m", "add encoder and decoder"]);

    // Change the encoder
    fs::write(
        repo.join("encoder.rs"),
        "// [check:tag WireFormat] Encoder side\nfn encode() {\n    todo!()\n}\n",
    )
    .unwrap();

    let output = run_marginalia(repo);
    assert!(
        output.status.success(),
        "marginalia should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should produce exactly one "---" separator (one check item)
    let separator_count = stdout.matches("---").count();
    assert_eq!(
        separator_count, 1,
        "two tags with the same name should produce one message, got {}:\n{}",
        separator_count, stdout
    );

    // Both locations should appear in the check list
    assert!(
        stdout.contains("encoder.rs:1"),
        "should list encoder.rs location: {}",
        stdout
    );
    assert!(
        stdout.contains("decoder.rs:1"),
        "should list decoder.rs location: {}",
        stdout
    );
}

#[test]
fn markdown_html_comments() {
    let source = "<!-- [check:all src/**/*.rs] Update examples -->\nSome text\n";
    let tokens = marginalia::comment_tokens("md").unwrap();
    let comments = marginalia::comments::extract_comments(source, &tokens);
    assert_eq!(comments.len(), 1);
    let anns = marginalia::annotations::extract_annotations(&comments, "README.md");
    assert_eq!(anns.len(), 1);
    assert_eq!(
        anns[0].kind,
        marginalia::annotations::CheckKind::All {
            pattern: "src/**/*.rs".to_string()
        }
    );
}
