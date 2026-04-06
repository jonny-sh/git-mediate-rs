use std::fs;
use std::process::Command;

use git_mediate::parse::{chunks_to_string, parse_conflicts};
use git_mediate::resolve::resolve_chunks;

/// Helper: create a temp dir with a git repo that has a merge conflict.
fn setup_conflict_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();

    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(p)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };

    git(&["init"]);
    git(&["config", "merge.conflictstyle", "diff3"]);

    // Base commit
    fs::write(p.join("file.txt"), "line1\nline2\nline3\n").unwrap();
    git(&["add", "file.txt"]);
    git(&["commit", "-m", "base"]);

    // Branch A
    git(&["checkout", "-b", "branch-a"]);
    fs::write(p.join("file.txt"), "line1\nmodified-by-a\nline3\n").unwrap();
    git(&["add", "file.txt"]);
    git(&["commit", "-m", "change a"]);

    // Branch B
    git(&["checkout", "main"]);
    git(&["checkout", "-b", "branch-b"]);
    fs::write(p.join("file.txt"), "line1\nline2\nmodified-by-b\n").unwrap();
    git(&["add", "file.txt"]);
    git(&["commit", "-m", "change b"]);

    // Merge (will conflict with diff3 style)
    let out = Command::new("git")
        .args(["merge", "branch-a"])
        .current_dir(p)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .unwrap();
    // Merge should fail with conflicts
    assert!(!out.status.success());

    dir
}

#[test]
fn test_full_pipeline_on_real_conflict() {
    let dir = setup_conflict_repo();
    let file_path = dir.path().join("file.txt");

    let content = fs::read_to_string(&file_path).unwrap();

    // Should have conflict markers
    assert!(content.contains("<<<<<<<"));
    assert!(content.contains("|||||||"));
    assert!(content.contains(">>>>>>>"));

    // Parse
    let chunks = parse_conflicts(&content).unwrap();
    let conflict_count = chunks
        .iter()
        .filter(|c| matches!(c, git_mediate::types::Chunk::Conflict(_)))
        .count();
    assert!(conflict_count > 0, "should have at least one conflict");

    // Resolve
    let (resolved_chunks, stats) = resolve_chunks(chunks);

    // Both sides changed different lines relative to base, and with
    // prefix/suffix reduction this should be resolvable
    // (line1 is common prefix, the conflicting middle should resolve
    // since each side changed a different line from base)
    let output = chunks_to_string(&resolved_chunks);

    // The output should not contain conflict markers if fully resolved
    if stats.is_fully_resolved() {
        assert!(
            !output.contains("<<<<<<<"),
            "resolved output should not have markers"
        );
        assert!(output.contains("modified-by-a"));
        assert!(output.contains("modified-by-b"));
    }

    // Write back and verify
    fs::write(&file_path, &output).unwrap();
    let final_content = fs::read_to_string(&file_path).unwrap();
    assert_eq!(final_content, output);
}

#[test]
fn test_true_conflict_not_resolved() {
    // Both sides modify the same line differently
    let content = "\
<<<<<<< HEAD
line modified by A
||||||| ancestor
original line
=======
line modified by B
>>>>>>> branch
";
    let chunks = parse_conflicts(content).unwrap();
    let (_, stats) = resolve_chunks(chunks);

    assert_eq!(stats.resolved, 0);
    assert!(stats.failed > 0 || stats.partially_resolved > 0);
}

#[test]
fn test_one_side_unchanged_resolves() {
    let content = "\
header
<<<<<<< HEAD
unchanged base line
||||||| ancestor
unchanged base line
=======
new line from theirs
>>>>>>> branch
footer
";
    let chunks = parse_conflicts(content).unwrap();
    let (resolved, stats) = resolve_chunks(chunks);

    assert_eq!(stats.resolved, 1);
    assert_eq!(stats.failed, 0);

    let output = chunks_to_string(&resolved);
    assert!(!output.contains("<<<<<<<"));
    assert!(output.contains("header"));
    assert!(output.contains("new line from theirs"));
    assert!(output.contains("footer"));
}

#[test]
fn test_multiple_conflicts_partial_resolution() {
    let content = "\
<<<<<<< HEAD
base
||||||| ancestor
base
=======
theirs1
>>>>>>> branch
between
<<<<<<< HEAD
ours2
||||||| ancestor
base2
=======
theirs2
>>>>>>> branch
";
    let chunks = parse_conflicts(content).unwrap();
    let (resolved, stats) = resolve_chunks(chunks);

    // First: a==base → take b (resolved)
    // Second: true conflict (unchanged)
    assert_eq!(stats.resolved, 1);
    assert_eq!(stats.failed, 1);

    let output = chunks_to_string(&resolved);
    assert!(output.contains("theirs1"));
    assert!(output.contains("<<<<<<<")); // second conflict remains
}

#[test]
fn test_roundtrip_preserves_unresolvable() {
    let content = "\
before
<<<<<<< HEAD
ours
||||||| ancestor
base
=======
theirs
>>>>>>> branch
after
";
    let chunks = parse_conflicts(content).unwrap();
    let (resolved, _) = resolve_chunks(chunks);
    let output = chunks_to_string(&resolved);

    // Should be unchanged since the conflict can't be resolved
    assert_eq!(content, output);
}

