use git2::{DiffOptions, Repository};
use std::path::Path;

/// A range of changed lines in a file (1-based, inclusive).
#[derive(Debug, Clone)]
pub struct Hunk {
    pub new_start: usize,
    pub new_end: usize,
}

/// A file that was changed in the diff.
#[derive(Debug, Clone)]
pub struct ChangedFile {
    pub path: String,
    pub hunks: Vec<Hunk>,
}

/// Find the merge base between HEAD and the given base branch, then return
/// the list of changed files with their changed line ranges.
///
/// This includes committed changes on the current branch, staged changes,
/// and unstaged working directory changes.
pub fn changed_files(repo_path: &Path, base_branch: &str) -> Result<Vec<ChangedFile>, String> {
    let repo = Repository::open(repo_path).map_err(|e| format!("failed to open repo: {}", e))?;

    let base_ref = resolve_base(&repo, base_branch)?;
    let base_commit = repo
        .find_commit(base_ref)
        .map_err(|e| format!("failed to find base commit: {}", e))?;

    let head_ref = repo
        .head()
        .map_err(|e| format!("failed to get HEAD: {}", e))?;
    let head_commit = head_ref
        .peel_to_commit()
        .map_err(|e| format!("failed to peel HEAD to commit: {}", e))?;

    let merge_base = repo
        .merge_base(base_commit.id(), head_commit.id())
        .map_err(|e| format!("failed to find merge base: {}", e))?;

    let merge_base_commit = repo
        .find_commit(merge_base)
        .map_err(|e| format!("failed to find merge base commit: {}", e))?;

    let base_tree = merge_base_commit
        .tree()
        .map_err(|e| format!("failed to get base tree: {}", e))?;

    // Diff the merge base against the working directory (includes both
    // committed, staged, and unstaged changes).
    let mut opts = DiffOptions::new();
    let mut diff = repo
        .diff_tree_to_workdir_with_index(Some(&base_tree), Some(&mut opts))
        .map_err(|e| format!("failed to compute diff: {}", e))?;

    // Also include committed changes between merge base and HEAD, in case
    // the working directory is clean but the branch has commits.
    let head_tree = head_commit
        .tree()
        .map_err(|e| format!("failed to get HEAD tree: {}", e))?;
    let committed_diff = repo
        .diff_tree_to_tree(Some(&base_tree), Some(&head_tree), None)
        .map_err(|e| format!("failed to compute committed diff: {}", e))?;
    diff.merge(&committed_diff)
        .map_err(|e| format!("failed to merge diffs: {}", e))?;

    let mut files: Vec<ChangedFile> = Vec::new();

    for delta_idx in 0..diff.deltas().len() {
        let delta = diff.get_delta(delta_idx).unwrap();
        let path = match delta.new_file().path().and_then(|p| p.to_str()) {
            Some(p) => p.to_string(),
            None => continue,
        };

        let patch = git2::Patch::from_diff(&diff, delta_idx)
            .map_err(|e| format!("failed to get patch: {}", e))?;

        let mut hunks = Vec::new();
        if let Some(ref patch) = patch {
            for hunk_idx in 0..patch.num_hunks() {
                let (hunk, _) = patch
                    .hunk(hunk_idx)
                    .map_err(|e| format!("failed to get hunk: {}", e))?;
                let start = hunk.new_start() as usize;
                let lines = hunk.new_lines() as usize;
                let end = if lines == 0 { start } else { start + lines - 1 };
                hunks.push(Hunk {
                    new_start: start,
                    new_end: end,
                });
            }
        }

        // Deduplicate: if we already have this file (from the merge), skip it
        if files.iter().any(|f| f.path == path) {
            continue;
        }

        if !hunks.is_empty() {
            files.push(ChangedFile { path, hunks });
        }
    }

    Ok(files)
}

/// Try to auto-detect the base branch if "auto" is given.
pub fn detect_base_branch(repo_path: &Path) -> Result<String, String> {
    let repo = Repository::open(repo_path).map_err(|e| format!("failed to open repo: {}", e))?;

    for candidate in &["main", "master"] {
        if resolve_base(&repo, candidate).is_ok() {
            return Ok(candidate.to_string());
        }
    }

    Err("could not auto-detect base branch (tried main, master)".to_string())
}

fn resolve_base(repo: &Repository, base: &str) -> Result<git2::Oid, String> {
    // Try as a local branch first
    if let Ok(reference) = repo.find_branch(base, git2::BranchType::Local) {
        if let Some(target) = reference.get().target() {
            return Ok(target);
        }
    }

    // Try as a remote tracking branch
    for remote in &["origin", "upstream"] {
        let remote_ref = format!("{}/{}", remote, base);
        if let Ok(reference) = repo.find_branch(&remote_ref, git2::BranchType::Remote) {
            if let Some(target) = reference.get().target() {
                return Ok(target);
            }
        }
    }

    // Try as a raw ref
    repo.revparse_single(base)
        .map(|obj| obj.id())
        .map_err(|e| format!("could not resolve '{}': {}", base, e))
}
