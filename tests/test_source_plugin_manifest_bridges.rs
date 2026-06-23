//! Regression test for PLUGIN-BUILD-STATE-CROSS-PLUGIN-BOOTSTRAP-BLOCKED
//! (dashboard fingerprint `c7d3cefee5c3`), also resolves
//! PLUGIN-BRIDGE-RESOLVE-USES-INSTALLED-NOT-SOURCE (same root cause,
//! re-filed 52 min after the original fix shipped).
//!
//! Background: `~/.cleen/plugins/<name>/plugin.toml` is the canonical
//! source of bridge declarations the resolver checks against — so a
//! plugin author who adds a new `[bridge].functions` entry and
//! immediately calls it from `src/main.cln` is stuck. The new bridge
//! is in the SOURCE manifest, but the resolver only sees the
//! INSTALLED manifest's bridge set, so the call SEM007s before the
//! plugin can be rebuilt. The framework reporter hit this trying to
//! land cross-plugin `build_state` coordination in a single commit
//! (78c8f54f5b53, ROUTES-PATH-GUARDS-NOT-ENFORCED) — frame.server
//! needed to declare _build_state_set/_build_state_get and immediately
//! use them, but build.sh failed at the compile step.
//!
//! Fix: when the entry being compiled lives under a directory that
//! contains a `plugin.toml` (or is one `src/` level below such a
//! directory, matching the framework's plugin source convention),
//! parse THAT manifest and add its bridge declarations to the
//! resolver's bridge_functions set. Installed-registry bridges still
//! take precedence — a name already present from the installed plugin
//! is not duplicated — but new declarations from the source manifest
//! become visible at this build.
//!
//! This test exercises the framework's canonical plugin source
//! layout (`<plugin_dir>/src/main.cln` with `<plugin_dir>/plugin.toml`)
//! and a bridge name that is intentionally NOT in any
//! `~/.cleen/plugins/` install, so the test would SEM007 pre-fix and
//! must compile clean post-fix.

use std::fs;
use tempfile::TempDir;

#[test]
fn source_plugin_manifest_bridge_resolves_from_neighbouring_plugin_toml() {
    let tmp = TempDir::new().expect("tempdir");
    let plugin_dir = tmp.path().join("test.bootstrap");
    let src_dir = plugin_dir.join("src");
    fs::create_dir_all(&src_dir).expect("mkdir src");

    // A new bridge declared ONLY in the source manifest. The name is
    // chosen so it is not present in any installed plugin under
    // ~/.cleen/plugins — the resolver MUST pick it up from
    // plugin_dir/plugin.toml or SEM007 fires.
    let manifest = "\
[plugin]\n\
name = \"test.bootstrap\"\n\
version = \"0.1.0\"\n\
description = \"Regression repro for c7d3cefee5c3\"\n\
author = \"test\"\n\
\n\
[handles]\n\
blocks = []\n\
\n\
[bridge]\n\
functions = [\n\
  { name = \"_pluginbootstrap_test_new_bridge\", params = [\"string\"], returns = \"string\", hosts = [\"all\"] },\n\
]\n\
";
    fs::write(plugin_dir.join("plugin.toml"), manifest).expect("write plugin.toml");

    // Source that immediately calls the new bridge. Resolution must
    // reach the source manifest to find it.
    let source = "\
functions:\n\
\tstring call_bridge(string key)\n\
\t\treturn _pluginbootstrap_test_new_bridge(key)\n\
";
    let entry = src_dir.join("main.cln");
    fs::write(&entry, source).expect("write main.cln");

    let result = clean_language_compiler::compile_multi_file_with_memory_tier(
        &entry,
        vec![src_dir.clone()],
        2,
        None,
        clean_language_compiler::MemoryTier::Plugin,
        false,
    );

    match result {
        Ok(_) => {
            // expected — the source manifest's bridge declaration was
            // picked up and the call resolved cleanly.
        }
        Err(errors) => {
            let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
            panic!(
                "compile must succeed once the source plugin.toml is parsed for \
                 its bridge declarations. Pre-fix this failed with SEM007 because \
                 resolution only consulted the installed registry (which has no \
                 _pluginbootstrap_test_new_bridge). Errors:\n{}",
                msgs.join("\n")
            );
        }
    }
}

#[test]
fn source_plugin_manifest_in_src_grandparent_is_also_picked_up() {
    // Sanity: the helper checks both `<entry>.parent()/plugin.toml`
    // (top-level layout) and `<entry>.parent().parent()/plugin.toml`
    // when the immediate parent is named `src`. This test exercises the
    // grandparent path explicitly — it is the framework's canonical
    // layout, so most plugin source compiles end up here.
    //
    // The previous test happens to live under `src/main.cln` already,
    // so it implicitly covers this branch; this test re-asserts the
    // contract with a different bridge name so a regression in the
    // candidate-list ordering surfaces here independently.
    let tmp = TempDir::new().expect("tempdir");
    let plugin_dir = tmp.path().join("test.bootstrap2");
    let src_dir = plugin_dir.join("src");
    fs::create_dir_all(&src_dir).expect("mkdir src");

    let manifest = "\
[plugin]\n\
name = \"test.bootstrap2\"\n\
version = \"0.1.0\"\n\
description = \"Regression repro for c7d3cefee5c3 (grandparent layout)\"\n\
author = \"test\"\n\
\n\
[handles]\n\
blocks = []\n\
\n\
[bridge]\n\
functions = [\n\
  { name = \"_pluginbootstrap_test_grandparent_bridge\", params = [], returns = \"void\", hosts = [\"all\"] },\n\
]\n\
";
    fs::write(plugin_dir.join("plugin.toml"), manifest).expect("write plugin.toml");

    let source = "\
functions:\n\
\tvoid invoke_bridge()\n\
\t\t_pluginbootstrap_test_grandparent_bridge()\n\
";
    let entry = src_dir.join("main.cln");
    fs::write(&entry, source).expect("write main.cln");

    let result = clean_language_compiler::compile_multi_file_with_memory_tier(
        &entry,
        vec![src_dir.clone()],
        2,
        None,
        clean_language_compiler::MemoryTier::Plugin,
        false,
    );

    if let Err(errors) = result {
        let msgs: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        panic!(
            "compile must succeed when the source plugin.toml lives at \
             `<entry>.parent().parent()/plugin.toml` (the framework's \
             standard plugin source layout). Errors:\n{}",
            msgs.join("\n")
        );
    }
}
