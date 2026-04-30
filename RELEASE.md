# Releasing bee-rs

This document describes how to cut a new release of `bee-rs`. The flow
mirrors what `bee-go` uses: a single commit on `main` updates the
version, the changelog, and is tagged `vX.Y.Z`. The tag triggers
publication to crates.io (when configured) and creates a GitHub
release.

## Versioning

`bee-rs` follows [Semantic Versioning](https://semver.org/). Until
P4 (live Bee soak) is green and the Bee version pin is bumped, the
crate stays in `0.x` — breaking changes are allowed in minor bumps.

## Prerequisites

- A clean working tree on `main` synced with `origin/main`.
- `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D
  warnings`, and `cargo test --all-targets --locked` all pass locally.
- `cargo doc --no-deps` runs without warnings (`RUSTDOCFLAGS=-Dwarnings`).
- Any new public surface introduced since the previous tag is
  documented and listed in `CHANGELOG.md` under `[Unreleased]`.

## Release steps

1. **Pick the version.** For a bug-fix-only release bump the patch;
   for new public surface bump the minor; for an MSRV bump or a
   removed item bump the major (or the minor while in `0.x`).
2. **Update `Cargo.toml`.** Set `package.version = "X.Y.Z"`. Update
   `package.rust-version` only when the MSRV actually changes.
3. **Roll the changelog.** In `CHANGELOG.md`, replace the
   `[Unreleased]` heading with `[X.Y.Z] - YYYY-MM-DD` and add a fresh
   empty `[Unreleased]` section above it. Keep the existing
   `### Added` / `### Changed` / `### Fixed` / `### Removed`
   subheadings.
4. **Run the full lint + test suite once more** so the release
   commit reflects the artifact you actually shipped:
   ```bash
   cargo fmt --all
   cargo clippy --all-targets -- -D warnings
   cargo test --all-targets --locked
   cargo doc --no-deps
   ```
5. **Commit.** Single commit, message `release: vX.Y.Z`. Push to
   `main`; CI must go green before tagging.
6. **Tag and push.** Annotate the tag with the changelog excerpt:
   ```bash
   git tag -a vX.Y.Z -m "vX.Y.Z" -m "$(cat <<'EOF'
   <paste this version's CHANGELOG entries here>
   EOF
   )"
   git push origin vX.Y.Z
   ```
7. **(Once configured) Publish to crates.io:**
   ```bash
   cargo publish
   ```
8. **Cut a GitHub release.** Create a release on the pushed tag with
   the same body as the tag annotation (`gh release create vX.Y.Z`).

## Hotfixes

Hotfixes branch from the previous release tag, land the minimal
change with a test, bump the patch version, and follow the same flow.
Once published, fast-forward `main` to include the hotfix.

## Rollback

If a release is broken on crates.io, `cargo yank --version X.Y.Z`
hides it from new dependents without breaking existing lockfiles.
The fix ships as a fresh patch release; do not retag.
