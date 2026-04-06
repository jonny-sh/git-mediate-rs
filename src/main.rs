use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use clap::Parser;
use colored::Colorize;

use git_mediate::diff;
use git_mediate::git;
use git_mediate::parse::{chunks_to_string, parse_conflicts};
use git_mediate::resolve::resolve_chunks;
use git_mediate::types::{Chunk, FileResult, UnmergedStatus};

#[derive(Parser)]
#[command(
    name = "git-mediate",
    version,
    about = "Automatically resolve trivial git merge conflicts"
)]
struct Cli {
    /// Open $EDITOR on files with remaining conflicts
    #[arg(short = 'e', long = "editor")]
    editor: bool,

    /// Show diff of each side against the base for remaining conflicts
    #[arg(short = 'd', long = "diff")]
    show_diff: bool,

    /// Show diff between the two sides for remaining conflicts
    #[arg(short = '2', long = "diff2")]
    show_diff2: bool,

    /// Set merge.conflictstyle to diff3
    #[arg(short = 's', long = "set-conflict-style")]
    set_conflict_style: bool,

    /// Process only this file instead of all unmerged files
    #[arg(short = 'f', long = "merge-file")]
    merge_file: Option<String>,

    /// Force colored output
    #[arg(short = 'c', long = "color")]
    color: bool,

    /// Disable colored output
    #[arg(short = 'C', long = "no-color")]
    no_color: bool,

    /// Only print what would be done, don't modify files
    #[arg(short = 'n', long)]
    dry_run: bool,

    /// Don't stage resolved files with `git add`
    #[arg(long)]
    no_add: bool,

    /// Be verbose about what's happening
    #[arg(short, long)]
    verbose: bool,
}

