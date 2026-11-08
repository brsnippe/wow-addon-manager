//! Picks the zip to install from a release with several, as described in ADR 0003.

use serde::{Deserialize, Serialize};

use crate::clients::{GameClient, GameType};
use crate::config::Settings;

/// Game flavors, named as the BigWigs packager names them in `release.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Flavor {
    Mainline,
    Classic,
    Bcc,
    Wrath,
    Cata,
    Mists,
    Forever,
}

impl Flavor {
    /// Reads a flavor from `release.json` or a word in a file name.
    pub fn parse(word: &str) -> Option<Self> {
        Some(match word.to_ascii_lowercase().as_str() {
            "mainline" | "retail" => Self::Mainline,
            "classic" | "vanilla" | "era" => Self::Classic,
            "bcc" | "tbc" | "tbcc" => Self::Bcc,
            "wrath" | "wotlk" | "wotlkc" => Self::Wrath,
            "cata" | "cataclysm" => Self::Cata,
            "mists" | "mop" => Self::Mists,
            "forever" => Self::Forever,
            _ => return None,
        })
    }
}

impl From<GameType> for Flavor {
    fn from(game_type: GameType) -> Self {
        match game_type {
            GameType::Standard => Self::Mainline,
            GameType::Vanilla => Self::Classic,
            GameType::Tbc => Self::Bcc,
            GameType::Wrath => Self::Wrath,
            GameType::Cata => Self::Cata,
            GameType::Mists => Self::Mists,
            GameType::Forever => Self::Forever,
        }
    }
}

/// What a source needs to know about a client to pick a file for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub interface: Option<u32>,
    pub flavor: Option<Flavor>,
    /// The user set this client's flavor: match on flavor only, not on interface number.
    pub overridden: bool,
}

impl Target {
    pub fn for_client(client: &GameClient, settings: &Settings) -> Self {
        match settings.flavor_overrides.get(&client.product) {
            Some(&flavor) => Self {
                interface: client.interface,
                flavor: Some(flavor),
                overridden: true,
            },
            None => Self {
                interface: client.interface,
                flavor: client.game_type.map(Flavor::from),
                overridden: false,
            },
        }
    }
}

/// The `release.json` the BigWigs packager publishes next to its zips.
#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseJson {
    pub releases: Vec<ReleaseJsonFile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseJsonFile {
    pub filename: String,
    #[serde(default)]
    pub nolib: bool,
    #[serde(default)]
    pub metadata: Vec<ReleaseJsonMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseJsonMetadata {
    /// Kept as text: the packager knows flavors this app doesn't.
    pub flavor: String,
    pub interface: u32,
}

/// Picks one of `zips` (asset file names) for `target`, or `None` rather than guessing.
///
/// Files listed in `release.json` are judged by its metadata: an exact interface match
/// first, then the file for the target's flavor. Other zips are judged by flavor words in
/// their names, and when there is no `release.json`, a lone zip is used unless its name
/// names another flavor.
pub fn select_file<'a>(
    zips: &[&'a str],
    release_json: Option<&ReleaseJson>,
    target: &Target,
) -> Option<&'a str> {
    let Some(json) = release_json else {
        return by_file_name(zips, target, true);
    };
    by_release_json(zips, json, target).or_else(|| {
        // Older packager versions list only some of the zips they upload.
        let unlisted: Vec<&'a str> = zips
            .iter()
            .filter(|zip| {
                !json
                    .releases
                    .iter()
                    .any(|f| f.filename.eq_ignore_ascii_case(zip))
            })
            .copied()
            .collect();
        by_file_name(&unlisted, target, false)
    })
}

fn by_release_json<'a>(zips: &[&'a str], json: &ReleaseJson, target: &Target) -> Option<&'a str> {
    let listed: Vec<(&'a str, &ReleaseJsonFile)> = json
        .releases
        .iter()
        .filter_map(|file| {
            let zip = zips
                .iter()
                .find(|z| z.eq_ignore_ascii_case(&file.filename))?;
            Some((*zip, file))
        })
        .collect();
    // Files without bundled libraries only count when nothing else is published.
    let full: Vec<_> = listed.iter().filter(|(_, f)| !f.nolib).copied().collect();
    let files = if full.is_empty() { listed } else { full };

    // Several files can list the same interface; the one built for fewest flavors wins.
    if let (Some(interface), false) = (target.interface, target.overridden) {
        let exact = files
            .iter()
            .filter(|(_, f)| f.metadata.iter().any(|m| m.interface == interface))
            .min_by_key(|(_, f)| f.metadata.len());
        if let Some((zip, _)) = exact {
            return Some(zip);
        }
    }

    let flavor = target.flavor?;
    files
        .iter()
        .filter_map(|(zip, file)| {
            let best = file
                .metadata
                .iter()
                .filter(|m| Flavor::parse(&m.flavor) == Some(flavor))
                .map(|m| interface_rank(m.interface, target.interface))
                .min()?;
            Some((best, file.metadata.len(), *zip))
        })
        .min()
        .map(|(_, _, zip)| zip)
}

