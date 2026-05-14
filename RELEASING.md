# Releasing

Releases are published with the `Publish` GitHub Actions workflow.

## Prerequisites

- `CRATES_IO_TOKEN` is configured as a repository secret.
- `Cargo.toml` contains the version that should be published.
- The release commit is on `main`.
- CI is passing for the release commit.

## Dry run

Run the `Publish` workflow from `main` with:

- `version`: the exact `Cargo.toml` package version, for example `0.1.0`
- `dry_run`: `true`

The dry run checks formatting, clippy, tests, package verification, and
`cargo publish --dry-run`. It does not upload the crate or create a tag.

## Publish

Run the `Publish` workflow from `main` with:

- `version`: the exact `Cargo.toml` package version
- `dry_run`: `false`

The workflow publishes the crate with `cargo publish --locked` and then creates
and pushes an annotated `vX.Y.Z` tag for the published version.
