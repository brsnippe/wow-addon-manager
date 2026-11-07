//! Detects the game clients in a World of Warcraft installation.
//!
//! Each client lives in a `_name_` folder (`_retail_`, `_classic_era_`, ...) that contains a
//! `.flavor.info` with its product code. The root `.build.info` lists the installed version of
//! every product.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

pub const BUILD_INFO_FILE: &str = ".build.info";
pub const FLAVOR_INFO_FILE: &str = ".flavor.info";

/// Known products with their display names, in the order clients are listed.
const KNOWN_PRODUCTS: &[(&str, &str)] = &[
    ("wow", "Retail"),
    ("wowt", "Retail PTR"),
    ("wowxptr", "Retail Experimental PTR"),
    ("wow_beta", "Retail Beta"),
    ("wow_classic", "Classic"),
    ("wow_classic_ptr", "Classic PTR"),
    ("wow_classic_beta", "Classic Beta"),
    ("wow_anniversary", "Anniversary"),
    ("wow_classic_era", "Classic Era"),
    ("wow_classic_era_ptr", "Classic Era PTR"),
    ("wowf", "Forever"),
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameClient {
    /// Folder name, e.g. `_retail_`.
    pub folder: String,
    pub path: PathBuf,
    /// Product code from `.flavor.info`, e.g. `wow_classic_era`.
    pub product: String,
    pub display_name: String,
    /// Full version from `.build.info`, e.g. `12.1.5.70783`.
    pub version: Option<String>,
    /// TOC interface number derived from the version, e.g. `120105`.
    pub interface: Option<u32>,
    /// Game type the client reports to addons; decides which TOC file it loads.
    pub game_type: Option<GameType>,
}

/// The game types the client uses to pick a TOC file and evaluate `AllowLoadGameType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GameType {
    Standard,
    Forever,
    Vanilla,
    Tbc,
    Wrath,
    Cata,
    Mists,
}

impl GameType {
    /// Works out the game type from the product, falling back to the interface number for
    /// classic products, whose product code doesn't say which expansion they run.
    pub fn detect(product: &str, interface: Option<u32>) -> Option<GameType> {
        match product {
            "wow" | "wowt" | "wowxptr" | "wow_beta" => return Some(GameType::Standard),
            "wowf" => return Some(GameType::Forever),
            _ => {}
        }
        let interface = interface?;
        let (major, minor) = (interface / 10_000, interface / 100 % 100);
        Some(match major {
            0 => return None,
            // Forever is versioned 1.60 and up, and its beta ships as `wow_classic_beta`.
            1 if minor >= 50 => GameType::Forever,
            1 => GameType::Vanilla,
            2 => GameType::Tbc,
            3 => GameType::Wrath,
            4 => GameType::Cata,
            5 => GameType::Mists,
            _ => GameType::Standard,
        })
    }

