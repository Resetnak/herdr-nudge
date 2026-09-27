//! The logos in `icons/agents/` against the rules the code and NOTICE.md
//! rely on. A file that breaks one would be silently unused, or shipped
//! with no record of whose it is.

use std::fs;
use std::path::{Path, PathBuf};

use herdr_nudge::handler::is_logo_label;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn pngs(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .map(|entry| entry.expect("dir entry"))
        .filter(|entry| entry.file_type().expect("file type").is_file())
        .map(|entry| entry.file_name().into_string().expect("utf-8 name"))
        // Finder's .DS_Store and the like, which git ignores anyway.
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    names
}

#[test]
fn every_logo_is_a_png_named_after_a_plain_label() {
    let icons = root().join("icons/agents");
    for dir in [icons.clone(), icons.join("dark")] {
        for name in pngs(&dir) {
            let label = name
                .strip_suffix(".png")
                .unwrap_or_else(|| panic!("{}/{name}: not a .png", dir.display()));
            assert!(
                is_logo_label(label),
                "{}/{name}: the handler would never look for this name",
                dir.display()
            );
            let bytes = fs::read(dir.join(&name)).expect("read logo");
            assert!(
                bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
                "{}/{name}: not PNG data",
                dir.display()
            );
        }
    }
}

/// The dark copy is only looked for once the light one is found.
#[test]
fn every_dark_logo_has_a_light_one() {
    let icons = root().join("icons/agents");
    let light = pngs(&icons);
    for name in pngs(&icons.join("dark")) {
        assert!(
            light.contains(&name),
            "icons/agents/dark/{name} has no icons/agents/{name}"
        );
    }
}

#[test]
fn every_shipped_image_is_in_the_notice() {
    let notice = fs::read_to_string(root().join("NOTICE.md")).expect("read NOTICE.md");
    let icons = root().join("icons/agents");
    let mut shipped: Vec<String> = pngs(&icons)
        .into_iter()
        .map(|n| format!("icons/agents/{n}"))
        .collect();
    shipped.extend(
        pngs(&icons.join("dark"))
            .into_iter()
            .map(|n| format!("icons/agents/dark/{n}")),
    );
    shipped.push("assets/herdr-logo.png".to_owned());
    for path in shipped {
        assert!(
            notice.contains(&format!("`{path}`")),
            "{path} is not listed in NOTICE.md"
        );
    }
}
