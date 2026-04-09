use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use colored::Colorize;

use crate::diff;
use crate::git;
use crate::parse::{chunks_to_string, parse_conflicts};
use crate::resolve::{ResolveOptions, resolve_chunks_with_options};
use crate::types::{Chunk, Conflict, FileResult, UnmergedStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone)]
pub struct GitMediateBuilder {
    root_dir: Option<PathBuf>,
    merge_file: Option<String>,
    set_conflict_style: bool,
    show_diff: bool,
    show_diff2: bool,
    editor: bool,
    color: ColorChoice,
    diff_context: usize,
    dry_run: bool,
    no_add: bool,
    verbose: bool,
    resolve_options: ResolveOptions,
}

impl Default for GitMediateBuilder {
    fn default() -> Self {
        Self {
            root_dir: None,
            merge_file: None,
            set_conflict_style: false,
            show_diff: false,
            show_diff2: false,
            editor: false,
            color: ColorChoice::Auto,
            diff_context: 3,
            dry_run: false,
            no_add: false,
            verbose: false,
            resolve_options: ResolveOptions::default(),
        }
    }
}

impl GitMediateBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn root_dir(mut self, root_dir: impl Into<PathBuf>) -> Self {
        self.root_dir = Some(root_dir.into());
        self
    }

    pub fn merge_file(mut self, merge_file: impl Into<String>) -> Self {
        self.merge_file = Some(merge_file.into());
        self
    }

    pub fn set_conflict_style(mut self, set_conflict_style: bool) -> Self {
        self.set_conflict_style = set_conflict_style;
        self
    }

    pub fn show_diff(mut self, show_diff: bool) -> Self {
        self.show_diff = show_diff;
        self
    }

    pub fn show_diff2(mut self, show_diff2: bool) -> Self {
        self.show_diff2 = show_diff2;
        self
    }

    pub fn editor(mut self, editor: bool) -> Self {
        self.editor = editor;
        self
    }

    pub fn color_choice(mut self, color: ColorChoice) -> Self {
        self.color = color;
        self
    }

    pub fn diff_context(mut self, diff_context: usize) -> Self {
        self.diff_context = diff_context;
        self
    }

    pub fn dry_run(mut self, dry_run: bool) -> Self {
        self.dry_run = dry_run;
        self
    }

    pub fn no_add(mut self, no_add: bool) -> Self {
        self.no_add = no_add;
        self
    }

    pub fn verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    pub fn resolve_options(mut self, resolve_options: ResolveOptions) -> Self {
        self.resolve_options = resolve_options;
        self
    }

    pub fn run(self) -> Result<GitMediateReport> {
        let _cwd_guard = CurrentDirGuard::change_to(self.root_dir.as_deref())?;
        let use_color = self.color.enabled();

        if matches!(self.color, ColorChoice::Always) {
            colored::control::set_override(true);
        } else if matches!(self.color, ColorChoice::Never) {
            colored::control::set_override(false);
        }

        let mut report = GitMediateReport::default();

        let in_repo = match git::repo_root() {
            Ok(root) => Some(root),
            Err(err) => {
                if self.set_conflict_style {
                    git::ensure_diff3_conflict_style(true)?;
                    report.push_line("Set merge.conflictstyle = diff3");
                    return Ok(report);
                }
                return Err(err).context("must be run inside a git repository");
            }
        };

        git::ensure_diff3_conflict_style(self.set_conflict_style)?;
        let _root = in_repo.expect("repo presence already checked");
        if self.set_conflict_style {
            report.push_line("Set merge.conflictstyle = diff3");
        }

        let files_to_process = collect_files_to_process(self.merge_file.as_deref())?;
        if files_to_process.is_empty() {
            report.push_line("No files to process.");
            return Ok(report);
        }

        let mut total = FileResult::default();
        let mut files_fully_resolved = 0usize;

        for (path_str, is_delete_modify) in &files_to_process {
            let path = Path::new(path_str);
            if *is_delete_modify && !self.dry_run {
                git::prepare_delete_modify_conflict(path).with_context(|| {
                    format!("failed to prepare delete/modify conflict for {}", path_str)
                })?;
            }

            let file_outcome = process_file(path, &self)?;
            if let Some(message) = &file_outcome.message {
                report.push_line(message);
            }
            report.extend(file_result_line(path_str, &file_outcome.result, use_color));

            if !file_outcome.remaining_conflicts.is_empty() {
                if self.show_diff {
                    for conflict in &file_outcome.remaining_conflicts {
                        report.push_raw(diff::show_side_diffs(
                            conflict,
                            use_color,
                            self.diff_context,
                        ));
                    }
                }
                if self.show_diff2 {
                    for conflict in &file_outcome.remaining_conflicts {
                        report.push_raw(diff::show_diff2(conflict, use_color, self.diff_context));
                    }
                }
            }

            let removed_empty_file = if *is_delete_modify && !self.dry_run {
                git::remove_file_if_empty(path)?
            } else {
                false
            };

            if file_outcome.result.is_fully_resolved()
                && (file_outcome.result.total_conflicts() > 0 || !file_outcome.had_conflicts)
            {
                files_fully_resolved += 1;
                if !self.dry_run && !self.no_add && !removed_empty_file {
                    git::stage_file(path)
                        .with_context(|| format!("failed to stage {}", path_str))?;
                }
            }

            if self.editor && !file_outcome.remaining_conflicts.is_empty() {
                let first_line = file_outcome.remaining_conflicts[0].start_line();
                if let Err(err) = git::open_editor(path, first_line) {
                    report.push_line(format!(
                        "{} {}: {}",
                        "warning:".yellow(),
                        path_str,
                        err
                    ));
                }
            }

            total.resolved += file_outcome.result.resolved;
            total.partially_resolved += file_outcome.result.partially_resolved;
            total.failed += file_outcome.result.failed;
        }

        report.total = total.clone();
        report.files_resolved = files_fully_resolved;
        report.dry_run = self.dry_run;
        report.no_add = self.no_add;

        if let Some(summary) =
            summary_line(&total, files_fully_resolved, self.dry_run, self.no_add, use_color)
        {
            if !report.output.is_empty() {
                report.push_raw("\n");
            }
            report.push_raw(summary);
        }

        report.exit_code = if total.failed > 0 || total.partially_resolved > 0 {
            1
        } else {
            0
        };

        Ok(report)
    }
}

