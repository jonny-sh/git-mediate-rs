use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use clap::Parser;
use colored::Colorize;

use git_mediate::git;
use git_mediate::parse::{chunks_to_string, parse_conflicts};
use git_mediate::resolve::resolve_chunks;
use git_mediate::types::{FileResult, UnmergedStatus};

#[derive(Parser)]
#[command(name = "git-mediate", about = "Automatically resolve trivial git merge conflicts")]
struct Cli {
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

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Check we're in a git repo
    let _root = git::repo_root().context("must be run inside a git repository")?;

    // Verify diff3 conflict style
    git::ensure_diff3_conflict_style()?;

    // Get unmerged files
    let unmerged = git::unmerged_files()?;

    if unmerged.is_empty() {
        println!("No unmerged files.");
        return Ok(());
    }

    let mut total = FileResult::default();
    let mut files_fully_resolved = 0usize;

    for file in &unmerged {
        match file.status {
            UnmergedStatus::DeletedByUs | UnmergedStatus::DeletedByThem => {
                println!(
                    "{} {} (deleted on one side, skipping)",
                    "skip:".yellow(),
                    file.path
                );
                continue;
            }
            UnmergedStatus::BothModified => {}
        }

        let result = process_file(Path::new(&file.path), &cli)?;
        print_file_result(&file.path, &result, &cli);

        if result.is_fully_resolved() && result.total_conflicts() > 0 {
            files_fully_resolved += 1;
            if !cli.dry_run && !cli.no_add {
                git::stage_file(Path::new(&file.path))
                    .with_context(|| format!("failed to stage {}", file.path))?;
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

fn process_file(path: &Path, cli: &Cli) -> Result<FileResult> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;

    let chunks = match parse_conflicts(&content) {
        Ok(chunks) => chunks,
        Err(e) => {
            eprintln!("{} {}: {}", "error:".red(), path.display(), e);
            return Ok(FileResult {
                failed: 1,
                ..Default::default()
            });
        }
    };

    let (resolved_chunks, stats) = resolve_chunks(chunks);

    if stats.resolved > 0 && !cli.dry_run {
        let new_content = chunks_to_string(&resolved_chunks);
        fs::write(path, &new_content)
            .with_context(|| format!("failed to write {}", path.display()))?;
    }

    Ok(stats)
}

fn print_file_result(path: &str, result: &FileResult, cli: &Cli) {
    if result.total_conflicts() == 0 {
        if cli.verbose {
            println!("{} {} (no conflicts)", "skip:".dimmed(), path);
        }
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
