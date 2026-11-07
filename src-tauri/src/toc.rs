//! Reads addon TOC files the way the game client does.
//!
//! An addon folder can hold several TOC files with game-type suffixes (`Addon_Mainline.toc`,
//! `Addon_Vanilla.toc`, ...). The client loads the most specific one for its game type and falls
//! back to `Addon.toc`. Directive values may carry inline conditions such as
//! `[AllowLoadGameType classic]` and UI escape sequences such as `|cffff0000red|r`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::clients::GameType;

/// Game-type and family names the client recognises in `AllowLoadGameType`.
const KNOWN_GAME_TYPE_NAMES: &[&str] = &[
    "standard",
    "forever",
    "camelot",
    "vanilla",
    "tbc",
    "wrath",
    "cata",
    "mists",
    "plunderstorm",
    "wowlabs",
    "wowhack",
    "mainline",
    "classic",
];

/// The directives from a TOC file that the addon manager uses. Text values have their escape
/// sequences removed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Toc {
    pub title: Option<String>,
    pub notes: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub interfaces: Vec<u32>,
    /// Required dependencies (`Dependencies`, `RequiredDeps` and other `Dep*` aliases).
    pub dependencies: Vec<String>,
    /// `X-Website`.
    pub website: Option<String>,
    /// `X-Curse-Project-ID`.
    pub curse_project_id: Option<String>,
    /// `X-Wago-ID`.
    pub wago_id: Option<String>,
    /// `X-WoWI-ID`.
    pub wowi_id: Option<String>,
}

/// Finds the TOC file the client would load for the addon in `addon_dir`.
///
/// Game-type suffixes win over the family suffix, `_` wins over `-`, and the unsuffixed file is
/// the fallback. Names are compared case-insensitively. With an unknown game type only the
/// unsuffixed file is considered.
pub fn find_toc(addon_dir: &Path, game: Option<GameType>) -> Option<PathBuf> {
    let name = addon_dir.file_name()?.to_str()?;
    let tocs: Vec<(String, PathBuf)> = fs::read_dir(addon_dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("toc"))
                && path.is_file()
        })
        .filter_map(|path| Some((path.file_stem()?.to_str()?.to_lowercase(), path)))
        .collect();

    let mut candidates = Vec::new();
    if let Some(game) = game {
        let family = [game.family()];
        for tier in [game.names(), &family] {
            for separator in ['_', '-'] {
                candidates.extend(
                    tier.iter()
                        .map(|suffix| format!("{name}{separator}{suffix}")),
                );
            }
        }
    }
    candidates.push(name.to_owned());

    candidates.iter().find_map(|candidate| {
        let candidate = candidate.to_lowercase();
        tocs.iter()
            .find(|(stem, _)| *stem == candidate)
            .map(|(_, path)| path.clone())
    })
}

/// Reads and parses a TOC file. Invalid UTF-8 is replaced rather than rejected.
pub fn read_toc(path: &Path, game: Option<GameType>) -> io::Result<Toc> {
    let bytes = fs::read(path)?;
    Ok(parse_toc(&String::from_utf8_lossy(&bytes), game))
}

/// Parses the `## Directive: Value` lines of a TOC file for the given game type.
///
/// Later directives replace earlier ones, except dependencies, which accumulate. Lines whose
/// inline conditions exclude the game type are skipped; with an unknown game type every
/// conditional line is skipped.
pub fn parse_toc(text: &str, game: Option<GameType>) -> Toc {
    let mut toc = Toc::default();
    for line in text.strip_prefix('\u{feff}').unwrap_or(text).lines() {
        let Some((key, value)) = line
            .strip_prefix("##")
            .and_then(|rest| rest.split_once(':'))
        else {
            continue;
        };
        let Some(value) = apply_inline_directives(value, game) else {
            continue;
        };
        let value = value.trim();
        let text = || Some(strip_escapes(value)).filter(|v| !v.is_empty());
        let plain = || Some(value.to_owned()).filter(|v| !v.is_empty());

        match key.trim().to_ascii_lowercase().as_str() {
            "title" => toc.title = text(),
            "notes" => toc.notes = text(),
            "version" => toc.version = text(),
            "author" => toc.author = text(),
            "interface" => {
                toc.interfaces = value
                    .split(',')
                    .filter_map(|v| v.trim().parse().ok())
                    .collect();
            }
            "x-website" => toc.website = plain(),
            "x-curse-project-id" => toc.curse_project_id = plain(),
            "x-wago-id" => toc.wago_id = plain(),
            "x-wowi-id" => toc.wowi_id = plain(),
            key if key == "requireddeps" || key.starts_with("dep") => {
                for dep in value.split(',').map(str::trim).filter(|d| !d.is_empty()) {
                    if !toc.dependencies.iter().any(|d| d == dep) {
                        toc.dependencies.push(dep.to_owned());
                    }
                }
            }
            _ => {}
        }
    }
    toc
}

