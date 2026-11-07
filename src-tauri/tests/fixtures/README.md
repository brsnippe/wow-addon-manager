# Test fixtures

`wow/` mimics a World of Warcraft installation with three game clients. All addons are synthetic.

| Client | Product (`.flavor.info`) | Version (`.build.info`) |
|---|---|---|
| `_retail_` | `wow` | 12.1.5.70783 |
| `_classic_era_` | `wow_classic_era` | 1.15.9.70576 |
| `_forever_` | `wowf` | 1.60.1.70862 |

## Addons and the case each one covers

| Addon | Clients | Covers |
|---|---|---|
| `SimpleAddon` | retail, classic_era | Single folder, all source IDs (`X-Website`, `X-Curse-Project-ID`, `X-Wago-ID`, `X-WoWI-ID`) |
| `MultiToc` | retail, classic_era, forever | Per-flavor TOCs (`_Mainline`, `_Vanilla`, `_Forever`) plus a fallback `MultiToc.toc` |
| `BossMods-Core`, `BossMods-Raids`, `BossMods-Dungeons` | retail | Multi-folder addon linked by `RequiredDeps` / `Dependencies`, color codes in titles, multiple interface numbers |
| `BomCrlf` | retail | UTF-8 BOM, CRLF line endings, upper- and lowercase color codes |
| `NoToc` | retail | Folder without a TOC, which is not an addon |
| `EraOnly` | classic_era | WoWInterface-only addon |
| `ForeverAddon` | forever | GitLab `X-Website`, Forever interface number, several authors |

Files here are stored byte-exact (see `.gitattributes`). Don't let an editor convert line endings or strip the BOM.
