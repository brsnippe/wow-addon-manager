# 2. Release zip first, git clone as fallback

- Status: Accepted
- Date: 2026-11-07

## Context

Most addons are developed in a git repository but distributed as a packaged zip, usually built by the BigWigs packager or a similar tool. Packaging changes the code:

- Libraries listed as externals in `.pkgmeta` (Ace3, LibStub, LibDataBroker and so on) are fetched into the zip. A raw repository often doesn't contain them.
- Tokens such as `@project-version@` in TOC and Lua files are replaced with the real version.
- Debug-only blocks (`--@debug@`) are removed and per-flavor TOC files are generated.
- Development files (tests, CI config, `.pkgmeta`) are left out.

A clone of the repository is therefore often not a working addon. Some addons, however, have no releases at all, for example small or private addons and some Forever addons that only exist as a repository.

## Decision

When an addon has packaged releases, install from the **release zip**. Only when no usable release exists, **fall back to a git clone** of the repository.

The installed version is the release tag (or the asset's version) for zip installs, and the commit for git installs.

## Consequences

- Each source (GitHub, GitLab, WoWInterface, and later CurseForge and Wago) needs a way to list releases and their assets.
- An addon's install method is recorded per addon, so updates use the same method that installed it.
- A git-cloned addon that relies on packager externals can be broken. The UI marks git installs so this is visible.
- Unpackaged development builds are not a goal. Users who want them can still clone manually.
