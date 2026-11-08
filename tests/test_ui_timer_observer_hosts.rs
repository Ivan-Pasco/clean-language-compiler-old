//! `ui.setTimeout` and `ui.observeVisible` are callable on server builds.
//!
//! A page whose components hydrate in the browser is compiled together with
//! the server module, so a component that calls these from `onMount()` must
//! compile for the server host as well; clean-server provides no-ops.

#[test]
fn ui_timer_and_observer_bridges_allow_the_server_host() {
    let registry: toml::Value =
        toml::from_str(include_str!("../src/plugins/function-registry.toml")).unwrap();
    let entries = registry["functions"].as_array().expect("functions table");
    for name in ["_ui_set_timeout", "_ui_observe_visible"] {
        let entry = entries
            .iter()
            .find(|f| f["name"].as_str() == Some(name))
            .unwrap_or_else(|| panic!("{name} missing from the registry"));
        let hosts: Vec<&str> = entry["hosts"]
            .as_array()
            .expect("hosts")
            .iter()
            .filter_map(|h| h.as_str())
            .collect();
        assert!(hosts.contains(&"server"), "{name} hosts: {hosts:?}");
        assert!(hosts.contains(&"browser"), "{name} hosts: {hosts:?}");
    }
}