#[test]
fn test_set_conflict_style_works_outside_repo() {
    let home = tempfile::tempdir().unwrap();
    let config_home = home.path().join("xdg");
    fs::create_dir_all(&config_home).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_git-mediate"))
        .arg("-s")
        .current_dir(home.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", &config_home)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "git-mediate -s failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let git_config = Command::new("git")
        .args(["config", "--global", "merge.conflictstyle"])
        .current_dir(home.path())
        .env("HOME", home.path())
        .env("XDG_CONFIG_HOME", &config_home)
        .output()
        .unwrap();

    assert!(
        git_config.status.success(),
        "git config --global failed: {}",
        String::from_utf8_lossy(&git_config.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&git_config.stdout).trim(), "diff3");
}

#[test]
fn test_set_conflict_style_continues_processing() {
    let dir = setup_conflict_repo();
    let home = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("file.txt");

    let unset_local = Command::new("git")
        .args(["config", "--unset", "merge.conflictstyle"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(unset_local.status.success());

    let set_global = Command::new("git")
        .args(["config", "--global", "merge.conflictstyle", "diff2"])
        .current_dir(dir.path())
        .env("HOME", home.path())
        .output()
        .unwrap();
    assert!(set_global.status.success());

    fs::write(&file_path, "line1\nmanual-resolution\nline3\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_git-mediate"))
        .arg("-s")
        .current_dir(dir.path())
        .env("HOME", home.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "git-mediate -s failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let global_style = Command::new("git")
        .args(["config", "--global", "merge.conflictstyle"])
        .current_dir(dir.path())
        .env("HOME", home.path())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&global_style.stdout).trim(), "diff3");

    let status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&status.stdout).trim(), "M  file.txt");
}

#[test]
fn test_set_conflict_style_fails_when_local_override_remains() {
    let dir = setup_conflict_repo();
    let home = tempfile::tempdir().unwrap();

    let output = Command::new("git")
        .args(["config", "merge.conflictstyle", "diff2"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_git-mediate"))
        .arg("-s")
        .current_dir(dir.path())
        .env("HOME", home.path())
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "git-mediate -s unexpectedly succeeded with a repo-local override"
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("override"));
}

#[test]
fn test_partial_reduction_is_written_back() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("file.txt");
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };

    git(&["init"]);
    git(&["config", "merge.conflictstyle", "diff3"]);

    let original = "\
<<<<<<< HEAD
common
ours
tail
||||||| ancestor
common
base
tail
=======
common
theirs
tail
>>>>>>> branch
";
    fs::write(&file_path, original).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_git-mediate"))
        .args(["--merge-file", "file.txt"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "git-mediate should still fail on reduced-but-unresolved conflicts"
    );

    let rewritten = fs::read_to_string(&file_path).unwrap();
    assert_ne!(rewritten, original, "reduced conflict should be written back");
    assert!(!rewritten.contains("common\nours\ntail"));
    assert!(!rewritten.contains("common\nbase\ntail"));
    assert!(!rewritten.contains("common\ntheirs\ntail"));
    assert!(rewritten.contains("<<<<<<< HEAD\nours"));
    assert!(rewritten.contains("||||||| ancestor\nbase"));
    assert!(rewritten.contains("=======\ntheirs"));
}

#[test]
fn test_manually_resolved_file_gets_staged() {
    let dir = setup_conflict_repo();
    let file_path = dir.path().join("file.txt");

    fs::write(&file_path, "line1\nmanual-resolution\nline3\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_git-mediate"))
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "git-mediate failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "git status failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&status.stdout).trim(), "M  file.txt");
}

#[test]
fn test_delete_modify_conflict_is_prepared_for_mediation() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();

    let git = |args: &[&str]| {
        let out = Command::new("git")
            .args(args)
            .current_dir(p)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@test.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@test.com")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };

    git(&["init"]);
    git(&["config", "merge.conflictstyle", "diff3"]);
    fs::write(p.join("file.txt"), "hello\n").unwrap();
    git(&["add", "file.txt"]);
    git(&["commit", "-m", "base"]);

    git(&["checkout", "-b", "delete-branch"]);
    git(&["rm", "file.txt"]);
    git(&["commit", "-m", "delete"]);

    git(&["checkout", "main"]);
    git(&["checkout", "-b", "modify-branch"]);
    fs::write(p.join("file.txt"), "hello\nchange\n").unwrap();
    git(&["add", "file.txt"]);
    git(&["commit", "-m", "modify"]);

    let merge = Command::new("git")
        .args(["merge", "delete-branch"])
        .current_dir(p)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .unwrap();
    assert!(!merge.status.success());

    let output = Command::new(env!("CARGO_BIN_EXE_git-mediate"))
        .current_dir(p)
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "git-mediate should still fail on unresolved delete/modify conflicts"
    );

    let content = fs::read_to_string(p.join("file.txt")).unwrap();
    assert!(content.contains("<<<<<<< LOCAL"));
    assert!(content.contains("||||||| BASE"));
    assert!(content.contains(">>>>>>> REMOTE"));

    let status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(p)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&status.stdout).trim(), "UD file.txt");
}
