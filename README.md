# git-mediate (Rust)

A Rust port of [git-mediate](https://github.com/Peaker/git-mediate) by [Eyal Lotem](https://github.com/Peaker).

The original is a Haskell tool that automatically resolves trivial git merge conflicts when using the `diff3` conflict style. This port was motivated by wanting to compile git-mediate for a Windows ARM VM running on macOS, where getting a Haskell toolchain set up is painful — but `cargo build` just works.

## How it works

When you configure git to use diff3 conflict style:

```shell
git config --global merge.conflictstyle diff3
```

Conflicts include the common ancestor (base) version:

```
Unconflicted stuff

<<<<<<< HEAD
Version A changes
|||||||
Base version
======= Version B
Version B changes
>>>>>>>

More unconflicted stuff
```

Many of these conflicts are mechanically resolvable. For example, if only one side changed anything relative to the base, the answer is obvious. git-mediate detects and resolves these trivial conflicts automatically, then runs `git add` on fully resolved files.

### Resolution strategies

git-mediate applies these strategies in order:

1. **Line ending normalization** — CRLF/LF differences don't cause false conflicts
2. **Trivial resolution** — if one side matches the base, take the other; if both sides match, take either
3. **Indentation-aware resolution** — if one side re-indented code while the other changed content, merge both changes (indent + content resolved independently)
4. **Prefix/suffix reduction** — strip matching lines at conflict boundaries, then retry resolution on the smaller conflict

## Installation

### From source

```shell
git clone https://github.com/user/git-mediate-rs
cd git-mediate-rs
cargo install --path .
```

### Cross-compilation

One of the main advantages of the Rust port — cross-compile to any target Rust supports:

```shell
# Example: Windows ARM64
rustup target add aarch64-pc-windows-msvc
cargo build --release --target aarch64-pc-windows-msvc
```

## Usage

First, make sure your git is configured to use diff3 conflict style (or use `-s`):

```shell
git-mediate -s   # sets merge.conflictstyle = diff3
```

Then, from a git repository with merge conflicts:

```shell
git-mediate      # resolve conflicts and stage resolved files
```

### Options

```
Usage: git-mediate [OPTIONS]

Options:
  -e, --editor                   Open $EDITOR on files with remaining conflicts
  -d, --diff                     Show diff of each side against the base for remaining conflicts
  -2, --diff2                    Show diff between the two sides for remaining conflicts
  -s, --set-conflict-style       Set merge.conflictstyle to diff3
  -f, --merge-file <MERGE_FILE>  Process only this file instead of all unmerged files
  -c, --color                    Force colored output
  -C, --no-color                 Disable colored output
  -n, --dry-run                  Only print what would be done, don't modify files
      --no-add                   Don't stage resolved files with git add
  -v, --verbose                  Be verbose about what's happening
  -h, --help                     Print help
  -V, --version                  Print version
```

### Show conflict diffs

When a conflict is a wall of text, use `-d` to see each side's changes relative to the base, or `-2` to see a direct diff between the two sides:

```shell
git-mediate -d    # diff each side against base
git-mediate -2    # diff side A vs side B
```

### Open editor

Use `-e` to open `$EDITOR` on files with remaining conflicts, jumping to the first conflict line:

```shell
git-mediate -e
```

## Library usage

The core logic is available as a library crate (`git_mediate`):

```rust
use git_mediate::parse::parse_conflicts;
use git_mediate::resolve::resolve_chunks;
use git_mediate::parse::chunks_to_string;

let content = std::fs::read_to_string("conflicted_file.txt")?;
let chunks = parse_conflicts(&content)?;
let (resolved_chunks, stats) = resolve_chunks(chunks);
let output = chunks_to_string(&resolved_chunks);

println!("{} resolved, {} remaining", stats.resolved, stats.failed);
```

## Credits

This is a port of [git-mediate](https://github.com/Peaker/git-mediate) by **Eyal Lotem**, originally written in Haskell. All credit for the design, algorithm, and concept goes to the original author.

## License

Copyright (C) 2014-2024 Eyal Lotem (original Haskell implementation)

The original git-mediate is licensed under the GNU General Public License v2.0. See the [original repository](https://github.com/Peaker/git-mediate) for details.
