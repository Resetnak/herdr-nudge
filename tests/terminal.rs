//! Which terminal a click brings forward, and reading the frontmost app from
//! `lsappinfo`.

mod support;

use herdr_nudge::config::Config;
use herdr_nudge::terminal::{
    Resolution, TerminalSource, frontmost_bundle_id, parse_bundle_id, resolve,
};
use support::{Recorded, Replay};

const GHOSTTY: &str = "com.mitchellh.ghostty";
const ITERM: &str = "com.googlecode.iterm2";

fn resolved(bundle_id: &str, source: TerminalSource) -> Resolution {
    Resolution {
        bundle_id: Some(bundle_id.to_owned()),
        source,
    }
}

#[test]
fn resolution_order_is_config_then_server_env() {
    let with_default = Config::parse(&format!("default_terminal = \"{ITERM}\"\n")).unwrap();
    let without = Config::default();

    assert_eq!(
        resolve(&with_default, Some(GHOSTTY)),
        resolved(ITERM, TerminalSource::DefaultConfig),
        "default_terminal beats the server env"
    );
    assert_eq!(
        resolve(&without, Some(GHOSTTY)),
        resolved(GHOSTTY, TerminalSource::ServerEnv),
        "server env when the config names none"
    );
    let unresolved = Resolution {
        bundle_id: None,
        source: TerminalSource::Unresolved,
    };
    assert_eq!(resolve(&without, None), unresolved, "neither");
}

/// Recorded `lsappinfo front` then `info`, the two calls the visibility
/// check makes.
#[test]
fn the_frontmost_app_comes_from_recorded_lsappinfo() {
    let lsappinfo = Replay::new([
        Recorded::sys("lsappinfo-front"),
        Recorded::sys("lsappinfo-bundleid-ghostty"),
    ]);
    assert_eq!(frontmost_bundle_id(&lsappinfo).as_deref(), Some(GHOSTTY));
    assert_eq!(
        *lsappinfo.calls.borrow(),
        [
            vec!["/usr/bin/lsappinfo", "front"],
            vec![
                "/usr/bin/lsappinfo",
                "info",
                "-only",
                "bundleid",
                "ASN:0x0-0x64b64b:"
            ]
        ]
    );
}

#[test]
fn bundle_id_parsing() {
    assert_eq!(
        parse_bundle_id(&Recorded::sys("lsappinfo-bundleid-ghostty").stdout).as_deref(),
        Some(GHOSTTY)
    );
    assert_eq!(
        parse_bundle_id(&Recorded::sys("lsappinfo-bundleid-gone").stdout),
        None,
        "lsappinfo-bundleid-gone: [ NULL ]"
    );
    assert_eq!(parse_bundle_id(""), None);
}

#[test]
fn a_front_reply_that_is_not_an_asn_asks_nothing_more() {
    let mut front = Recorded::sys("lsappinfo-front");
    front.stdout = String::new();
    let replay = Replay::new([front]);
    assert_eq!(frontmost_bundle_id(&replay), None);
    assert_eq!(replay.call_count(), 1);
}
