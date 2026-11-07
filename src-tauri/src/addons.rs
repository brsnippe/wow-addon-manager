//! Lists the addons installed for a game client.
//!
//! Many addons ship several folders (`BigWigs`, `BigWigs_Core`, ...). Folders are grouped into
//! one entry using, in order:
//!
//! 1. Shared source IDs (`X-Curse-Project-ID`, `X-Wago-ID`, `X-WoWI-ID`): the packager writes
//!    the project's IDs into each TOC, so an identical ID means the same project.
//! 2. An explicit `## Group:` naming another installed folder.
//! 3. For folders with neither, the closest relative: the folder with the longest shared name
//!    prefix that it has a dependency link with, or a sibling carrying source IDs that requires
//!    the same addon (`LittleWigs_*` requires `BigWigs`, as `LittleWigs` does). A folder only
//!    joins a relative with IDs that it depends on itself, never a plugin that depends on it.
//!
//! Folders with source IDs of their own are never pulled into another project by rule 3. IDs
//! missing from the loaded TOC are taken from the folder's other TOC files.
//!
//! The entry's metadata comes from its main folder: the one most `Group` directives point at,
//! then one with source IDs, then the one most other folders depend on, then the shortest name.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs;
use std::io;
use std::path::Path;

use serde::Serialize;

use crate::clients::{GameClient, GameType};
use crate::toc::{self, Toc};

/// An addon folder with the TOC the client loads from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddonFolder {
    pub name: String,
    pub toc: Toc,
}

/// One addon as shown to the user: its main folder's metadata plus every folder it owns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledAddon {
    /// Main folder name; identifies the addon within a client.
    pub name: String,
    pub title: String,
    pub notes: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub interfaces: Vec<u32>,
    /// Every folder of the addon, main folder first.
    pub folders: Vec<String>,
    pub website: Option<String>,
    pub curse_project_id: Option<String>,
    pub wago_id: Option<String>,
    pub wowi_id: Option<String>,
}

/// Lists the addons in the client's `Interface/AddOns` folder, sorted by title.
pub fn list_addons(client: &GameClient) -> io::Result<Vec<InstalledAddon>> {
    let dir = client.path.join("Interface").join("AddOns");
    Ok(group_folders(scan_folders(&dir, client.game_type)?))
}

/// Reads every folder in `addons_dir` that has a TOC the client would load, sorted by name.
/// A missing `addons_dir` means no addons.
pub fn scan_folders(addons_dir: &Path, game: Option<GameType>) -> io::Result<Vec<AddonFolder>> {
    let entries = match fs::read_dir(addons_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut folders: Vec<AddonFolder> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let path = toc::find_toc(&entry.path(), game)?;
            let mut toc = toc::read_toc(&path, game).ok()?;
            fill_ids_from_other_tocs(&mut toc, &entry.path(), &path, game);
            Some(AddonFolder { name, toc })
        })
        .collect();
    folders.sort_by_key(|f| f.name.to_lowercase());
    Ok(folders)
}

/// Source IDs identify the project, not a game flavor, but packagers sometimes leave them out of
/// flavor TOCs. Fills IDs missing from the loaded TOC from the folder's other TOC files,
/// unsuffixed file first.
fn fill_ids_from_other_tocs(toc: &mut Toc, dir: &Path, loaded: &Path, game: Option<GameType>) {
    let ids =
        |t: &Toc| [&t.curse_project_id, &t.wago_id, &t.wowi_id].map(|id| valid_id(id).is_some());
    if ids(toc).iter().all(|&has| has) {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let stem = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_lowercase();
    let mut others: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path != loaded
                && path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("toc"))
        })
        .collect();
    others.sort_by_key(|path| {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_lowercase();
        (name != stem, name)
    });
    for path in others {
        let Ok(other) = toc::read_toc(&path, game) else {
            continue;
        };
        for (mine, theirs) in [
            (&mut toc.curse_project_id, &other.curse_project_id),
            (&mut toc.wago_id, &other.wago_id),
            (&mut toc.wowi_id, &other.wowi_id),
        ] {
            if valid_id(mine).is_none() && valid_id(theirs).is_some() {
                mine.clone_from(theirs);
            }
        }
        if ids(toc).iter().all(|&has| has) {
            return;
        }
    }
}

