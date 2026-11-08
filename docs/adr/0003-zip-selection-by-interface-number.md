# 3. Select release zips by interface number, flavor name as fallback

- Status: Accepted, amended 2026-11-08 (see [Amendment](#amendment-2026-11-08))
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

## Amendment (2026-11-08)

Implementing the decision (issue #18) against real releases changed four points.

**The override is a flavor per client, not a pinned asset per addon.** `settings.json` gets `"flavorOverrides": { "<product>": "<flavor>" }`, keyed by product code (`wowf`, `wow_anniversary`, ...), with the packager's flavor names. An overridden client is matched on flavor only, skipping interface numbers. Interface matching already handles Forever and Anniversary for packager releases, so the per-addon pin waits until there is UI to set it (tracked in the backlog).

**"Closest lower interface, same major version" became "closest interface, same flavor".** Classic Era (11509) and Forever (16001) share major version 1 but are different games, so the major version can't stand in for the flavor. When no file lists the exact interface, the file for the client's flavor with the nearest interface wins, with ties going to the higher number. A Forever client never gets a `classic` file this way, unless the user overrides it.

**`release.json` is authoritative for the files it lists; other zips are judged by name.** Older packager versions list only some of the uploaded zips. For example, KuiNameplates 2.27.3a lists only the `-bcc` zip, though the release also has `-classic` and plain zips. Unlisted zips fall back to flavor words in their names, but never to the single-asset rule: a release that has a `release.json` and no file for the client has nothing for it. Files marked `nolib` are only used when no full file is published. When several files list the exact interface, the one built for the fewest flavors wins.

**A release without a file for the client falls back to an older one.** The source checks up to ten releases on the addon's channel that have zips. Each may cost a `release.json` download.

The selection order is now:

1. **Per-client flavor override:** match `release.json` entries on that flavor only, then file names.
2. **Interface number:** a `release.json` entry listing the client's exact interface.
3. **Flavor in `release.json`:** the entry for the client's flavor with the closest interface.
4. **Flavor name:** a known flavor word in the file name. For retail, this includes a single zip without one.
5. **Single asset:** only without a `release.json`, a lone zip whose name doesn't name another flavor.
