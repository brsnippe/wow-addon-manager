# 5. Use the system `git` executable instead of a Rust git library

- Status: Accepted
- Date: 2026-11-07

## Context

The git source (see [ADR 2](0002-release-zip-first-git-clone-fallback.md)) needs to clone repositories, fetch updates, check out a tag or branch and read the current commit. Some repositories are private and need the user's credentials.

Options considered:

- **`git2` (libgit2 bindings):** mature, but adds a C dependency to the build, has its own credential handling and doesn't read the user's credential helper or SSH setup the way git does.
- **`gix` (gitoxide):** pure Rust, but its API is still changing and clone/checkout coverage for all cases (submodules, auth) is incomplete.
- **System `git`:** already installed by anyone who uses git-hosted addons, and behaves exactly like the user's terminal.

## Decision

Run the **system `git` executable** as a child process for all git operations.

- Arguments are passed as an array, never through a shell.
- `GIT_TERMINAL_PROMPT=0` is set so git never waits for input, and on Windows the process is started with `CREATE_NO_WINDOW` so no console window flashes.
- Output is read from machine-readable commands (`rev-parse`, `ls-remote`, `--porcelain`) rather than human-readable text.
- Clones are shallow (`--depth 1`) unless a specific older commit is needed.

## Consequences

- The user's credential manager, SSH keys and git configuration work without extra code.
- No extra native build dependency or binary size.
- `git` must be on `PATH`. The app detects it at startup; if it's missing, git sources are disabled with a clear message, and zip-based sources keep working.
- Behavior depends on the installed git version. The app requires a minimum version and checks it with `git --version`.
- Tests that exercise git need `git` available, which is true on the CI runners.