    /// Names this game type answers to in TOC suffixes and `AllowLoadGameType`, preferred
    /// first. The family is not included.
    pub fn names(self) -> &'static [&'static str] {
        match self {
            GameType::Standard => &["Standard"],
            GameType::Forever => &["Camelot", "Forever"],
            GameType::Vanilla => &["Vanilla"],
            GameType::Tbc => &["TBC"],
            GameType::Wrath => &["Wrath"],
            GameType::Cata => &["Cata"],
            GameType::Mists => &["Mists"],
        }
    }

    /// The family name (`Mainline` or `Classic`), which also works as a TOC suffix.
    pub fn family(self) -> &'static str {
        match self {
            GameType::Standard | GameType::Forever => "Mainline",
            _ => "Classic",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildInfoEntry {
    pub product: String,
    pub version: String,
    pub active: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("World of Warcraft folder {0} does not exist")]
    RootMissing(PathBuf),
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// Lists every client folder under `wow_root`. Folders without a `.flavor.info` are skipped.
pub fn detect_clients(wow_root: &Path) -> Result<Vec<GameClient>, ClientError> {
    if !wow_root.is_dir() {
        return Err(ClientError::RootMissing(wow_root.to_owned()));
    }

    let build_info = match fs::read_to_string(wow_root.join(BUILD_INFO_FILE)) {
        Ok(text) => parse_build_info(&text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(source) => {
            return Err(ClientError::Read {
                path: wow_root.join(BUILD_INFO_FILE),
                source,
            });
        }
    };

    let entries = fs::read_dir(wow_root).map_err(|source| ClientError::Read {
        path: wow_root.to_owned(),
        source,
    })?;

    let mut clients = Vec::new();
    for entry in entries.flatten() {
        let Ok(folder) = entry.file_name().into_string() else {
            continue;
        };
        if !is_client_folder_name(&folder) || !entry.path().is_dir() {
            continue;
        }
        let Some(product) = fs::read_to_string(entry.path().join(FLAVOR_INFO_FILE))
            .ok()
            .and_then(|text| parse_flavor_info(&text))
        else {
            continue;
        };

        let version = version_for(&build_info, &product);
        let interface = version.as_deref().and_then(interface_number);
        clients.push(GameClient {
            display_name: display_name(&product, &folder),
            game_type: GameType::detect(&product, interface),
            interface,
            path: entry.path(),
            folder,
            product,
            version,
        });
    }

    clients.sort_by(|a, b| {
        product_rank(&a.product)
            .cmp(&product_rank(&b.product))
            .then_with(|| a.folder.cmp(&b.folder))
    });
    Ok(clients)
}

/// Parses `.build.info`: a `|`-separated table whose header cells look like `Name!TYPE:size`.
pub fn parse_build_info(text: &str) -> Vec<BuildInfoEntry> {
    let mut lines = strip_bom(text).lines().filter(|l| !l.trim().is_empty());
    let Some(header) = lines.next() else {
        return Vec::new();
    };
    let columns: Vec<&str> = header
        .split('|')
        .map(|cell| cell.split('!').next().unwrap_or("").trim())
        .collect();
    let index = |name: &str| columns.iter().position(|c| *c == name);
    let (Some(product_col), Some(version_col)) = (index("Product"), index("Version")) else {
        return Vec::new();
    };
    let active_col = index("Active");

    lines
        .filter_map(|line| {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            let product = cells.get(product_col).filter(|p| !p.is_empty())?;
            let version = cells.get(version_col).filter(|v| !v.is_empty())?;
            let active = active_col
                .and_then(|i| cells.get(i))
                .is_none_or(|a| *a == "1");
            Some(BuildInfoEntry {
                product: (*product).to_owned(),
                version: (*version).to_owned(),
                active,
            })
        })
        .collect()
}

/// Reads the product code from `.flavor.info`: a header line followed by the product.
pub fn parse_flavor_info(text: &str) -> Option<String> {
    strip_bom(text)
        .lines()
        .nth(1)
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
}

/// Converts a game version such as `12.1.5.70783` into its interface number (`120105`).
pub fn interface_number(version: &str) -> Option<u32> {
    let mut parts = version.split('.').map(|p| p.parse::<u32>().ok());
    let major = parts.next()??;
    let minor = parts.next()??;
    let patch = parts.next()??;
    (minor < 100 && patch < 100).then(|| major * 10_000 + minor * 100 + patch)
}

/// The display name for a product, or a name derived from the folder for unknown products.
pub fn display_name(product: &str, folder: &str) -> String {
    if let Some((_, name)) = KNOWN_PRODUCTS.iter().find(|(p, _)| *p == product) {
        return (*name).to_owned();
    }
    let words: Vec<String> = folder
        .split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default()
        })
        .collect();
    if words.is_empty() {
        product.to_owned()
    } else {
        words.join(" ")
    }
}

fn version_for(build_info: &[BuildInfoEntry], product: &str) -> Option<String> {
    let mut matching = build_info.iter().filter(|e| e.product == product);
    let first = matching.clone().next()?;
    let chosen = matching.find(|e| e.active).unwrap_or(first);
    Some(chosen.version.clone())
}

fn product_rank(product: &str) -> usize {
    KNOWN_PRODUCTS
        .iter()
        .position(|(p, _)| *p == product)
        .unwrap_or(KNOWN_PRODUCTS.len())
}

fn is_client_folder_name(name: &str) -> bool {
    name.len() > 2 && name.starts_with('_') && name.ends_with('_')
}

fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "Branch!STRING:0|Active!DEC:1|Version!STRING:0|Product!STRING:0";

    fn fixture_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wow")
    }

    fn make_client(root: &Path, folder: &str, product: &str) {
        let dir = root.join(folder);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(FLAVOR_INFO_FILE),
            format!("Product Flavor!STRING:0\n{product}\n"),
        )
        .unwrap();
    }

    #[test]
    fn detects_fixture_clients() {
        let clients = detect_clients(&fixture_root()).unwrap();
        let summary: Vec<_> = clients
            .iter()
            .map(|c| {
                (
                    c.folder.as_str(),
                    c.product.as_str(),
                    c.display_name.as_str(),
                    c.version.as_deref(),
                    c.interface,
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                (
                    "_retail_",
                    "wow",
                    "Retail",
                    Some("12.1.5.70783"),
                    Some(120105)
                ),
                (
                    "_classic_era_",
                    "wow_classic_era",
                    "Classic Era",
                    Some("1.15.9.70576"),
                    Some(11509)
                ),
                (
                    "_forever_",
                    "wowf",
                    "Forever",
                    Some("1.60.1.70862"),
                    Some(16001)
                ),
            ]
        );
        assert_eq!(clients[0].path, fixture_root().join("_retail_"));
        let game_types: Vec<_> = clients.iter().map(|c| c.game_type).collect();
        assert_eq!(
            game_types,
            [
                Some(GameType::Standard),
                Some(GameType::Vanilla),
                Some(GameType::Forever)
            ]
        );
    }

    #[test]
    fn unknown_product_gets_a_generic_name() {
        let dir = tempfile::tempdir().unwrap();
        make_client(dir.path(), "_new_flavor_", "wow_new_flavor");
        make_client(dir.path(), "_retail_", "wow");
        fs::write(
            dir.path().join(BUILD_INFO_FILE),
            format!("{HEADER}\neu|1|13.0.0.1|wow\neu|1|3.4.5.6|wow_new_flavor\n"),
        )
        .unwrap();

        let clients = detect_clients(dir.path()).unwrap();
        assert_eq!(clients.len(), 2);
        assert_eq!(clients[0].product, "wow", "known products come first");
        assert_eq!(clients[1].display_name, "New Flavor");
        assert_eq!(clients[1].interface, Some(30405));
    }

    #[test]
    fn missing_build_info_leaves_version_empty() {
        let dir = tempfile::tempdir().unwrap();
        make_client(dir.path(), "_retail_", "wow");
        let clients = detect_clients(dir.path()).unwrap();
        assert_eq!(clients[0].version, None);
        assert_eq!(clients[0].interface, None);
    }

    #[test]
    fn ignores_folders_that_are_not_clients() {
        let dir = tempfile::tempdir().unwrap();
        make_client(dir.path(), "_retail_", "wow");
        fs::create_dir_all(dir.path().join("_no_flavor_")).unwrap();
        make_client(dir.path(), "Data", "wow");
        fs::write(dir.path().join("_file_"), "not a folder").unwrap();
        let folders: Vec<_> = detect_clients(dir.path())
            .unwrap()
            .into_iter()
            .map(|c| c.folder)
            .collect();
        assert_eq!(folders, ["_retail_"]);
    }

    #[test]
    fn missing_root_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = detect_clients(&dir.path().join("nope")).unwrap_err();
        assert!(matches!(err, ClientError::RootMissing(_)));
    }

    #[test]
    fn build_info_columns_are_found_by_name() {
        let text = "\u{feff}Product!STRING:0|Version!STRING:0\r\nwow|12.1.5.70783\r\n\r\n";
        assert_eq!(
            parse_build_info(text),
            [BuildInfoEntry {
                product: "wow".into(),
                version: "12.1.5.70783".into(),
                active: true,
            }]
        );
    }

    #[test]
    fn build_info_without_needed_columns_is_empty() {
        assert!(parse_build_info("Branch!STRING:0\neu\n").is_empty());
        assert!(parse_build_info("").is_empty());
    }

    #[test]
    fn active_build_info_row_wins() {
        let entries =
            parse_build_info(&format!("{HEADER}\neu|0|12.0.0.1|wow\neu|1|12.1.5.2|wow\n"));
        assert_eq!(version_for(&entries, "wow").as_deref(), Some("12.1.5.2"));
        assert_eq!(version_for(&entries, "wowt"), None);
    }

    #[test]
    fn flavor_info_handles_bom_and_crlf() {
        assert_eq!(
            parse_flavor_info("\u{feff}Product Flavor!STRING:0\r\nwow_anniversary\r\n").as_deref(),
            Some("wow_anniversary")
        );
        assert_eq!(parse_flavor_info("Product Flavor!STRING:0\n"), None);
    }

    #[test]
    fn game_types() {
        let detect = GameType::detect;
        assert_eq!(detect("wow", Some(120105)), Some(GameType::Standard));
        assert_eq!(detect("wowt", None), Some(GameType::Standard));
        assert_eq!(detect("wowf", Some(16001)), Some(GameType::Forever));
        assert_eq!(
            detect("wow_classic_era", Some(11509)),
            Some(GameType::Vanilla)
        );
        assert_eq!(detect("wow_anniversary", Some(20506)), Some(GameType::Tbc));
        assert_eq!(detect("wow_classic", Some(30405)), Some(GameType::Wrath));
        assert_eq!(detect("wow_classic", Some(38002)), Some(GameType::Wrath));
        assert_eq!(detect("wow_classic", Some(40402)), Some(GameType::Cata));
        assert_eq!(detect("wow_classic", Some(50504)), Some(GameType::Mists));
        assert_eq!(
            detect("wow_classic_beta", Some(16001)),
            Some(GameType::Forever)
        );
        assert_eq!(detect("wow_new", Some(130000)), Some(GameType::Standard));
        assert_eq!(detect("wow_classic", None), None);
    }

    #[test]
    fn interface_numbers() {
        assert_eq!(interface_number("12.1.5.70783"), Some(120105));
        assert_eq!(interface_number("1.15.9.70576"), Some(11509));
        assert_eq!(interface_number("2.5.6.70577"), Some(20506));
        assert_eq!(interface_number("1.60.1"), Some(16001));
        assert_eq!(interface_number("1.15"), None);
        assert_eq!(interface_number("1.100.0"), None);
        assert_eq!(interface_number("abc"), None);
    }
}
