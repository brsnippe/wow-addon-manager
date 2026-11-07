# 1. Tauri v2, Rust and Svelte as the stack

- Status: Accepted
- Date: 2026-11-07

## Context

The app is a personal Windows desktop tool. Its core work is file system access, HTTP requests to several hosting APIs, extracting zip archives and running `git`. The interface is mostly one large, sortable table with a few dialogs, modelled on the Wago app.

Options considered:

- **Electron + TypeScript:** familiar web UI, but ships a full Chromium and Node runtime (~150 MB) and needs a second process model for file system work.
- **Native Rust GUI (egui, iced, Slint):** a single language, but data tables, theming and text layout take much more work than in HTML/CSS.
- **.NET (WPF, WinUI):** good Windows integration, but a heavier toolchain and no reuse of web UI skills.
- **Tauri v2 + Rust + a web frontend:** small binary that uses the WebView2 runtime already present on Windows 10 and 11, with a Rust backend for the I/O-heavy work. `vikinghug/forever-addons` is prior art for a WoW addon manager built this way.

## Decision

Use **Tauri v2** with a **Rust** backend and a **Svelte 5 + TypeScript** frontend (SvelteKit with `adapter-static` and `ssr = false`), managed with npm.

All file system, network, archive and `git` work happens in Rust and is exposed to the frontend as Tauri commands. The frontend only renders state and calls commands.

## Consequences

- Development needs the Rust MSVC toolchain, the Microsoft C++ Build Tools and the Windows SDK (see the README). Users only need WebView2.
- Two languages in one repo: CI runs both `svelte-check` and `cargo fmt`/`clippy`/`test`.
- Business logic lives in Rust, where it is unit-testable without a browser. The frontend stays thin.
- Types shared across the command boundary are defined twice (Rust structs with `serde(rename_all = "camelCase")` and TypeScript interfaces) and must be kept in sync by hand.
