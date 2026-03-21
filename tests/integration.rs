use std::fs;
use std::path::Path;
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
