//! The dependency fence (§11): `gate-state`, `gate-graph`, `gate-ontology`,
//! `gate-tui` are forbidden in `[dependencies]` AND `[dev-dependencies]`,
//! transitively — "the fence that keeps Rein a harness and Gate the knowledge
//! layer". A CI test, not a convention.
//!
//! Cross-product integration goes through the installed `gate` *binary*
//! (`rein propose to-gate`, M3) with a native wire-format mirror — the same
//! way an outside consumer would arrive. No gate-* crate appears in the
//! resolved graph at all: the build is standalone.

use std::process::Command;

const FORBIDDEN: [&str; 4] = ["gate-state", "gate-graph", "gate-ontology", "gate-tui"];

#[test]
fn forbidden_gate_crates_are_absent_from_the_resolved_graph() {
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml");
    let out = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--manifest-path",
            manifest,
        ])
        .output()
        .expect("cargo metadata runs");
    assert!(
        out.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).expect("metadata is JSON");

    // The resolved package set covers dependencies and dev-dependencies,
    // transitively — exactly the surface the fence forbids.
    let packages = meta["packages"].as_array().expect("packages array");
    let mut names: Vec<&str> = packages.iter().filter_map(|p| p["name"].as_str()).collect();
    names.sort_unstable();

    for forbidden in FORBIDDEN {
        assert!(
            !names.contains(&forbidden),
            "dependency fence breached: `{forbidden}` is in the resolved graph.\n\
             The knowledge layer is Gate's; Rein reaches it only through the \
             installed `gate` binary. Resolved packages: {names:?}"
        );
    }
}
