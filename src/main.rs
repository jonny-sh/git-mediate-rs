use anyhow::Result;
use clap::{Args, Parser};

use git_mediate::mediate::{ColorChoice, GitMediateBuilder};
use git_mediate::resolve::ResolveOptions;

#[derive(Debug, Clone, Args, Default)]
struct ResolutionCliArgs {
    #[arg(
        short = 'U',
        long = "context",
        help = "Number of context lines to show in conflict diffs"
    )]
    context: Option<usize>,

    #[arg(
        long = "untabify",
        value_name = "TABSIZE",
        help = "Normalize tabs to spaces before resolving conflicts"
    )]
    untabify: Option<usize>,

    #[arg(long = "trivial", help = "Enable trivial conflict resolution")]
    trivial: bool,
    #[arg(long = "no-trivial", help = "Disable trivial conflict resolution")]
    no_trivial: bool,

    #[arg(
        long = "reduce",
        help = "Enable prefix, suffix, and common-block reduction"
    )]
    reduce: bool,
    #[arg(
        long = "no-reduce",
        help = "Disable prefix, suffix, and common-block reduction"
    )]
    no_reduce: bool,

    #[arg(long = "line-endings", help = "Enable line-ending normalization")]
    line_endings: bool,
    #[arg(long = "no-line-endings", help = "Disable line-ending normalization")]
    no_line_endings: bool,

    #[arg(
        long = "lines-added-around",
        help = "Resolve conflicts where both sides added lines around unchanged base text"
    )]
    lines_added_around: bool,

    #[arg(
        long = "reduce-deleted",
        help = "Reduce delete/modify conflicts by stripping common non-deleted context"
    )]
    reduce_deleted: bool,

    #[arg(
        long = "split-markers",
        help = "Enable splitting conflicts at matched markers"
    )]
    split_markers: bool,
    #[arg(
        long = "no-split-markers",
        help = "Disable splitting conflicts at matched markers"
    )]
    no_split_markers: bool,

    #[arg(
        long = "indentation",
        help = "Resolve conflicts where one side only changed indentation"
    )]
    indentation: bool,
    #[arg(long = "no-indentation", help = "Disable indentation-aware resolution")]
    no_indentation: bool,
}

impl ResolutionCliArgs {
    fn apply(&self, mut options: ResolveOptions) -> ResolveOptions {
        if self.trivial {
            options.trivial = true;
        }
        if self.no_trivial {
            options.trivial = false;
        }
        if self.reduce {
            options.reduce = true;
        }
        if self.no_reduce {
            options.reduce = false;
        }
        if let Some(tabsize) = self.untabify {
            options.untabify = Some(tabsize);
        }
        if self.line_endings {
            options.line_endings = true;
        }
        if self.no_line_endings {
            options.line_endings = false;
        }
        if self.lines_added_around {
            options.lines_added_around = true;
        }
        if self.reduce_deleted {
            options.reduce_deleted = true;
        }
        if self.split_markers {
            options.split_markers = true;
        }
        if self.no_split_markers {
            options.split_markers = false;
        }
        if self.indentation {
            options.indentation = true;
        }
        if self.no_indentation {
            options.indentation = false;
        }
        options
    }
}

#[derive(Parser, Default)]
struct EnvArgs {
    #[command(flatten)]
    resolution: ResolutionCliArgs,
}

#[derive(Parser)]
#[command(
    name = "git-mediate",
    version,
    about = "Automatically resolve trivial git merge conflicts"
)]
struct Cli {
    #[arg(
        short = 'e',
        long = "editor",
        help = "Open $EDITOR on files with remaining conflicts"
    )]
    editor: bool,

    #[arg(
        short = 'd',
        long = "diff",
        help = "Show each side's diff against the base for remaining conflicts"
    )]
    show_diff: bool,

    #[arg(
        short = '2',
        long = "diff2",
        help = "Show direct diffs between the two sides for remaining conflicts"
    )]
    show_diff2: bool,

    #[arg(
        short = 's',
        long = "set-conflict-style",
        alias = "style",
        help = "Set global merge.conflictstyle to diff3 before processing"
    )]
    set_conflict_style: bool,

    #[arg(short = 'f', long = "merge-file", help = "Process only this file")]
    merge_file: Option<String>,

    #[arg(short = 'c', long = "color", help = "Force colored output")]
    color: bool,

    #[arg(short = 'C', long = "no-color", help = "Disable colored output")]
    no_color: bool,

    #[arg(
        short = 'n',
        long,
        help = "Print what would change without modifying files"
    )]
    dry_run: bool,

    #[arg(long, help = "Do not stage resolved files with git add")]
    no_add: bool,

    #[arg(short, long, help = "Print verbose progress information")]
    verbose: bool,

    #[command(flatten)]
    resolution: ResolutionCliArgs,
}

fn main() -> Result<()> {
    let env_args = parse_env_args();
    let cli = Cli::parse();

    let resolve_options = cli
        .resolution
        .apply(env_args.resolution.apply(ResolveOptions::default()));
    let diff_context = cli
        .resolution
        .context
        .or(env_args.resolution.context)
        .unwrap_or(3);

    let color = match (cli.color, cli.no_color) {
        (true, _) => ColorChoice::Always,
        (_, true) => ColorChoice::Never,
        _ => ColorChoice::Auto,
    };

    let mut builder = GitMediateBuilder::new()
        .set_conflict_style(cli.set_conflict_style)
        .show_diff(cli.show_diff)
        .show_diff2(cli.show_diff2)
        .editor(cli.editor)
        .color_choice(color)
        .diff_context(diff_context)
        .dry_run(cli.dry_run)
        .no_add(cli.no_add)
        .verbose(cli.verbose)
        .resolve_options(resolve_options);

    if let Some(merge_file) = cli.merge_file {
        builder = builder.merge_file(merge_file);
    }

    let report = builder.run()?;
    report.print();

    if report.exit_code() != 0 {
        std::process::exit(report.exit_code());
    }

    Ok(())
}

fn parse_env_args() -> EnvArgs {
    let Ok(raw) = std::env::var("GIT_MEDIATE_OPTIONS") else {
        return EnvArgs::default();
    };

    let mut args = vec!["git-mediate".to_string()];
    args.extend(raw.split_whitespace().map(ToOwned::to_owned));

    match EnvArgs::try_parse_from(args) {
        Ok(env_args) => env_args,
        Err(err) => {
            eprintln!("warning: failed to parse GIT_MEDIATE_OPTIONS: {err}");
            EnvArgs::default()
        }
    }
}
