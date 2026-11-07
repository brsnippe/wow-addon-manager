//! Guards the fixture installation that other tests rely on.

use std::fs;
use std::path::PathBuf;

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wow")
}

#[test]
fn every_client_has_a_flavor_and_addons_folder() {
    for (client, product) in [
        ("_retail_", "wow"),
        ("_classic_era_", "wow_classic_era"),
        ("_forever_", "wowf"),
    ] {
        let dir = fixture_root().join(client);
        let flavor = fs::read_to_string(dir.join(".flavor.info")).unwrap();
        assert_eq!(flavor.lines().nth(1), Some(product), "{client}");
        assert!(dir.join("Interface/AddOns").is_dir(), "{client}");
    }
}

#[test]
fn build_info_lists_every_client_product() {
    let build_info = fs::read_to_string(fixture_root().join(".build.info")).unwrap();
    for product in ["wow", "wow_classic_era", "wowf"] {
        assert!(
            build_info
                .lines()
                .skip(1)
                .any(|line| line.ends_with(&format!("|{product}"))),
            "{product}"
        );
    }
}

#[test]
fn bom_crlf_toc_kept_its_bytes() {
    let bytes =
        fs::read(fixture_root().join("_retail_/Interface/AddOns/BomCrlf/BomCrlf.toc")).unwrap();
    assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]), "UTF-8 BOM missing");
    assert!(
        bytes.windows(2).any(|w| w == b"\r\n"),
        "CRLF line endings missing"
    );
}