/// Sort key for an interface: nearest to the client's first, ties and unknown clients
/// going to the higher number.
fn interface_rank(interface: u32, client: Option<u32>) -> (u32, std::cmp::Reverse<u32>) {
    let distance = client.map_or(0, |c| c.abs_diff(interface));
    (distance, std::cmp::Reverse(interface))
}

fn by_file_name<'a>(zips: &[&'a str], target: &Target, allow_lone_zip: bool) -> Option<&'a str> {
    let tagged: Vec<(&'a str, Option<Flavor>)> =
        zips.iter().map(|zip| (*zip, flavor_in_name(zip))).collect();

    if let Some(flavor) = target.flavor {
        let mut matching = tagged.iter().filter(|(_, f)| *f == Some(flavor));
        if let Some((zip, _)) = matching.next() {
            return Some(zip);
        }
        // Retail zips usually carry no flavor at all.
        if flavor == Flavor::Mainline {
            let mut plain = tagged.iter().filter(|(_, f)| f.is_none());
            if let (Some((zip, _)), None) = (plain.next(), plain.next()) {
                return Some(zip);
            }
        }
    }

    match tagged.as_slice() {
        _ if !allow_lone_zip => None,
        [(zip, None)] => Some(zip),
        [(zip, Some(flavor))] if target.flavor == Some(*flavor) => Some(zip),
        _ => None,
    }
}

