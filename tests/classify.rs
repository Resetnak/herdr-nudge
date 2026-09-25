//! Agent pane or shell command, decided from the captured `pane get` and
//! `agent get` replies in `tests/fixtures/cli/`.

mod support;

use std::collections::BTreeSet;
use std::path::Path;

use herdr_nudge::classify::{Classification, ClassifySignal, PaneKind, classify};
use herdr_nudge::config::Config;
use herdr_nudge::herdr::{Cli, PaneInfo};
use support::{Recorded, Replay};

const HERDR: &str = "/Users/dev/.local/bin/herdr";

/// The pane as Herdr described it in a capture, through the real parse path.
fn pane_get(fixture: &str, pane_id: &str) -> PaneInfo {
    let recorded = Recorded::cli(fixture);
    let replay = Replay::answering("herdr", &["pane", "get", pane_id], &recorded);
    Cli {
        bin: Path::new(HERDR),
        runner: &replay,
    }
    .pane_get(pane_id)
    .unwrap_or_else(|e| panic!("{fixture}: {e}"))
}

/// `agent get`'s reply read straight from the capture. Nothing in the crate
/// runs `agent get`; this is only here to compare its shape with `pane get`.
fn agent_get(fixture: &str) -> PaneInfo {
    let recorded = Recorded::cli(fixture);
    let body: serde_json::Value =
        serde_json::from_str(&recorded.stdout).unwrap_or_else(|e| panic!("{fixture}: stdout: {e}"));
    serde_json::from_value(body["result"]["agent"].clone())
        .unwrap_or_else(|e| panic!("{fixture}: result.agent: {e}"))
}

/// Herdr's own agent labels, as `server agent-manifests` returned them.
fn manifests() -> BTreeSet<String> {
    let replay = Replay::new([Recorded::cli("agent-manifests")]);
    let cli = Cli {
        bin: Path::new(HERDR),
        runner: &replay,
    };
    cli.agent_manifests().unwrap().into_iter().collect()
}

fn of(pane: &PaneInfo) -> Classification {
    classify(
        &Config::default(),
        &manifests(),
        pane.agent.as_deref(),
        Some(pane.has_agent_session()),
    )
}

#[test]
fn a_pane_claude_owns_is_an_agent() {
    let pane = pane_get("pane-get-focused", "w3:p1");
    assert_eq!(pane.agent.as_deref(), Some("claude"));
    assert_eq!(
        of(&pane),
        Classification {
            kind: PaneKind::Agent,
            signal: ClassifySignal::Catalogue,
        },
        "pane-get-focused"
    );
}

#[test]
fn a_reported_command_is_a_shell() {
    let pane = pane_get("pane-get-reported-no-session", "w3:p2");
    assert_eq!(pane.agent.as_deref(), Some("make"));
    assert!(
        !pane.has_agent_session(),
        "pane-get-reported-no-session: a shell hook has no session to bind"
    );
    assert_eq!(
        of(&pane),
        Classification {
            kind: PaneKind::Shell,
            signal: ClassifySignal::Neither,
        },
        "pane-get-reported-no-session"
    );
}

/// Why classification reads `pane get` and never runs `agent get`: both
/// carry the two signals, and `pane get` also answers for a pane that never
/// had an agent, where `agent get` fails.
///
/// The two captures are of different claims on different panes, so this is
/// about the shape of the reply, not about one moment in time.
#[test]
fn pane_get_and_agent_get_classify_a_reporter_the_same_way() {
    let from_pane = pane_get("pane-get-reported-no-session", "w3:p2");
    let from_agent = agent_get("agent-get-reported-no-session");

    assert!(from_pane.agent.is_some() && from_agent.agent.is_some());
    assert!(!from_pane.has_agent_session() && !from_agent.has_agent_session());
    assert_eq!(of(&from_pane), of(&from_agent));

    let plain = Recorded::cli("agent-get-plain-shell");
    assert!(
        plain.exit_code == 1 && plain.stderr.contains("\"agent_not_found\""),
        "agent-get-plain-shell: agent get has no answer for an unclaimed pane"
    );
    let unclaimed = pane_get("pane-get-unfocused", "w3:p2");
    assert_eq!(
        of(&unclaimed),
        Classification {
            kind: PaneKind::Shell,
            signal: ClassifySignal::Neither,
        },
        "pane-get-unfocused: pane get still answers, and says shell"
    );
}

/// A release takes the agent identity off the pane, so the answer for the
/// pane the command ran in goes back to what a plain pane gets. The title
/// and the state labels stay behind; only the label decides anything.
#[test]
fn a_release_takes_the_agent_label_off_the_pane() {
    let claimed = pane_get("pane-get-reported-no-session", "w3:p2");
    let released = pane_get("pane-get-after-release", "w3:p2");
    assert_eq!(claimed.pane_id, released.pane_id);
    assert_eq!(claimed.agent.as_deref(), Some("make"));
    assert_eq!(
        released.agent, None,
        "pane-get-after-release: release-agent clears the label"
    );
    assert_eq!(
        of(&released),
        Classification {
            kind: PaneKind::Shell,
            signal: ClassifySignal::Neither,
        },
        "pane-get-after-release"
    );
}

#[test]
fn an_agent_herdr_does_not_know_is_found_by_its_session() {
    let pane = pane_get("pane-get-focused", "w3:p1");
    let empty = BTreeSet::new();
    assert_eq!(
        classify(
            &Config::default(),
            &empty,
            pane.agent.as_deref(),
            Some(pane.has_agent_session()),
        ),
        Classification {
            kind: PaneKind::Agent,
            signal: ClassifySignal::AgentSession,
        }
    );
}