/// Removes UI escape sequences: colors (`|cAARRGGBB`, `|cnNAME:`, `|r`), textures (`|T...|t`),
/// atlases (`|A...|a`) and hyperlink wrappers (`|H...|h`). `||` becomes `|`, `|n` a space, and
/// runs of whitespace collapse to one space.
pub fn strip_escapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(bar) = rest.find('|') {
        out.push_str(&rest[..bar]);
        let after = &rest[bar + 1..];
        let skip_to_closing = |close: &str| after.find(close).map_or("", |i| &after[i + 2..]);
        rest = match after.chars().next() {
            Some('c' | 'C') => {
                let tail = &after[1..];
                if tail.len() >= 8 && tail.as_bytes()[..8].iter().all(u8::is_ascii_hexdigit) {
                    &tail[8..]
                } else if let Some(end) = tail
                    .strip_prefix(['n', 'N'])
                    .and_then(|named| named.find(':').map(|i| &named[i + 1..]))
                {
                    end
                } else {
                    out.push('|');
                    after
                }
            }
            Some('T') => skip_to_closing("|t"),
            Some('A') => skip_to_closing("|a"),
            Some('H') => skip_to_closing("|h"),
            Some('r' | 'R' | 't' | 'a' | 'h') => &after[1..],
            Some('n') => {
                out.push(' ');
                &after[1..]
            }
            Some('|') => {
                out.push('|');
                &after[1..]
            }
            _ => {
                out.push('|');
                after
            }
        };
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Strips inline directives (`[Name args]`) from a directive value. Returns `None` when they
/// stop the line from applying to `game`.
fn apply_inline_directives(value: &str, game: Option<GameType>) -> Option<String> {
    let mut out = String::with_capacity(value.len());
    let mut applies = true;
    let mut rest = value;
    while let Some(open) = rest.find('[') {
        let Some(len) = rest[open..].find(']') else {
            break;
        };
        let inner = &rest[open + 1..open + len];
        let (name, args) = inner.trim().split_once(' ').unwrap_or((inner.trim(), ""));
        let args: Vec<String> = args
            .split(',')
            .map(|a| a.trim().to_ascii_lowercase())
            .filter(|a| !a.is_empty())
            .collect();
        let handled = match name.to_ascii_lowercase().as_str() {
            "allowloadgametype" => {
                applies &= game.is_some_and(|g| allows(g, &args));
                true
            }
            "excludeloadgametype" => {
                applies &= game.is_some_and(|g| !matches_game(g, &args));
                true
            }
            // Localised variants and bootstrap-only values never describe the addon as listed.
            "allowloadtextlocale" | "bootstrap" => {
                applies = false;
                true
            }
            "allowload" | "allowloadenvironment" | "loadintoenvironment" => true,
            _ => false,
        };
        let keep_until = if handled { open } else { open + len + 1 };
        out.push_str(&rest[..keep_until]);
        rest = &rest[open + len + 1..];
    }
    out.push_str(rest);
    applies.then_some(out)
}

/// `AllowLoadGameType` is satisfied when it names the game type or its family, or when the
/// client recognises none of the names it lists.
fn allows(game: GameType, names: &[String]) -> bool {
    matches_game(game, names)
        || !names
            .iter()
            .any(|n| KNOWN_GAME_TYPE_NAMES.contains(&n.as_str()))
}

fn matches_game(game: GameType, names: &[String]) -> bool {
    names.iter().any(|n| {
        game.names()
            .iter()
            .copied()
            .chain([game.family()])
            .any(|g| g.eq_ignore_ascii_case(n))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(client: &str, addon: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/wow")
            .join(client)
            .join("Interface/AddOns")
            .join(addon)
    }

    fn read_fixture(client: &str, addon: &str, game: GameType) -> Toc {
        let dir = fixture(client, addon);
        read_toc(&find_toc(&dir, Some(game)).unwrap(), Some(game)).unwrap()
    }

    fn file_name(path: Option<PathBuf>) -> Option<String> {
        path.map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
    }

    fn addon_dir_with(files: &[&str]) -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Addon");
        fs::create_dir(&dir).unwrap();
        for file in files {
            fs::write(dir.join(file), "## Title: x\n").unwrap();
        }
        (root, dir)
    }

    #[test]
    fn reads_every_field_and_source_id() {
        let toc = read_fixture("_retail_", "SimpleAddon", GameType::Standard);
        assert_eq!(
            toc,
            Toc {
                title: Some("Simple Addon".into()),
                notes: Some("A single-folder addon with all source IDs.".into()),
                version: Some("1.2.3".into()),
                author: Some("Fixture Author".into()),
                interfaces: vec![120105],
                dependencies: vec![],
                website: Some("https://github.com/example/SimpleAddon".into()),
                curse_project_id: Some("123456".into()),
                wago_id: Some("aBcDeFgH".into()),
                wowi_id: Some("98765".into()),
            }
        );
    }

    #[test]
    fn strips_colors_and_reads_dependency_aliases() {
        let core = read_fixture("_retail_", "BossMods-Core", GameType::Standard);
        assert_eq!(core.title.as_deref(), Some("Boss Mods Core"));
        assert_eq!(core.interfaces, [120105, 120100]);
        assert!(core.dependencies.is_empty());

        let raids = read_fixture("_retail_", "BossMods-Raids", GameType::Standard);
        assert_eq!(raids.dependencies, ["BossMods-Core"]);
        let dungeons = read_fixture("_retail_", "BossMods-Dungeons", GameType::Standard);
        assert_eq!(dungeons.dependencies, ["BossMods-Core"]);
    }

    #[test]
    fn handles_bom_crlf_and_uppercase_color_codes() {
        let toc = read_fixture("_retail_", "BomCrlf", GameType::Standard);
        assert_eq!(toc.title.as_deref(), Some("BOM and CRLF Addon"));
        assert_eq!(toc.interfaces, [120105]);
        assert_eq!(toc.version.as_deref(), Some("2.0"));
    }

    #[test]
    fn keeps_author_lists_and_gitlab_websites() {
        let toc = read_fixture("_forever_", "ForeverAddon", GameType::Forever);
        assert_eq!(toc.author.as_deref(), Some("Fixture Author, Second Author"));
        assert_eq!(
            toc.website.as_deref(),
            Some("https://gitlab.com/example/forever-addon")
        );
        assert_eq!(toc.interfaces, [16001]);
    }

    #[test]
    fn picks_the_fixture_toc_for_each_client() {
        let pick = |client, game| file_name(find_toc(&fixture(client, "MultiToc"), game));
        assert_eq!(
            pick("_retail_", Some(GameType::Standard)).as_deref(),
            Some("MultiToc_Mainline.toc")
        );
        assert_eq!(
            pick("_classic_era_", Some(GameType::Vanilla)).as_deref(),
            Some("MultiToc_Vanilla.toc")
        );
        assert_eq!(
            pick("_forever_", Some(GameType::Forever)).as_deref(),
            Some("MultiToc_Forever.toc")
        );
        assert_eq!(
            pick("_retail_", Some(GameType::Tbc)).as_deref(),
            Some("MultiToc.toc")
        );
        assert_eq!(pick("_retail_", None).as_deref(), Some("MultiToc.toc"));
    }

    #[test]
    fn game_suffix_beats_family_suffix() {
        let (_root, dir) =
            addon_dir_with(&["Addon_Mainline.toc", "Addon_Standard.toc", "Addon.toc"]);
        let pick = |game| file_name(find_toc(&dir, Some(game)));
        assert_eq!(
            pick(GameType::Standard).as_deref(),
            Some("Addon_Standard.toc")
        );
        assert_eq!(
            pick(GameType::Forever).as_deref(),
            Some("Addon_Mainline.toc")
        );
        assert_eq!(pick(GameType::Mists).as_deref(), Some("Addon.toc"));
    }

    #[test]
    fn classic_family_suffix_covers_all_classic_game_types() {
        let (_root, dir) = addon_dir_with(&["Addon_Classic.toc", "Addon_Vanilla.toc"]);
        let pick = |game| file_name(find_toc(&dir, Some(game)));
        assert_eq!(
            pick(GameType::Vanilla).as_deref(),
            Some("Addon_Vanilla.toc")
        );
        assert_eq!(pick(GameType::Tbc).as_deref(), Some("Addon_Classic.toc"));
        assert_eq!(pick(GameType::Mists).as_deref(), Some("Addon_Classic.toc"));
        assert_eq!(pick(GameType::Standard), None);
    }

    #[test]
    fn camelot_beats_its_forever_alias() {
        let (_root, dir) = addon_dir_with(&["Addon_Forever.toc", "Addon_Camelot.toc"]);
        assert_eq!(
            file_name(find_toc(&dir, Some(GameType::Forever))).as_deref(),
            Some("Addon_Camelot.toc")
        );
    }

    #[test]
    fn underscore_beats_hyphen_and_hyphen_still_works() {
        let (_root, dir) =
            addon_dir_with(&["Addon-TBC.toc", "Addon_TBC.toc", "Addon-Mainline.toc"]);
        let pick = |game| file_name(find_toc(&dir, Some(game)));
        assert_eq!(pick(GameType::Tbc).as_deref(), Some("Addon_TBC.toc"));
        assert_eq!(
            pick(GameType::Standard).as_deref(),
            Some("Addon-Mainline.toc")
        );
    }

    #[test]
    fn file_names_are_matched_case_insensitively() {
        let (_root, dir) = addon_dir_with(&["addon_MAINLINE.TOC"]);
        assert_eq!(
            file_name(find_toc(&dir, Some(GameType::Standard))).as_deref(),
            Some("addon_MAINLINE.TOC")
        );
    }

    #[test]
    fn retired_and_foreign_tocs_are_ignored() {
        let (_root, dir) = addon_dir_with(&["Addon_BCC.toc", "Addon_WOTLKC.toc", "Other.toc"]);
        assert_eq!(find_toc(&dir, Some(GameType::Tbc)), None);
        assert_eq!(find_toc(&dir, Some(GameType::Wrath)), None);
        let (_root, empty) = addon_dir_with(&[]);
        assert_eq!(find_toc(&empty, Some(GameType::Standard)), None);
    }

    #[test]
    fn later_directives_win_and_dependencies_accumulate() {
        let toc = parse_toc(
            "## title: First\n## Title : Second\n# ## Title: comment\n  ## Title: file name\n\
             ## Dependencies: A, B\n## Dep: B,C\n## RequiredDeps: D\n## Interface: 120105, ,x, 11509\n\
             ## Version:\n",
            Some(GameType::Standard),
        );
        assert_eq!(toc.title.as_deref(), Some("Second"));
        assert_eq!(toc.dependencies, ["A", "B", "C", "D"]);
        assert_eq!(toc.interfaces, [120105, 11509]);
        assert_eq!(toc.version, None);
    }

    #[test]
    fn inline_game_type_conditions_select_the_value() {
        let text = "## Title: Retail name\n## Title: Classic name [AllowLoadGameType classic]\n\
                    ## Notes: Shown [ExcludeLoadGameType vanilla, tbc]\n\
                    ## Version: 1.0 [AllowLoadGameType futuregame]\n\
                    ## Author: [Bracketed] name [AllowLoad Game]\n";
        let retail = parse_toc(text, Some(GameType::Standard));
        assert_eq!(retail.title.as_deref(), Some("Retail name"));
        assert_eq!(retail.notes.as_deref(), Some("Shown"));
        assert_eq!(
            retail.version.as_deref(),
            Some("1.0"),
            "unknown game types are satisfied"
        );
        assert_eq!(retail.author.as_deref(), Some("[Bracketed] name"));

        let vanilla = parse_toc(text, Some(GameType::Vanilla));
        assert_eq!(vanilla.title.as_deref(), Some("Classic name"));
        assert_eq!(vanilla.notes, None);

        let unknown = parse_toc(text, None);
        assert_eq!(unknown.title.as_deref(), Some("Retail name"));
        assert_eq!(unknown.version, None);
    }

    #[test]
    fn localised_and_locale_restricted_values_are_ignored() {
        let toc = parse_toc(
            "## Title: English\n## Title-deDE: Deutsch\n## Title: Francais [AllowLoadTextLocale frFR]\n",
            Some(GameType::Standard),
        );
        assert_eq!(toc.title.as_deref(), Some("English"));
    }

    #[test]
    fn strips_ui_escape_sequences() {
        assert_eq!(
            strip_escapes("|cff33ff99BOM|r and |CFFFF0000CRLF|R"),
            "BOM and CRLF"
        );
        assert_eq!(
            strip_escapes("HighLevelAlert by |cff55d2ffD4KiR |T132115:16:16:0:0|t"),
            "HighLevelAlert by D4KiR"
        );
        assert_eq!(
            strip_escapes("|TInterface\\Emotes\\EZ:0|tTwitch Emotes|TInterface\\Emotes\\EZ:0|t"),
            "Twitch Emotes"
        );
        assert_eq!(strip_escapes("|A:atlas:16:16|a Icon"), "Icon");
        assert_eq!(strip_escapes("|cnGREEN_FONT_COLOR:Green|r"), "Green");
        assert_eq!(strip_escapes("|Hitem:123|h[Sword]|h"), "[Sword]");
        assert_eq!(strip_escapes("A || B|nC"), "A | B C");
        assert_eq!(
            strip_escapes("Pipe | here |cff12 short"),
            "Pipe | here |cff12 short"
        );
        assert_eq!(strip_escapes("Broken |Tno end"), "Broken");
    }
}