/// The flavor named by a word in a file name, as in `Addon-1.2.0-classic.zip`.
fn flavor_in_name(file_name: &str) -> Option<Flavor> {
    let stem = file_name
        .len()
        .checked_sub(4)
        .filter(|&end| file_name[end..].eq_ignore_ascii_case(".zip"))
        .map_or(file_name, |end| &file_name[..end]);
    stem.split(['-', '_', '.', '+', ' '])
        .skip(1)
        .find_map(Flavor::parse)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;

    const RETAIL: u32 = 120105;
    const ERA: u32 = 11509;
    const ANNIVERSARY: u32 = 20506;
    const FOREVER: u32 = 16001;
    const CATA: u32 = 40402;

    fn target(interface: u32, flavor: Flavor) -> Target {
        Target {
            interface: Some(interface),
            flavor: Some(flavor),
            overridden: false,
        }
    }

    fn fixture(name: &str) -> ReleaseJson {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/github")
            .join(name);
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    fn aio_zips() -> [&'static str; 3] {
        [
            "AdvancedInterfaceOptions-2.1.14-cata.zip",
            "AdvancedInterfaceOptions-2.1.14-classic.zip",
            "AdvancedInterfaceOptions-2.1.14.zip",
        ]
    }

    #[test]
    fn single_zip_release_json_serves_every_listed_client() {
        let json = fixture("bigwigs-v427.8-release.json");
        let zips = ["BigWigs-v427.8.zip"];
        for target in [
            target(RETAIL, Flavor::Mainline),
            target(ERA, Flavor::Classic),
            target(ANNIVERSARY, Flavor::Bcc),
            target(FOREVER, Flavor::Forever),
        ] {
            assert_eq!(
                select_file(&zips, Some(&json), &target),
                Some("BigWigs-v427.8.zip"),
                "{target:?}"
            );
        }
    }

    #[test]
    fn multi_zip_release_prefers_the_most_specific_exact_match() {
        let json = fixture("aio-2.1.14-release.json");
        let zips = aio_zips();
        let pick = |interface, flavor| select_file(&zips, Some(&json), &target(interface, flavor));
        assert_eq!(
            pick(ERA, Flavor::Classic),
            Some("AdvancedInterfaceOptions-2.1.14-classic.zip")
        );
        assert_eq!(
            pick(CATA, Flavor::Cata),
            Some("AdvancedInterfaceOptions-2.1.14-cata.zip")
        );
        assert_eq!(
            pick(RETAIL, Flavor::Mainline),
            Some("AdvancedInterfaceOptions-2.1.14.zip")
        );
        assert_eq!(
            pick(FOREVER, Flavor::Forever),
            Some("AdvancedInterfaceOptions-2.1.14.zip")
        );
    }

    #[test]
    fn outdated_interfaces_fall_back_to_the_flavor() {
        let json = fixture("kuinameplates2-2.29.1-release.json");
        let zips = [
            "Kui_Nameplates-2.29.1-classic.zip",
            "Kui_Nameplates-2.29.1-wrath.zip",
            "Kui_Nameplates-2.29.1.zip",
        ];
        let pick = |interface, flavor| select_file(&zips, Some(&json), &target(interface, flavor));
        assert_eq!(
            pick(RETAIL, Flavor::Mainline),
            Some("Kui_Nameplates-2.29.1.zip")
        );
        assert_eq!(
            pick(ERA, Flavor::Classic),
            Some("Kui_Nameplates-2.29.1-classic.zip")
        );
        // Nothing for these flavors: no guessing.
        assert_eq!(pick(ANNIVERSARY, Flavor::Bcc), None);
        assert_eq!(pick(FOREVER, Flavor::Forever), None);
    }

    #[test]
    fn forever_does_not_take_the_era_file_despite_sharing_a_major_version() {
        let json: ReleaseJson = serde_json::from_str(
            r#"{"releases": [
                {"filename": "A-classic.zip", "metadata": [{"flavor": "classic", "interface": 11509}]},
                {"filename": "A.zip", "metadata": [{"flavor": "mainline", "interface": 120105}]}
            ]}"#,
        )
        .unwrap();
        let zips = ["A-classic.zip", "A.zip"];
        assert_eq!(
            select_file(&zips, Some(&json), &target(FOREVER, Flavor::Forever)),
            None
        );
    }

    #[test]
    fn within_a_flavor_the_closest_interface_wins() {
        let json: ReleaseJson = serde_json::from_str(
            r#"{"releases": [
                {"filename": "A-old.zip", "metadata": [{"flavor": "mainline", "interface": 110002}]},
                {"filename": "A-new.zip", "metadata": [{"flavor": "mainline", "interface": 120001}]}
            ]}"#,
        )
        .unwrap();
        let zips = ["A-old.zip", "A-new.zip"];
        assert_eq!(
            select_file(&zips, Some(&json), &target(RETAIL, Flavor::Mainline)),
            Some("A-new.zip")
        );
    }

    #[test]
    fn nolib_files_are_used_only_when_nothing_else_is_published() {
        let both: ReleaseJson = serde_json::from_str(
            r#"{"releases": [
                {"filename": "A-nolib.zip", "nolib": true, "metadata": [{"flavor": "mainline", "interface": 120105}]},
                {"filename": "A.zip", "nolib": false, "metadata": [{"flavor": "mainline", "interface": 120105}]}
            ]}"#,
        )
        .unwrap();
        let retail = target(RETAIL, Flavor::Mainline);
        assert_eq!(
            select_file(&["A-nolib.zip", "A.zip"], Some(&both), &retail),
            Some("A.zip")
        );
        assert_eq!(
            select_file(&["A-nolib.zip"], Some(&both), &retail),
            Some("A-nolib.zip")
        );
    }

    #[test]
    fn release_json_entries_without_an_asset_are_ignored() {
        let json = fixture("aio-2.1.14-release.json");
        let zips = ["AdvancedInterfaceOptions-2.1.14.zip"];
        assert_eq!(
            select_file(&zips, Some(&json), &target(ERA, Flavor::Classic)),
            Some("AdvancedInterfaceOptions-2.1.14.zip")
        );
    }

    #[test]
    fn zips_missing_from_release_json_are_judged_by_name() {
        let json = fixture("kuinameplates2-2.27.3a-release.json");
        let zips = [
            "Kui_Nameplates-2.27.3a-bcc.zip",
            "Kui_Nameplates-2.27.3a-classic.zip",
            "Kui_Nameplates-2.27.3a.zip",
        ];
        let pick = |interface, flavor| select_file(&zips, Some(&json), &target(interface, flavor));
        assert_eq!(
            pick(ANNIVERSARY, Flavor::Bcc),
            Some("Kui_Nameplates-2.27.3a-bcc.zip")
        );
        assert_eq!(
            pick(ERA, Flavor::Classic),
            Some("Kui_Nameplates-2.27.3a-classic.zip")
        );
        assert_eq!(
            pick(RETAIL, Flavor::Mainline),
            Some("Kui_Nameplates-2.27.3a.zip")
        );
        assert_eq!(pick(FOREVER, Flavor::Forever), None);
    }

    #[test]
    fn an_unlisted_lone_zip_is_not_used_for_other_flavors() {
        let json: ReleaseJson = serde_json::from_str(
            r#"{"releases": [
                {"filename": "A-bcc.zip", "metadata": [{"flavor": "bcc", "interface": 20506}]}
            ]}"#,
        )
        .unwrap();
        let zips = ["A-bcc.zip", "A.zip"];
        assert_eq!(
            select_file(&zips, Some(&json), &target(ERA, Flavor::Classic)),
            None
        );
    }

    #[test]
    fn an_override_matches_on_flavor_only() {
        let json = fixture("aio-2.1.14-release.json");
        let zips = aio_zips();
        // Without the override, the Forever client gets the main zip by interface number.
        let overridden = Target {
            interface: Some(FOREVER),
            flavor: Some(Flavor::Classic),
            overridden: true,
        };
        assert_eq!(
            select_file(&zips, Some(&json), &overridden),
            Some("AdvancedInterfaceOptions-2.1.14-classic.zip")
        );
    }

    #[test]
    fn file_names_decide_without_release_json() {
        let zips = [
            "MyAddon-1.2.0.zip",
            "MyAddon-1.2.0-classic.zip",
            "MyAddon-1.2.0-bcc.zip",
            "MyAddon-1.2.0-wotlkc.zip",
        ];
        let pick = |interface, flavor| select_file(&zips, None, &target(interface, flavor));
        assert_eq!(pick(RETAIL, Flavor::Mainline), Some("MyAddon-1.2.0.zip"));
        assert_eq!(
            pick(ERA, Flavor::Classic),
            Some("MyAddon-1.2.0-classic.zip")
        );
        assert_eq!(
            pick(ANNIVERSARY, Flavor::Bcc),
            Some("MyAddon-1.2.0-bcc.zip")
        );
        assert_eq!(pick(30405, Flavor::Wrath), Some("MyAddon-1.2.0-wotlkc.zip"));
        assert_eq!(pick(FOREVER, Flavor::Forever), None);
    }

    #[test]
    fn a_single_zip_is_used_unless_it_names_another_flavor() {
        let retail = target(RETAIL, Flavor::Mainline);
        let era = target(ERA, Flavor::Classic);
        assert_eq!(
            select_file(&["ls_UI-110207.02.zip"], None, &era),
            Some("ls_UI-110207.02.zip")
        );
        assert_eq!(select_file(&["Addon-1.0-classic.zip"], None, &retail), None);
        let unknown = Target {
            interface: None,
            flavor: None,
            overridden: false,
        };
        assert_eq!(
            select_file(&["Addon-1.0.zip"], None, &unknown),
            Some("Addon-1.0.zip")
        );
        assert_eq!(select_file(&["A-1.zip", "B-1.zip"], None, &era), None);
    }

    #[test]
    fn the_addon_name_itself_is_not_a_flavor() {
        assert_eq!(flavor_in_name("Classic-1.0.zip"), None);
        assert_eq!(flavor_in_name("ClassicCastbars-1.0.zip"), None);
        assert_eq!(
            flavor_in_name("Addon-1.0-Classic.ZIP"),
            Some(Flavor::Classic)
        );
        assert_eq!(flavor_in_name("Questie-v12.6.4+v1.0.12.zip"), None);
    }

    fn client(product: &str, interface: u32, game_type: GameType) -> GameClient {
        GameClient {
            folder: "_test_".into(),
            path: PathBuf::from("_test_"),
            product: product.into(),
            display_name: "Test".into(),
            version: None,
            interface: Some(interface),
            game_type: Some(game_type),
        }
    }

    #[test]
    fn targets_follow_the_game_type_unless_overridden() {
        let forever = client("wowf", FOREVER, GameType::Forever);
        assert_eq!(
            Target::for_client(&forever, &Settings::default()),
            target(FOREVER, Flavor::Forever)
        );
        let settings = Settings {
            flavor_overrides: BTreeMap::from([("wowf".to_string(), Flavor::Classic)]),
            ..Settings::default()
        };
        assert_eq!(
            Target::for_client(&forever, &settings),
            Target {
                interface: Some(FOREVER),
                flavor: Some(Flavor::Classic),
                overridden: true,
            }
        );
        let anniversary = client("wow_anniversary", ANNIVERSARY, GameType::Tbc);
        assert_eq!(
            Target::for_client(&anniversary, &settings),
            target(ANNIVERSARY, Flavor::Bcc)
        );
    }
}
