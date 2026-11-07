# 4. The app only touches folders it owns

- Status: Accepted
- Date: 2026-11-07

## Context

`Interface\AddOns` is shared. It contains addons installed by hand, by other managers (CurseForge, Wago, WowUp) and by this app. Users will migrate gradually, so for a while several tools manage the same folder. Deleting or overwriting the wrong folder loses an addon, its local edits, or another tool's install.

One addon can also span several folders (for example `BossMods-Core`, `BossMods-Raids`, `BossMods-Dungeons`), and the folder list can change between versions.

## Decision

The app records which folders it installed, per client and per addon, in its own tracking file in the app config folder. It **only modifies or deletes folders listed there**.

- Folders the app didn't install are shown as *unmanaged*. They are read for display but never changed.
- Unmanaged folders become managed only through an explicit user action (adopting or reinstalling the addon).
- If a zip or clone contains a folder that already exists but isn't owned, the install stops and asks the user.
- Updates remove owned folders that the new version no longer ships, and nothing else.
- Installs are prepared in a temporary folder and moved into place only when complete. Folders being replaced are backed up first and restored if the update fails.
- The app never writes to `WTF` (saved variables and settings) or anything else outside `Interface\AddOns`.

## Consequences

- The app can run next to other managers and manual installs without damaging them.
- A tracking file is required before the first install, and losing it turns managed addons into unmanaged ones (safe, but they need adopting again).
- Adopting existing addons needs a separate flow.
- Every destructive operation goes through one ownership check, which is a single place to test.
