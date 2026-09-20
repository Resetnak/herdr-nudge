//! Loading the captured fixtures.
//!
//! Tests feed the code the same bytes Herdr fed the probe plugin, rather than
//! JSON we wrote ourselves. `tests/fixtures/README.md` describes the format
//! and `tools/fixtures/extract.py` regenerates them.

#![allow(dead_code)] // Not every helper has a caller yet.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use herdr_nudge::context::{Context, Env};
use herdr_nudge::event::Envelope;
use serde::Deserialize;

/// One `tests/fixtures/events/<category>/<name>.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct Fixture {
    /// Filled in by the loader, not present in the file: `agent/blocked`.
    #[serde(skip)]
    pub name: String,
    /// Where this was captured from: `tests/fixtures/raw/<log>:<line>`.
    pub source: String,
    pub captured_at: String,
    /// `manual`, `programmatic` or `unknown`. The two don't always behave
    /// the same, so a programmatic capture says nothing about manual use.
    pub provenance: String,
    pub mark: Option<String>,
    pub why: String,
    /// The dotted `HERDR_PLUGIN_EVENT` name, e.g. `pane.agent_status_changed`.
    pub event: String,
    /// `HERDR_PLUGIN_EVENT_JSON`, verbatim, as a string.
    pub event_json: String,
    /// The rest of the `HERDR_*` environment, verbatim.
    pub env: BTreeMap<String, String>,
}

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

impl Fixture {
    /// `Fixture::load("agent/blocked")`.
    pub fn load(name: &str) -> Fixture {
        let path = fixtures_dir().join("events").join(format!("{name}.json"));
        let json = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("reading fixture {}: {e}", path.display()));
        let mut fixture: Fixture = serde_json::from_str(&json)
            .unwrap_or_else(|e| panic!("parsing fixture {}: {e}", path.display()));
        fixture.name = name.to_owned();
        fixture
    }

    /// Every fixture under `events/`, sorted by name.
    pub fn all() -> Vec<Fixture> {
        let root = fixtures_dir().join("events");
        let mut names = Vec::new();
        for category in read_dir_sorted(&root) {
            if !category.is_dir() {
                continue;
            }
            let category_name = file_stem(&category);
            for file in read_dir_sorted(&category) {
                if file.extension().is_some_and(|e| e == "json") {
                    names.push(format!("{category_name}/{}", file_stem(&file)));
                }
            }
        }
        assert!(
            !names.is_empty(),
            "no fixtures found under {}",
            root.display()
        );
        names.iter().map(|n| Fixture::load(n)).collect()
    }

    /// `agent`, `shell`, `detected`, `focus` or `lifecycle`.
    pub fn category(&self) -> &str {
        self.name.split('/').next().unwrap_or(&self.name)
    }

    /// The captured payload, parsed. Panics with the fixture name so a
    /// failure says which capture it was.
    pub fn envelope(&self) -> Envelope {
        Envelope::parse(&self.event_json)
            .unwrap_or_else(|e| panic!("{}: parsing event_json: {e}", self.name))
    }

    pub fn context_json(&self) -> &str {
        self.env
            .get("HERDR_PLUGIN_CONTEXT_JSON")
            .unwrap_or_else(|| panic!("{}: no HERDR_PLUGIN_CONTEXT_JSON", self.name))
    }

    pub fn context(&self) -> Context {
        Context::parse(self.context_json())
            .unwrap_or_else(|e| panic!("{}: parsing context: {e}", self.name))
    }

    /// The captured environment. These paths belong to the probe plugin, so
    /// a test that writes files needs to override them.
    pub fn herdr_env(&self) -> Env {
        Env::from_map(&self.env).unwrap_or_else(|e| panic!("{}: {e}", self.name))
    }

    /// The captured `event_json` with one field of `data` changed.
    ///
    /// For the cases Herdr has never sent us: it leaves optional fields out
    /// instead of nulling them, and it can't send a status that doesn't
    /// exist yet. Changing one field of a real capture beats writing an event
    /// by hand.
    pub fn event_json_with(&self, key: &str, value: serde_json::Value) -> String {
        let mut root: serde_json::Value = serde_json::from_str(&self.event_json)
            .unwrap_or_else(|e| panic!("{}: parsing event_json: {e}", self.name));
        let object = root
            .get_mut("data")
            .and_then(serde_json::Value::as_object_mut)
            .unwrap_or_else(|| panic!("{}: data is not an object", self.name));
        object.insert(key.to_owned(), value);
        root.to_string()
    }

    /// The captured `event_json` with one field of `data` set to null.
    pub fn event_json_with_null(&self, key: &str) -> String {
        self.event_json_with(key, serde_json::Value::Null)
    }
}

fn read_dir_sorted(dir: &Path) -> Vec<PathBuf> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()))
        .map(|e| e.expect("directory entry").path())
        .collect();
    entries.sort();
    entries
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .unwrap_or_else(|| panic!("no file stem: {}", path.display()))
        .to_string_lossy()
        .into_owned()
}

/// One line of a probe log in `tests/fixtures/raw/`.
///
/// `tools/probe/dump.sh` writes `HH:MM:SS\t<event>\t<json>` per invocation,
/// then the environment indented by four spaces. `tools/probe/mark.sh` writes
/// `HH:MM:SS\t#mark\t<note>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// 1-based, matching the `<log>:<line>` in a fixture's `source`.
    pub line: usize,
    pub time: String,
    /// The dotted event name, or `#mark`.
    pub kind: String,
    pub rest: String,
}

impl Record {
    pub fn is_mark(&self) -> bool {
        self.kind == "#mark"
    }
}

/// Every event and mark in a probe log, in order. The indented environment
/// lines are skipped.
pub fn raw_log(file_name: &str) -> Vec<Record> {
    let path = fixtures_dir().join("raw").join(file_name);
    let text =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));

    let mut records = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.starts_with(' ') || line.is_empty() {
            continue;
        }
        let mut fields = line.splitn(3, '\t');
        let (Some(time), Some(kind)) = (fields.next(), fields.next()) else {
            continue;
        };
        records.push(Record {
            line: index + 1,
            time: time.to_owned(),
            kind: kind.to_owned(),
            rest: fields.next().unwrap_or("").to_owned(),
        });
    }
    assert!(!records.is_empty(), "no records in {}", path.display());
    records
}

/// Index of the one mark whose note contains `needle`.
pub fn mark_index(records: &[Record], needle: &str) -> usize {
    let matches: Vec<usize> = records
        .iter()
        .enumerate()
        .filter(|(_, r)| r.is_mark() && r.rest.contains(needle))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "expected exactly one mark containing {needle:?}, found {}",
        matches.len()
    );
    matches[0]
}
