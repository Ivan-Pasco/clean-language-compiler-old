//! `PluginLifecycle::slot_export` maps every lifecycle slot name to the
//! export the manifest declares for it (lifecycle.md §3). The registry, the
//! WASM adapter and artifact predicates all dispatch through it.

use clean_language_compiler::plugins::plugin_abi::PluginLifecycle;

#[test]
fn slot_export_maps_every_declared_slot() {
    let lifecycle = PluginLifecycle {
        module_helpers: Some("emit_helpers".to_string()),
        program_init: Some("emit_program".to_string()),
        client_init: Some("emit_client".to_string()),
        server_init: Some("emit_server".to_string()),
        ..Default::default()
    };
    assert_eq!(
        lifecycle.slot_export("module_helpers"),
        Some("emit_helpers")
    );
    assert_eq!(lifecycle.slot_export("program_init"), Some("emit_program"));
    assert_eq!(lifecycle.slot_export("client_init"), Some("emit_client"));
    assert_eq!(lifecycle.slot_export("server_init"), Some("emit_server"));
}

#[test]
fn slot_export_is_none_for_undeclared_or_unknown_slots() {
    let lifecycle = PluginLifecycle::default();
    assert_eq!(lifecycle.slot_export("program_init"), None);
    assert_eq!(lifecycle.slot_export("not_a_slot"), None);
    // The `per_request` and `artifact_emitters` slots were removed from the
    // plugin contract (lifecycle.md §3): a plugin naming them gets no export.
    assert_eq!(lifecycle.slot_export("per_request"), None);
    assert_eq!(lifecycle.slot_export("artifact_emitters"), None);
}