/// Groups folders that belong to one addon into a single entry. Ungrouped folders become entries
/// of their own.
pub fn group_folders(mut folders: Vec<AddonFolder>) -> Vec<InstalledAddon> {
    folders.sort_by_key(|f| f.name.to_lowercase());
    let n = folders.len();
    let index: HashMap<String, usize> = folders
        .iter()
        .enumerate()
        .map(|(i, f)| (f.name.to_lowercase(), i))
        .collect();
    let find = |name: &str| index.get(&name.to_lowercase()).copied();

    let ids: Vec<Vec<(&str, &str)>> = folders.iter().map(|f| source_ids(&f.toc)).collect();
    let group_target: Vec<Option<usize>> = folders
        .iter()
        .enumerate()
        .map(|(i, f)| f.toc.group.as_deref().and_then(find).filter(|&t| t != i))
        .collect();
    let links: Vec<Vec<usize>> = folders
        .iter()
        .map(|f| {
            let toc = &f.toc;
            toc.dependencies
                .iter()
                .chain(&toc.optional_dependencies)
                .chain(&toc.load_with)
                .filter_map(|dep| find(dep))
                .collect()
        })
        .collect();

    let mut sets = DisjointSet::new(n);

    let mut id_owner: HashMap<(&str, &str), usize> = HashMap::new();
    for (i, folder_ids) in ids.iter().enumerate() {
        for &id in folder_ids {
            match id_owner.entry(id) {
                Entry::Occupied(owner) => sets.union(*owner.get(), i),
                Entry::Vacant(slot) => {
                    slot.insert(i);
                }
            }
        }
    }

    for (i, target) in group_target.iter().enumerate() {
        if let Some(target) = *target {
            sets.union(i, target);
        }
    }

    for i in 0..n {
        if !ids[i].is_empty() || group_target[i].is_some() {
            continue;
        }
        let closest = (0..n)
            .filter(|&j| j != i)
            .filter_map(|j| {
                let prefix = shared_name_prefix(&folders[i].name, &folders[j].name);
                let has_ids = !ids[j].is_empty();
                // A dependent with IDs of its own is a separate plugin (`Details_Streamer`), unless
                // its name is this folder's stem (`Questie` requires `QuestieDB`).
                let stem = prefix == folders[j].name.to_lowercase().len();
                let direct = links[i].contains(&j) || (links[j].contains(&i) && (!has_ids || stem));
                let sibling = has_ids && shares_dependency(&folders[i].toc, &folders[j].toc);
                (prefix > 0 && (direct || sibling)).then_some((prefix, direct, has_ids, Reverse(j)))
            })
            .max();
        if let Some((.., Reverse(j))) = closest {
            sets.union(i, j);
        }
    }

    let mut members: Vec<Vec<usize>> = Vec::new();
    let mut slot_of_root: HashMap<usize, usize> = HashMap::new();
    for i in 0..n {
        let slot = *slot_of_root.entry(sets.find(i)).or_insert_with(|| {
            members.push(Vec::new());
            members.len() - 1
        });
        members[slot].push(i);
    }

    let mut addons: Vec<InstalledAddon> = members
        .into_iter()
        .map(|group| {
            let main = *group
                .iter()
                .max_by_key(|&&m| {
                    let pointing = group
                        .iter()
                        .filter(|&&o| group_target[o] == Some(m))
                        .count();
                    let dependents = group
                        .iter()
                        .filter(|&&o| o != m && links[o].contains(&m))
                        .count();
                    let name = &folders[m].name;
                    (
                        pointing,
                        !ids[m].is_empty(),
                        dependents,
                        Reverse(name.len()),
                        Reverse(m),
                    )
                })
                .expect("groups are never empty");
            let ordered: Vec<&AddonFolder> = std::iter::once(main)
                .chain(group.iter().copied().filter(|&m| m != main))
                .map(|m| &folders[m])
                .collect();
            build_entry(&ordered)
        })
        .collect();
    addons.sort_by(|a, b| {
        a.title
            .to_lowercase()
            .cmp(&b.title.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
    });
    addons
}

/// Builds an entry from its folders (main folder first). Source IDs missing from the main TOC
/// are taken from the first other folder that has them.
fn build_entry(folders: &[&AddonFolder]) -> InstalledAddon {
    let main = folders[0];
    let first_id = |get: fn(&Toc) -> &Option<String>| {
        folders
            .iter()
            .find_map(|f| valid_id(get(&f.toc)))
            .map(str::to_owned)
    };
    InstalledAddon {
        name: main.name.clone(),
        title: main.toc.title.clone().unwrap_or_else(|| main.name.clone()),
        notes: main.toc.notes.clone(),
        version: main.toc.version.clone(),
        author: main.toc.author.clone(),
        interfaces: main.toc.interfaces.clone(),
        folders: folders.iter().map(|f| f.name.clone()).collect(),
        website: first_id(|t| &t.website),
        curse_project_id: first_id(|t| &t.curse_project_id),
        wago_id: first_id(|t| &t.wago_id),
        wowi_id: first_id(|t| &t.wowi_id),
    }
}

/// The source IDs in a TOC, tagged by source. Placeholders such as `0` or `@project-id@` are
/// not IDs.
fn source_ids(toc: &Toc) -> Vec<(&'static str, &str)> {
    [
        ("curse", &toc.curse_project_id),
        ("wago", &toc.wago_id),
        ("wowi", &toc.wowi_id),
    ]
    .into_iter()
    .filter_map(|(source, id)| Some((source, valid_id(id)?)))
    .collect()
}

fn valid_id(id: &Option<String>) -> Option<&str> {
    id.as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty() && *id != "0" && !id.contains('@'))
}

