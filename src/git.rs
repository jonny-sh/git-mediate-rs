use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::types::{UnmergedFile, UnmergedStatus};

/// Find the root of the current git repository.
pub fn repo_root() -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("failed to run git")?;
    if !output.status.success() {
        bail!(
            "not a git repository: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let root = String::from_utf8(output.stdout)
        .context("invalid utf-8 in git output")?
        .trim()
        .to_string();
    Ok(PathBuf::from(root))
}

/// Get the list of unmerged files from `git status`.
pub fn unmerged_files() -> Result<Vec<UnmergedFile>> {
    let output = Command::new("git")
        .args(["status", "--porcelain", "-z"])
        .output()
        .context("failed to run git status")?;
    if !output.status.success() {
        bail!(
            "git status failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let raw = String::from_utf8(output.stdout).context("invalid utf-8 in git status output")?;
    parse_status_output(&raw)
}

/// Parse `git status --porcelain -z` output for unmerged entries.
fn parse_status_output(raw: &str) -> Result<Vec<UnmergedFile>> {
    let mut files = Vec::new();

    // -z separates entries with NUL. Each entry is "XY path" (or "XY path\0newpath" for renames).
    for entry in raw.split('\0') {
        if entry.len() < 3 {
            continue;
        }
        let xy = &entry[..2];
        let path = entry[3..].to_string();

        let status = match xy {
            "UU" => UnmergedStatus::BothModified,
            "AA" => UnmergedStatus::BothModified,
            "DA" | "AD" => UnmergedStatus::BothModified,
            "DU" => UnmergedStatus::DeletedByUs,
            "UD" => UnmergedStatus::DeletedByThem,
            "DD" | "AU" | "UA" => continue,
            _ => continue,
        };

        files.push(UnmergedFile { status, path });
    }

    Ok(files)
}

pub fn unmerged_status(path: &str) -> Result<Option<UnmergedStatus>> {
    Ok(unmerged_files()?
        .into_iter()
        .find(|file| file.path == path)
        .map(|file| file.status))
}

fn current_conflict_style() -> Result<Option<String>> {
    let output = Command::new("git")
        .args(["config", "merge.conflictstyle"])
        .output()
        .context("failed to run git config")?;

    if output.status.success() {
        return Ok(Some(String::from_utf8_lossy(&output.stdout).trim().to_string()));
    }

    if output.status.code() == Some(1) {
        return Ok(None);
    }

    bail!(
        "git config failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
}

/// Ensure `merge.conflictstyle` is set to `diff3`.
pub fn ensure_diff3_conflict_style(set_if_needed: bool) -> Result<()> {
    let current = current_conflict_style()?;

    if !matches!(current.as_deref(), Some("diff3" | "zdiff3")) {
        if !set_if_needed {
            bail!(
                "merge.conflictstyle is '{}', but git-mediate requires 'diff3' (or 'zdiff3').\n\
                 Run: git config merge.conflictstyle diff3",
                current.as_deref().unwrap_or("unset")
            );
        }

        set_conflict_style()?;

        let updated = current_conflict_style()?;
        if updated.as_deref() != Some("diff3") {
            bail!(
                "attempt to set merge.conflictstyle failed; a repo-local git config may still override it"
            );
        }
    }

    Ok(())
}

/// Set `merge.conflictstyle` to `diff3`.
pub fn set_conflict_style() -> Result<()> {
    let output = Command::new("git")
        .args(["config", "--global", "merge.conflictstyle", "diff3"])
        .output()
        .context("failed to run git config")?;
    if !output.status.success() {
        bail!(
            "failed to set merge.conflictstyle: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

/// Stage a file with `git add`.
pub fn stage_file(path: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["add", "--"])
        .arg(path)
        .output()
        .context("failed to run git add")?;
    if !output.status.success() {
        bail!(
            "git add failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

pub fn prepare_delete_modify_conflict(path: &Path) -> Result<()> {
    let current =
        std::fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    if current.lines().any(|line| line.starts_with("<<<<<<<")) {
        return Ok(());
    }

    let base = read_stage_file(path, 1)?.unwrap_or_default();
    let ours = read_stage_file(path, 2)?.unwrap_or_default();
    let theirs = read_stage_file(path, 3)?.unwrap_or_default();

    let conflict = format!(
        "<<<<<<< LOCAL\n{}||||||| BASE\n{}=======\n{}>>>>>>> REMOTE\n",
        ensure_trailing_newline(&ours),
        ensure_trailing_newline(&base),
        ensure_trailing_newline(&theirs)
    );

    std::fs::write(path, conflict)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

pub fn remove_file_if_empty(path: &Path) -> Result<bool> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    if !content.is_empty() {
        return Ok(false);
    }

    std::fs::remove_file(path).with_context(|| format!("failed to remove {}", path.display()))?;
    let output = Command::new("git")
        .args(["add", "-u", "--"])
        .arg(path)
        .output()
        .context("failed to run git add -u")?;
    if !output.status.success() {
        bail!(
            "git add -u failed for {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(true)
}

/// Open the user's editor at a specific file and line.
pub fn open_editor(path: &Path, line: usize) -> Result<()> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());

    let status = if editor.contains("code") {
        // VS Code uses --goto file:line
        Command::new(&editor)
            .arg("--goto")
            .arg(format!("{}:{}", path.display(), line))
            .status()
    } else {
        // Most editors use +line file
        Command::new(&editor)
            .arg(format!("+{}", line))
            .arg(path)
            .status()
    };

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => bail!("editor exited with status {}", s),
        Err(e) => bail!("failed to launch editor '{}': {}", editor, e),
    }
}

fn read_stage_file(path: &Path, stage: u8) -> Result<Option<String>> {
    let output = Command::new("git")
        .arg("show")
        .arg(format!(":{}:{}", stage, path.to_string_lossy()))
        .output()
        .context("failed to run git show")?;
    if output.status.success() {
        return String::from_utf8(output.stdout)
            .map(Some)
            .context("invalid utf-8 in git show output");
    }

    Ok(None)
}

fn ensure_trailing_newline(content: &str) -> String {
    if content.is_empty() || content.ends_with('\n') {
        content.to_string()
    } else {
        format!("{content}\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_status_both_modified() {
        let raw = "UU src/main.rs\0UU src/lib.rs\0";
        let files = parse_status_output(raw).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "src/main.rs");
        assert_eq!(files[0].status, UnmergedStatus::BothModified);
        assert_eq!(files[1].path, "src/lib.rs");
    }

    #[test]
    fn test_parse_status_mixed() {
        let raw = "UU conflict.rs\0M  clean.rs\0DU deleted_by_us.rs\0";
        let files = parse_status_output(raw).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].status, UnmergedStatus::BothModified);
        assert_eq!(files[1].status, UnmergedStatus::DeletedByUs);
    }

    #[test]
    fn test_parse_status_empty() {
        let files = parse_status_output("").unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn test_parse_status_no_unmerged() {
        let raw = "M  modified.rs\0A  added.rs\0";
        let files = parse_status_output(raw).unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn test_parse_status_add_add_and_delete_add() {
        let raw = "AA both_added.rs\0DA delete_add.rs\0AD add_delete.rs\0";
        let files = parse_status_output(raw).unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].status, UnmergedStatus::BothModified);
        assert_eq!(files[1].status, UnmergedStatus::BothModified);
        assert_eq!(files[2].status, UnmergedStatus::BothModified);
    }
}
