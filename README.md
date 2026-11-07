# wow-addon-manager

[![CI](https://github.com/brsnippe/wow-addon-manager/actions/workflows/ci.yml/badge.svg)](https://github.com/brsnippe/wow-addon-manager/actions/workflows/ci.yml)

A git-first World of Warcraft addon manager for every game version installed on your PC: Retail, Classic Era, Anniversary, Classic Beta and Forever.

Most addons are developed in git repositories, but no single addon manager covers every source. This app aims to manage them all in one place: add, update and remove addons for each game version.

## How it works

- **Detects game clients** from each `_*_` folder in your WoW install (`.flavor.info` / `.build.info`).
- **Release zips first:** installs the packaged release of a repo (GitHub/GitLab releases, picking the right zip per game version via `release.json`). Raw repositories often miss bundled libraries, so a plain `git clone` is only used as a fallback.
- **Only touches what it owns:** every installed folder is recorded, updates are backed up and rolled back on failure, and uninstalls never delete folders the app didn't install.
- **Sources:** GitHub, GitLab, git, WoWInterface, and later CurseForge and Wago Addons (once API keys are available).

## Stack

[Tauri v2](https://v2.tauri.app/) with a Rust backend and a Svelte 5 + TypeScript frontend.

## Development

### Prerequisites (Windows)

1. **Rust** (stable, MSVC toolchain) via rustup:
   ```powershell
   winget install --id Rustlang.Rustup --exact
   ```
   To keep the toolchain off a small system drive, set `RUSTUP_HOME` and `CARGO_HOME` (user environment variables) to a folder on another drive and add `%CARGO_HOME%\bin` to `PATH`.
2. **Microsoft C++ Build Tools** with the MSVC compiler and the Windows 11 SDK:
   ```powershell
   winget install --id Microsoft.VisualStudio.2022.BuildTools --exact --override "--wait --passive --add Microsoft.VisualStudio.Component.VC.Tools.x86.x64 --add Microsoft.VisualStudio.Component.Windows11SDK.26100"
   ```
   Add `--installPath <dir> --path cache=<dir> --path shared=<dir>` to install on another drive. The Windows SDK always installs on the system drive (~1.5 GB).
3. **WebView2 runtime**: preinstalled on Windows 10 (1803+) and 11.
4. **Node.js** 20 or newer.
5. **git** (used by the git clone source).

Check the setup:

```powershell
rustc --version
cargo --version
node --version
```

### WoW folder used by the app

The app looks for the World of Warcraft installation in this order:

1. The `WAM_WOW_ROOT` environment variable
2. `wowRoot` in `settings.json` in the app config folder (`%APPDATA%\com.brsnippe.wowaddonmanager`)
3. **Debug builds only:** `dev-sandbox/` in this repository

Development builds therefore never touch your real installation unless you point them at it. To fill the sandbox with a copy of your addons:

```powershell
./scripts/make-sandbox.ps1                                  # all clients, full copy
./scripts/make-sandbox.ps1 -Clients _retail_,_forever_      # selected clients
./scripts/make-sandbox.ps1 -TocOnly                         # only .toc files (fast, scanning only)
./scripts/make-sandbox.ps1 -Source "D:\Games\World of Warcraft"
```

Rust tests use the small synthetic installation in `src-tauri/tests/fixtures/wow` (see its README).

### Commands

| Command | What it does |
|---|---|
| `npm install` | Install frontend dependencies |
| `npm run tauri dev` | Run the app with hot reload |
| `npm run tauri build` | Build a release bundle |
| `npm run check` | Type-check the Svelte/TypeScript frontend |
| `npm run fmt:rust` | Format the Rust code |
| `npm run lint:rust` | Check Rust formatting and run clippy (warnings are errors) |
| `npm run test:rust` | Run the Rust tests |
| `npm run verify` | Run all checks, as CI does |

## Roadmap

Work is tracked in [issues](https://github.com/brsnippe/wow-addon-manager/issues), grouped per phase (epics with sub-issues) and [milestones](https://github.com/brsnippe/wow-addon-manager/milestones):

| Milestone | Phases |
|---|---|
| M1 Read-only MVP | [Phase 0: Project setup](https://github.com/brsnippe/wow-addon-manager/issues/1), [Phase 1: Read-only scanning](https://github.com/brsnippe/wow-addon-manager/issues/7) |
| M2 GitHub updates | [Phase 2: Tracking and GitHub source](https://github.com/brsnippe/wow-addon-manager/issues/14), [Phase 3: Install, update and remove](https://github.com/brsnippe/wow-addon-manager/issues/22) |
| M3 Git sources & import | [Phase 4: More git sources](https://github.com/brsnippe/wow-addon-manager/issues/29), [Phase 5: Import existing addons](https://github.com/brsnippe/wow-addon-manager/issues/33) |
| M4 WoWInterface | [Phase 6: WoWInterface](https://github.com/brsnippe/wow-addon-manager/issues/38) |
| M5 Polish & release | [Phase 8: Polish and release](https://github.com/brsnippe/wow-addon-manager/issues/48) |
| Backlog | [Phase 7: CurseForge and Wago](https://github.com/brsnippe/wow-addon-manager/issues/42) (blocked: API keys) |

## License

[MIT](LICENSE)
