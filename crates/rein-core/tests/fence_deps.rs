//! The dependency fence (§11): every package in the resolved graph must come
//! from a registry — no path or git dependencies outside this workspace, in
//! `[dependencies]` AND `[dev-dependencies]`, transitively. This is the fence
//! that keeps Rein standalone: a fresh clone resolves and builds with no
//! sibling checkout present. A CI test, not a convention.

use std::process::Command;

const WORKSPACE: [&str; 4] = ["rein-core", "rein-runtime", "rein-finance", "rein"];

#[test]
fn resolved_graph_is_workspace_plus_registry_only() {
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
    // transitively — exactly the surface the fence guards.
    let packages = meta["packages"].as_array().expect("packages array");
    let mut members_seen = Vec::new();
    for p in packages {
        let name = p["name"].as_str().expect("package name");
        match p["source"].as_str() {
            // A null source is a local package: it must be one of ours.
            None => {
                assert!(
                    WORKSPACE.contains(&name),
                    "dependency fence breached: `{name}` resolves from a local \
                     path but is not a workspace member — a sibling checkout \
                     has leaked into the graph"
                );
                members_seen.push(name);
            }
            Some(src) => assert!(
                src.starts_with("registry+"),
                "dependency fence breached: `{name}` resolves from `{src}`, \
                 not a registry — the build must stand alone"
            ),
        }
    }
    members_seen.sort_unstable();
    let mut expected = WORKSPACE;
    expected.sort_unstable();
    assert_eq!(members_seen, expected, "every workspace member is resolved");
}
