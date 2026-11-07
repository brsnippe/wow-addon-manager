# 3. Select release zips by interface number, flavor name as fallback

- Status: Accepted
- Date: 2026-11-07

## Context

One release often has several zips, one per game flavor (for example `MyAddon-1.2.0.zip`, `MyAddon-1.2.0-classic.zip`, `MyAddon-1.2.0-bcc.zip`). The app manages several clients side by side: retail, Classic Era, Anniversary, Classic beta and Forever.

Flavor names in file names are inconsistent: `classic`, `vanilla` and `era` mean the same; `bcc` and `tbc` do too; `wrath` and `wotlkc` likewise. Newer clients such as Anniversary and Forever have no established suffix at all.

Interface numbers are unambiguous. Each client's interface number follows from its version in `.build.info`: `major * 10000 + minor * 100 + patch`, so 12.1.5 gives 120105, 1.15.9 gives 11509 and 1.60.1 gives 16001. The BigWigs packager publishes a `release.json` next to the zips that lists every file with its flavor and interface numbers. `omaforge` uses this file to pick zips.

## Decision

For each client, choose the release asset in this order:

1. **Per-client override:** if the user has pinned an asset for this addon and client, use it.
2. **Interface number:** if the release has a `release.json`, pick the file whose interface list contains the client's interface number. If none matches exactly, pick the closest lower interface number with the same major version.
3. **Flavor name:** otherwise match the asset file name against a table of known flavor suffixes for the client's product.
4. **Single asset:** if the release has exactly one zip, use it.

If no rule picks an asset, the addon is marked as having no compatible release for that client instead of guessing.

## Consequences

- Most releases resolve without user input, including new patches, because the interface number comes from the installed client.
- The flavor suffix table needs maintenance when Blizzard adds clients, but it's only a fallback.
- Users can resolve unusual naming schemes with an override instead of waiting for a code change.
- Selection is a pure function of (release metadata, client, overrides), which keeps it easy to unit-test.