#[test]
fn the_catalogue_answers_before_the_session_is_looked_at() {
    // A pane Herdr calls claude, with no session bound to it.
    assert_eq!(
        classify(
            &Config::default(),
            &manifests(),
            Some("claude"),
            Some(false),
        ),
        Classification {
            kind: PaneKind::Agent,
            signal: ClassifySignal::Catalogue,
        }
    );
}

#[test]
fn config_can_force_a_command_to_count_as_an_agent() {
    let config = Config::parse("[shell]\nknown_agents_extra = [\"make\"]\n").unwrap();
    assert_eq!(
        classify(&config, &manifests(), Some("make"), Some(false)),
        Classification {
            kind: PaneKind::Agent,
            signal: ClassifySignal::ConfigOverride,
        }
    );
}

#[test]
fn config_can_force_a_pane_with_a_session_to_count_as_a_shell() {
    let config = Config::parse("[shell]\nknown_agents_remove = [\"claude\"]\n").unwrap();
    let pane = pane_get("pane-get-focused", "w3:p1");
    assert!(pane.has_agent_session());
    assert_eq!(
        classify(&config, &manifests(), pane.agent.as_deref(), Some(true)),
        Classification {
            kind: PaneKind::Shell,
            signal: ClassifySignal::ConfigOverride,
        },
        "known_agents_remove has to beat the session, or it can't force anything"
    );
}

/// With no answer from `pane get`, only the catalogue can still say Agent.
#[test]
fn a_pane_we_could_not_ask_about_is_a_shell_unless_catalogued() {
    assert_eq!(
        classify(&Config::default(), &manifests(), Some("claude"), None),
        Classification {
            kind: PaneKind::Agent,
            signal: ClassifySignal::Catalogue,
        }
    );
    assert_eq!(
        classify(&Config::default(), &manifests(), Some("make"), None),
        Classification {
            kind: PaneKind::Shell,
            signal: ClassifySignal::Unavailable,
        }
    );
    assert_eq!(
        classify(&Config::default(), &manifests(), None, None),
        Classification {
            kind: PaneKind::Shell,
            signal: ClassifySignal::Unavailable,
        }
    );
}

/// `classify` applies `known_agents_extra` and `known_agents_remove` itself
/// instead of taking the merged catalogue, so it has to agree with the
/// function that writes the same lists into `shell.env`.
#[test]
fn the_config_lists_decide_the_same_labels_as_the_shared_catalogue() {
    let config = Config::parse(
        "[shell]\nknown_agents_extra = [\"aider\", \"both\"]\nknown_agents_remove = [\"pi\", \"both\", \"mastracode\"]\n",
    )
    .unwrap();
    let manifests = manifests();
    let catalogue = config.agent_catalogue(manifests.iter().map(String::as_str));

    let mut labels: BTreeSet<&str> = manifests.iter().map(String::as_str).collect();
    labels.extend(["aider", "both", "pi", "make", "sudo", "omp", "mastracode"]);
    for label in labels {
        // No session, so only the labels can decide.
        let kind = classify(&config, &manifests, Some(label), Some(false)).kind;
        assert_eq!(
            kind == PaneKind::Agent,
            catalogue.contains(label),
            "{label}: classify and agent_catalogue disagree"
        );
    }
}

/// Herdr 0.9.0 leaves `agent_session` out rather than sending null. If that
/// ever changes, null still has to mean no session.
#[test]
fn an_explicit_null_session_is_no_session() {
    let mut recorded = Recorded::cli("pane-get-focused");
    let mut body: serde_json::Value = serde_json::from_str(&recorded.stdout).unwrap();
    body["result"]["pane"]["agent_session"] = serde_json::Value::Null;
    recorded.stdout = body.to_string();

    let replay = Replay::answering("herdr", &["pane", "get", "w3:p1"], &recorded);
    let pane = Cli {
        bin: Path::new(HERDR),
        runner: &replay,
    }
    .pane_get("w3:p1")
    .unwrap();

    assert!(!pane.has_agent_session());
    assert_eq!(
        classify(
            &Config::default(),
            &BTreeSet::new(),
            pane.agent.as_deref(),
            Some(pane.has_agent_session()),
        ),
        Classification {
            kind: PaneKind::Shell,
            signal: ClassifySignal::Neither,
        }
    );
}

/// Herdr integrates `omp` and `mastracode` but has no manifest for either,
/// so `agent-manifests` never names them. They count as catalogued anyway,
/// and `known_agents_remove` still overrides that.
#[test]
fn agents_herdr_integrates_without_a_manifest_are_agents() {
    let manifests = manifests();
    assert!(
        !manifests.contains("omp") && !manifests.contains("mastracode"),
        "agent-manifests now names them; the built-in list can go"
    );
    for label in herdr_nudge::herdr::AGENTS_WITHOUT_MANIFEST {
        let got = classify(&Config::default(), &manifests, Some(label), Some(false));
        assert_eq!(got.kind, PaneKind::Agent, "{label}");
        assert_eq!(got.signal, ClassifySignal::Catalogue, "{label}");
    }

    let config = Config::parse("[shell]\nknown_agents_remove = [\"omp\"]\n").unwrap();
    let got = classify(&config, &manifests, Some("omp"), Some(false));
    assert_eq!(got.kind, PaneKind::Shell, "known_agents_remove = [omp]");
}