fn shares_dependency(a: &Toc, b: &Toc) -> bool {
    a.dependencies
        .iter()
        .any(|dep| b.dependencies.iter().any(|d| d.eq_ignore_ascii_case(dep)))
}

/// Length of the name prefix two folders share, ignoring case. A partial word doesn't count:
/// the shared part must be a whole name (`Wigs` / `Wigs_Core`) or end at a separator
/// (`DBM-Party-BC` / `DBM-Party-MoP` share `DBM-Party`).
fn shared_name_prefix(a: &str, b: &str) -> usize {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let common: usize = a
        .chars()
        .zip(b.chars())
        .take_while(|(x, y)| x == y)
        .map(|(x, _)| x.len_utf8())
        .sum();
    if common == a.len() || common == b.len() {
        return common;
    }
    a[..common].rfind(['-', '_', '.', ' ']).unwrap_or(0)
}

struct DisjointSet {
    parent: Vec<usize>,
}

impl DisjointSet {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
        }
    }

    fn find(&mut self, i: usize) -> usize {
        let parent = self.parent[i];
        if parent == i {
            return i;
        }
        let root = self.find(parent);
        self.parent[i] = root;
        root
    }

    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.parent[b.max(a)] = a.min(b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture_client(folder: &str, game_type: GameType) -> GameClient {
        let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/wow")
            .join(folder);
        GameClient {
            folder: folder.into(),
            path,
            product: String::new(),
            display_name: String::new(),
            version: None,
            interface: None,
            game_type: Some(game_type),
        }
    }

    /// A folder whose TOC is described as `key=value` pairs; list values are comma-separated.
    fn folder(name: &str, directives: &[(&str, &str)]) -> AddonFolder {
        let mut toc = Toc::default();
        let list = |v: &str| v.split(',').map(|s| s.trim().to_owned()).collect();
        for &(key, value) in directives {
            match key {
                "deps" => toc.dependencies = list(value),
                "optional" => toc.optional_dependencies = list(value),
                "loadwith" => toc.load_with = list(value),
                "group" => toc.group = Some(value.into()),
                "curse" => toc.curse_project_id = Some(value.into()),
                "wago" => toc.wago_id = Some(value.into()),
                "wowi" => toc.wowi_id = Some(value.into()),
                "title" => toc.title = Some(value.into()),
                _ => panic!("unknown key {key}"),
            }
        }
        AddonFolder {
            name: name.into(),
            toc,
        }
    }

    fn groups(folders: Vec<AddonFolder>) -> Vec<Vec<String>> {
        let mut groups: Vec<_> = group_folders(folders)
            .into_iter()
            .map(|a| a.folders)
            .collect();
        groups.sort();
        groups
    }

    fn names(groups: &[&[&str]]) -> Vec<Vec<String>> {
        let mut groups: Vec<Vec<String>> = groups
            .iter()
            .map(|g| g.iter().map(|s| s.to_string()).collect())
            .collect();
        groups.sort();
        groups
    }

    #[test]
    fn groups_the_fixture_install() {
        let addons = list_addons(&fixture_client("_retail_", GameType::Standard)).unwrap();
        let summary: Vec<_> = addons
            .iter()
            .map(|a| (a.title.as_str(), a.folders.join(" ")))
            .collect();
        assert_eq!(
            summary,
            [
                ("BOM and CRLF Addon", "BomCrlf".to_owned()),
                (
                    "Boss Mods Core",
                    "BossMods-Core BossMods-Dungeons BossMods-Raids BossMods-StatusBars".into()
                ),
                (
                    "Boss Mods Old Dungeons",
                    "BossMods-Party-Old BossMods-Party-New".into()
                ),
                ("Multi TOC", "MultiToc".into()),
                ("Simple Addon", "SimpleAddon".into()),
                ("Small Wigs", "SmallWigs SmallWigs_Dungeon".into()),
                ("Wigs", "Wigs Wigs_Core Wigs_Options Wigs_Raid".into()),
            ]
        );
        let wigs = addons.last().unwrap();
        assert_eq!(wigs.version.as_deref(), Some("v400"));
        assert_eq!(wigs.curse_project_id.as_deref(), Some("333333"));
        assert_eq!(wigs.wago_id.as_deref(), Some("wigsWago"));
    }

    #[test]
    fn missing_addons_folder_means_no_addons() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            scan_folders(&dir.path().join("nope"), None)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn dbm_style_packages_split_by_source_id() {
        let core = ("curse", "3358");
        let dungeons = ("curse", "314994");
        let groups = groups(vec![
            folder(
                "DBM-Core",
                &[("group", "DBM-Core"), ("deps", "DBM-StatusBarTimers"), core],
            ),
            folder("DBM-StatusBarTimers", &[("group", "DBM-Core")]),
            folder("DBM-GUI", &[("group", "DBM-Core"), ("deps", "DBM-Core")]),
            folder("DBM-Raids-Midnight", &[("deps", "DBM-Core"), core]),
            folder("DBM-Azeroth", &[("deps", "DBM-Core")]),
            folder("DBM-Party-BC", &[("deps", "DBM-Core"), dungeons]),
            folder("DBM-Party-Cataclysm", &[("deps", "DBM-Core"), dungeons]),
            folder("DBM-Party-BfA", &[("deps", "DBM-Core")]),
            folder(
                "DBM-Raids-Vanilla",
                &[("deps", "DBM-Core"), ("curse", "16442")],
            ),
        ]);
        assert_eq!(
            groups,
            names(&[
                &[
                    "DBM-Core",
                    "DBM-Azeroth",
                    "DBM-GUI",
                    "DBM-Raids-Midnight",
                    "DBM-StatusBarTimers"
                ],
                &["DBM-Party-BC", "DBM-Party-BfA", "DBM-Party-Cataclysm"],
                &["DBM-Raids-Vanilla"],
            ])
        );
    }

    #[test]
    fn bigwigs_style_modules_join_their_own_package() {
        let groups = groups(vec![
            folder("BigWigs", &[("curse", "2382"), ("wago", "5NRegwG3")]),
            folder("BigWigs_Core", &[("deps", "BigWigs")]),
            folder("BigWigs_Plugins", &[("deps", "BigWigs, BigWigs_Core")]),
            folder("BigWigs_TheVoidspire", &[("deps", "BigWigs")]),
            folder("LittleWigs", &[("deps", "BigWigs"), ("curse", "4383")]),
            folder("LittleWigs_Legion", &[("deps", "BigWigs")]),
            folder("LittleWigs_Classic", &[("deps", "BigWigs")]),
        ]);
        assert_eq!(
            groups,
            names(&[
                &[
                    "BigWigs",
                    "BigWigs_Core",
                    "BigWigs_Plugins",
                    "BigWigs_TheVoidspire"
                ],
                &["LittleWigs", "LittleWigs_Classic", "LittleWigs_Legion"],
            ])
        );
    }

    #[test]
    fn data_modules_join_the_addon_they_depend_on() {
        let groups = groups(vec![
            folder("RaiderIO", &[("curse", "279257")]),
            folder("RaiderIO_DB_EU_F", &[("deps", "RaiderIO")]),
            folder("RaiderIO_DB_EU_M", &[("deps", "RaiderIO")]),
            folder("RaiderIO_DB_US_F", &[("deps", "RaiderIO")]),
        ]);
        assert_eq!(
            groups,
            names(&[&[
                "RaiderIO",
                "RaiderIO_DB_EU_F",
                "RaiderIO_DB_EU_M",
                "RaiderIO_DB_US_F"
            ]])
        );
    }

    #[test]
    fn folders_with_their_own_ids_stay_separate() {
        let groups = groups(vec![
            folder("Masque", &[("curse", "13592")]),
            folder("Masque_Dominos", &[("deps", "Masque"), ("curse", "699643")]),
            folder("Details", &[("wago", "qv63A7Gb")]),
            folder(
                "Details_TinyThreat",
                &[("deps", "Details"), ("wago", "Rn6VJW6d")],
            ),
            folder("Details_Compare2", &[("deps", "Details")]),
        ]);
        assert_eq!(
            groups,
            names(&[
                &["Details", "Details_Compare2"],
                &["Details_TinyThreat"],
                &["Masque"],
                &["Masque_Dominos"],
            ])
        );
    }

    #[test]
    fn main_addon_without_ids_is_not_pulled_into_its_plugin() {
        let groups = groups(vec![
            folder("Details", &[]),
            folder("Details_Compare2", &[("deps", "Details")]),
            folder(
                "Details_Streamer",
                &[("deps", "Details"), ("wago", "VBNB9MGx")],
            ),
        ]);
        assert_eq!(
            groups,
            names(&[&["Details", "Details_Compare2"], &["Details_Streamer"]])
        );
    }

    #[test]
    fn data_folders_join_the_addon_that_requires_them() {
        let groups = groups(vec![
            folder("Questie", &[("deps", "QuestieDB"), ("curse", "334372")]),
            folder("QuestieDB", &[]),
        ]);
        assert_eq!(groups, names(&[&["Questie", "QuestieDB"]]));
    }

    #[test]
    fn ids_missing_from_the_loaded_toc_come_from_other_tocs() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Addon");
        fs::create_dir(&dir).unwrap();
        let write = |file: &str, text: &str| fs::write(dir.join(file), text).unwrap();
        write(
            "Addon_Vanilla.toc",
            "## Title: Vanilla\n## X-Wago-ID: own\n",
        );
        write(
            "Addon_Mists.toc",
            "## X-Curse-Project-ID: 2\n## X-WoWI-ID: 3\n",
        );
        write(
            "Addon.toc",
            "## X-Curse-Project-ID: 1\n## X-Wago-ID: other\n",
        );

        let folders = scan_folders(root.path(), Some(GameType::Vanilla)).unwrap();
        let toc = &folders[0].toc;
        assert_eq!(toc.title.as_deref(), Some("Vanilla"));
        assert_eq!(toc.wago_id.as_deref(), Some("own"));
        assert_eq!(toc.curse_project_id.as_deref(), Some("1"));
        assert_eq!(toc.wowi_id.as_deref(), Some("3"));
    }

    #[test]
    fn explicit_group_wins_and_names_the_main_folder() {
        let addons = group_folders(vec![
            folder(
                "WagoAnalytics",
                &[("group", "WagoAppCompanion"), ("wago", "a")],
            ),
            folder("WagoAppCompanion", &[("group", "WagoAppCompanion")]),
            folder("WagoUI", &[("group", "WagoAppCompanion"), ("wago", "b")]),
            folder("EllesmereUIBags", &[("group", "EllesmereUI")]),
            folder("EllesmereUI", &[]),
            folder("Orphan", &[("group", "NotInstalled")]),
        ]);
        let folders: Vec<_> = addons.iter().map(|a| a.folders.join(" ")).collect();
        assert_eq!(
            folders,
            [
                "EllesmereUI EllesmereUIBags",
                "Orphan",
                "WagoAppCompanion WagoAnalytics WagoUI"
            ]
        );
    }

    #[test]
    fn unrelated_names_or_missing_links_are_not_grouped() {
        let groups = groups(vec![
            folder(
                "ElvUI",
                &[("deps", "ElvUI_Libraries"), ("optional", "BigWigs")],
            ),
            folder("ElvUI_Libraries", &[("group", "ElvUI_Libraries")]),
            folder(
                "ElvUI_Options",
                &[("deps", "ElvUI"), ("group", "ElvUI_Options")],
            ),
            folder("BigWigs", &[]),
            folder("Dominos", &[]),
            folder("Dominos_Cast", &[("loadwith", "Dominos")]),
            folder("ExtraBars", &[("deps", "Dominos")]),
            folder("Bagnon", &[]),
            folder("Bagnon_Config", &[]),
        ]);
        assert_eq!(
            groups,
            names(&[
                &["Bagnon"],
                &["Bagnon_Config"],
                &["BigWigs"],
                &["Dominos", "Dominos_Cast"],
                &["ElvUI", "ElvUI_Libraries", "ElvUI_Options"],
                &["ExtraBars"],
            ])
        );
    }

    #[test]
    fn entry_fills_missing_ids_from_other_folders() {
        let addons = group_folders(vec![
            folder("Pack", &[("title", "The Pack"), ("curse", "1")]),
            folder("Pack_Extra", &[("curse", "1"), ("wowi", "77")]),
            folder("Placeholder", &[("curse", "0"), ("wago", "@wago-id@")]),
        ]);
        assert_eq!(addons[0].name, "Placeholder");
        assert_eq!(
            addons[0].title, "Placeholder",
            "falls back to the folder name"
        );
        assert_eq!(addons[0].curse_project_id, None);
        assert_eq!(addons[0].wago_id, None);
        assert_eq!(addons[1].title, "The Pack");
        assert_eq!(addons[1].wowi_id.as_deref(), Some("77"));
    }

    #[test]
    fn name_prefixes_stop_at_word_boundaries() {
        assert_eq!(shared_name_prefix("Wigs", "Wigs_Core"), 4);
        assert_eq!(shared_name_prefix("DBM-Party-BC", "DBM-Party-MoP"), 9);
        assert_eq!(shared_name_prefix("DBM-Core", "DBM-Party-BC"), 3);
        assert_eq!(
            shared_name_prefix("ClassReminders", "ClassRemindersPets"),
            14
        );
        assert_eq!(
            shared_name_prefix("ClassRemindersBars", "ClassRemindersPets"),
            0
        );
        assert_eq!(shared_name_prefix("bigwigs", "BigWigs_Core"), 7);
        assert_eq!(shared_name_prefix("Wigs", "SmallWigs"), 0);
    }
}