impl Cli {
    fn use_color(&self) -> bool {
        if self.no_color {
            return false;
        }
        if self.color {
            return true;
        }
        // Auto-detect: color if stdout is a terminal
        std::io::IsTerminal::is_terminal(&std::io::stdout())
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.no_color {
        colored::control::set_override(false);
    } else if cli.color {
        colored::control::set_override(true);
    }

    // Handle -s: set conflict style and exit
    if cli.set_conflict_style {
        git::set_conflict_style()?;
        println!("Set merge.conflictstyle = diff3");
        return Ok(());
    }

    // Check we're in a git repo
    let _root = git::repo_root().context("must be run inside a git repository")?;

    // Verify diff3 conflict style
    git::ensure_diff3_conflict_style()?;

    // Get file list
    let files_to_process: Vec<_> = if let Some(ref path) = cli.merge_file {
        vec![path.clone()]
    } else {
        let unmerged = git::unmerged_files()?;
        if unmerged.is_empty() {
            println!("No unmerged files.");
            return Ok(());
        }

        let mut paths = Vec::new();
        for file in &unmerged {
            match file.status {
                UnmergedStatus::DeletedByUs | UnmergedStatus::DeletedByThem => {
                    println!(
                        "{} {} (deleted on one side, skipping)",
                        "skip:".yellow(),
                        file.path
                    );
                }
                UnmergedStatus::BothModified => {
                    paths.push(file.path.clone());
                }
            }
        }
        paths
    };

    if files_to_process.is_empty() {
        println!("No files to process.");
        return Ok(());
    }

    let mut total = FileResult::default();
    let mut files_fully_resolved = 0usize;
    let use_color = cli.use_color();

    for path_str in &files_to_process {
        let path = Path::new(path_str);
        let (result, remaining_conflicts) = process_file(path, &cli)?;
        print_file_result(path_str, &result);

        // Show diffs for remaining conflicts
        if !remaining_conflicts.is_empty() {
            if cli.show_diff {
                for conflict in &remaining_conflicts {
                    print!("{}", diff::show_side_diffs(conflict, use_color));
                }
            }
            if cli.show_diff2 {
                for conflict in &remaining_conflicts {
                    print!("{}", diff::show_diff2(conflict, use_color));
                }
            }
        }

        if result.is_fully_resolved() && result.total_conflicts() > 0 {
            files_fully_resolved += 1;
            if !cli.dry_run && !cli.no_add {
                git::stage_file(path).with_context(|| format!("failed to stage {}", path_str))?;
            }
        }

        // Open editor on files with remaining conflicts
        if cli.editor && !remaining_conflicts.is_empty() {
            let first_line = remaining_conflicts[0].start_line();
            if let Err(e) = git::open_editor(path, first_line) {
                eprintln!("{} {}: {}", "warning:".yellow(), path_str, e);
            }
        }

        total.resolved += result.resolved;
        total.partially_resolved += result.partially_resolved;
        total.failed += result.failed;
    }

    // Print summary
    println!();
    print_summary(&total, files_fully_resolved, &cli);

    if total.failed > 0 || total.partially_resolved > 0 {
        std::process::exit(1);
    }

    Ok(())
}

/// Process a file: parse, resolve, write back.
/// Returns the stats and any remaining (unresolved) conflicts.
fn process_file(path: &Path, cli: &Cli) -> Result<(FileResult, Vec<git_mediate::types::Conflict>)> {
    let content =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;

    let chunks = match parse_conflicts(&content) {
        Ok(chunks) => chunks,
        Err(e) => {
            eprintln!("{} {}: {}", "error:".red(), path.display(), e);
            return Ok((
                FileResult {
                    failed: 1,
                    ..Default::default()
                },
                Vec::new(),
            ));
        }
    };

    let (resolved_chunks, stats) = resolve_chunks(chunks);

    // Collect remaining conflicts for diff display / editor
    let remaining: Vec<_> = resolved_chunks
        .iter()
        .filter_map(|c| match c {
            Chunk::Conflict(conflict) => Some(conflict.clone()),
            _ => None,
        })
        .collect();

    if stats.resolved > 0 && !cli.dry_run {
        let new_content = chunks_to_string(&resolved_chunks);
        atomic_write(path, new_content.as_bytes())
            .with_context(|| format!("failed to write {}", path.display()))?;
    }

    Ok((stats, remaining))
}

/// Write content to a file atomically: write to a temp file in the same
/// directory, then rename over the target.
fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir).context("failed to create temp file")?;
    tmp.write_all(content)
        .context("failed to write temp file")?;
    tmp.persist(path).context("failed to rename temp file")?;
    Ok(())
}

fn print_file_result(path: &str, result: &FileResult) {
    if result.total_conflicts() == 0 {
        return;
    }

    let mut parts = Vec::new();
    if result.resolved > 0 {
        parts.push(format!("{} resolved", result.resolved).green().to_string());
    }
    if result.partially_resolved > 0 {
        parts.push(
            format!("{} reduced", result.partially_resolved)
                .yellow()
                .to_string(),
        );
    }
    if result.failed > 0 {
        parts.push(format!("{} remaining", result.failed).red().to_string());
    }

    let status = if result.is_fully_resolved() {
        "ok:".green().to_string()
    } else {
        "conflict:".red().to_string()
    };

    println!("{} {} ({})", status, path, parts.join(", "));
}

fn print_summary(total: &FileResult, files_resolved: usize, cli: &Cli) {
    let total_conflicts = total.total_conflicts();
    if total_conflicts == 0 {
        return;
    }

    let prefix = if cli.dry_run { "(dry run) " } else { "" };

    println!(
        "{}Summary: {} conflicts in total: {} resolved, {} reduced, {} remaining",
        prefix,
        total_conflicts,
        total.resolved.to_string().green(),
        total.partially_resolved.to_string().yellow(),
        total.failed.to_string().red(),
    );

    if files_resolved > 0 && !cli.dry_run && !cli.no_add {
        println!(
            "Staged {} fully resolved file{}.",
            files_resolved,
            if files_resolved == 1 { "" } else { "s" }
        );
    }
}