#[derive(Debug, Clone, Default)]
pub struct GitMediateReport {
    output: String,
    pub total: FileResult,
    pub files_resolved: usize,
    pub dry_run: bool,
    pub no_add: bool,
    exit_code: i32,
}

impl GitMediateReport {
    pub fn output(&self) -> &str {
        &self.output
    }

    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }

    pub fn is_success(&self) -> bool {
        self.exit_code == 0
    }

    pub fn print(&self) {
        print!("{}", self.output);
    }

    fn push_line(&mut self, line: impl AsRef<str>) {
        self.output.push_str(line.as_ref());
        self.output.push('\n');
    }

    fn push_raw(&mut self, text: impl AsRef<str>) {
        self.output.push_str(text.as_ref());
    }

    fn extend(&mut self, text: Option<String>) {
        if let Some(text) = text {
            self.push_line(text);
        }
    }
}

#[derive(Debug)]
struct FileOutcome {
    result: FileResult,
    remaining_conflicts: Vec<Conflict>,
    had_conflicts: bool,
    message: Option<String>,
}

fn collect_files_to_process(merge_file: Option<&str>) -> Result<Vec<(String, bool)>> {
    if let Some(path) = merge_file {
        return Ok(vec![(
            path.to_string(),
            matches!(
                git::unmerged_status(path)?,
                Some(UnmergedStatus::DeletedByUs | UnmergedStatus::DeletedByThem)
            ),
        )]);
    }

    let unmerged = git::unmerged_files()?;
    if unmerged.is_empty() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    for file in unmerged {
        match file.status {
            UnmergedStatus::DeletedByUs | UnmergedStatus::DeletedByThem => {
                files.push((file.path, true));
            }
            UnmergedStatus::BothModified => {
                files.push((file.path, false));
            }
        }
    }
    Ok(files)
}

fn process_file(path: &Path, options: &GitMediateBuilder) -> Result<FileOutcome> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;

    let chunks = match parse_conflicts(&content) {
        Ok(chunks) => chunks,
        Err(err) => {
            return Ok(FileOutcome {
                result: FileResult {
                    failed: 1,
                    ..Default::default()
                },
                remaining_conflicts: Vec::new(),
                had_conflicts: false,
                message: Some(format!("{} {}: {}", "error:".red(), path.display(), err)),
            });
        }
    };
    let had_conflicts = chunks.iter().any(|chunk| matches!(chunk, Chunk::Conflict(_)));
    let (resolved_chunks, result) = resolve_chunks_with_options(chunks, &options.resolve_options);

    let remaining_conflicts = resolved_chunks
        .iter()
        .filter_map(|chunk| match chunk {
            Chunk::Conflict(conflict) => Some(conflict.clone()),
            Chunk::Plain(_) => None,
        })
        .collect::<Vec<_>>();

    if (result.resolved > 0 || result.partially_resolved > 0) && !options.dry_run {
        let new_content = chunks_to_string(&resolved_chunks);
        atomic_write(path, new_content.as_bytes())
            .with_context(|| format!("failed to write {}", path.display()))?;
    }

    Ok(FileOutcome {
        result,
        remaining_conflicts,
        had_conflicts,
        message: None,
    })
}

fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir).context("failed to create temp file")?;
    tmp.write_all(content)
        .context("failed to write temp file")?;
    tmp.persist(path).context("failed to rename temp file")?;
    Ok(())
}

fn file_result_line(path: &str, result: &FileResult, color: bool) -> Option<String> {
    if result.total_conflicts() == 0 {
        return None;
    }

    let mut parts = Vec::new();
    if result.resolved > 0 {
        parts.push(colorize(format!("{} resolved", result.resolved), "green", color));
    }
    if result.partially_resolved > 0 {
        parts.push(colorize(
            format!("{} reduced", result.partially_resolved),
            "yellow",
            color,
        ));
    }
    if result.failed > 0 {
        parts.push(colorize(format!("{} remaining", result.failed), "red", color));
    }

    let status = if result.is_fully_resolved() {
        colorize("ok:".to_string(), "green", color)
    } else {
        colorize("conflict:".to_string(), "red", color)
    };

    Some(format!("{} {} ({})", status, path, parts.join(", ")))
}

fn summary_line(
    total: &FileResult,
    files_resolved: usize,
    dry_run: bool,
    no_add: bool,
    color: bool,
) -> Option<String> {
    if total.total_conflicts() == 0 {
        return None;
    }

    let mut out = String::new();
    let prefix = if dry_run { "(dry run) " } else { "" };
    out.push_str(&format!(
        "{}Summary: {} conflicts in total: {} resolved, {} reduced, {} remaining\n",
        prefix,
        total.total_conflicts(),
        colorize(total.resolved.to_string(), "green", color),
        colorize(total.partially_resolved.to_string(), "yellow", color),
        colorize(total.failed.to_string(), "red", color),
    ));

    if files_resolved > 0 && !dry_run && !no_add {
        out.push_str(&format!(
            "Staged {} fully resolved file{}.\n",
            files_resolved,
            if files_resolved == 1 { "" } else { "s" }
        ));
    }

    Some(out)
}

fn colorize(text: String, color_name: &str, enabled: bool) -> String {
    if !enabled {
        return text;
    }

    match color_name {
        "green" => text.green().to_string(),
        "yellow" => text.yellow().to_string(),
        "red" => text.red().to_string(),
        _ => text,
    }
}

impl ColorChoice {
    fn enabled(self) -> bool {
        match self {
            ColorChoice::Auto => std::io::IsTerminal::is_terminal(&std::io::stdout()),
            ColorChoice::Always => true,
            ColorChoice::Never => false,
        }
    }
}

struct CurrentDirGuard {
    previous: PathBuf,
}

impl CurrentDirGuard {
    fn change_to(path: Option<&Path>) -> Result<Option<Self>> {
        let Some(path) = path else {
            return Ok(None);
        };

        let previous = std::env::current_dir().context("failed to read current directory")?;
        std::env::set_current_dir(path)
            .with_context(|| format!("failed to change directory to {}", path.display()))?;
        Ok(Some(Self { previous }))
    }
}

impl Drop for CurrentDirGuard {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.previous);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn builder_uses_root_dir() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir(&repo).unwrap();

        let git = |args: &[&str]| {
            let out = Command::new("git")
                .args(args)
                .current_dir(&repo)
                .env("GIT_AUTHOR_NAME", "Test")
                .env("GIT_AUTHOR_EMAIL", "test@test.com")
                .env("GIT_COMMITTER_NAME", "Test")
                .env("GIT_COMMITTER_EMAIL", "test@test.com")
                .output()
                .unwrap();
            assert!(out.status.success(), "git {:?} failed", args);
        };

        git(&["init"]);
        git(&["config", "merge.conflictstyle", "diff3"]);
        std::fs::write(
            repo.join("file.txt"),
            "<<<<<<< HEAD\nbase\n||||||| base\nbase\n=======\ntheirs\n>>>>>>> branch\n",
        )
        .unwrap();

        let previous = std::env::current_dir().unwrap();
        let report = GitMediateBuilder::new()
            .root_dir(&repo)
            .merge_file("file.txt")
            .run()
            .unwrap();
        assert!(report.is_success());
        assert_eq!(std::env::current_dir().unwrap(), previous);
        assert_eq!(std::fs::read_to_string(repo.join("file.txt")).unwrap(), "theirs\n");
    }
}
