# Agent instructions

## Versions

Landing bumps the version, not each work commit (tpm CLAUDE.md "Landing bumps the version"): branch commits put
their CHANGELOG entries under `[Unreleased]`, and `tpm land` turns them into `## [x.y.z] - YYYY-MM-DD` with the
manifest bump (all workspace crates share one version) in one `release: x.y.z` commit, so any build from main
traces back. **A branch never touches the version** (manifests, `## [x.y.z]` headings, tags) — not a session's
call. The bump follows Cargo's semver: before 1.0, minor means breaking, so 0.x gets patch for additive or
compatible changes and minor only for a breaking one; from 1.0, minor for Added/Deprecated, patch for fixes,
major only when the owner says so. A breaking change is said in the entry itself (`**Breaking:** ...` under
Changed or Removed), so the landing can tell.

CLIs print version, git SHA and commit time in `--version`, e.g. `tool 0.4.2 (a1b2c3d-dirty, commit 3 Oct 2026
18:20 UTC)` — the commit's own time, labelled `commit`, not `built` (a build time forces rebuilds when nothing
changed and means nothing once builds are reproducible): a small `build.rs` without extra crates (`git rev-parse
--short HEAD`, `-dirty` when `git diff-index --quiet HEAD --` is non-zero, the commit time from `git log -1
--format=%ct`, worktree-safe `rerun-if-changed` on the real git dir's `HEAD`/`index`, `unknown` without git;
`cnt_cli/build.rs` is the model here). Firmware reports the same through `fw_info`. When touching a CLI that
lacks it or still says `built`, change it.
