//! plugin.toml `[aliases]` contract.
//!
//! `[aliases]` names a language call whose bridge no naming convention
//! derives (frame.locale: `locale.current` → `_i18n_locale`). The compiler
//! used to ignore the section, so a call such as `locale.current()` never
//! reached its bridge import and codegen failed with COD000 "Function
//! 'locale.current' not found in function map".
//!
//! The registry is built in-test from a manifest string, so these tests do not
//! depend on installed plugins.

use clean_language_compiler::plugins::registry_loader::ValidationPolicy;
use clean_language_compiler::plugins::{PluginManifest, PluginRegistry};
use wasmparser::{Parser as WasmParser, Payload};

const MANIFEST: &str = r#"
[plugin]
name = "test.gauge"
version = "1.0.0"
description = "Alias contract fixture"
author = "Test"

[handles]
blocks = []

[bridge]
functions = [
  { name = "_gauge_level", params = [], returns = "string" },
  { name = "_gauge_set_level", params = ["string"], returns = "void" },
]

[language]
functions = [
  { name = "meter.reading", signature = "meter.reading() -> string", description = "Current level." },
]

[aliases]
functions = [
  { clean_name = "meter.reading", bridge_name = "_gauge_level" },
  { clean_name = "meter.adjust", bridge_name = "_gauge_set_level" },
  { clean_name = "gauge.level", bridge_name = "_gauge_set_level" },
]
"#;

fn registry() -> PluginRegistry {
    let manifest: PluginManifest = toml::from_str(MANIFEST).expect("fixture manifest parses");
    PluginRegistry::builder()
        .with_validation_policy(ValidationPolicy::Off)
        .add_bridge_functions(&manifest.bridge)
        .add_manifest("test.gauge".to_string(), manifest)
        .build()
        .expect("registry builds")
}

fn import_names(wasm: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    for payload in WasmParser::new(0).parse_all(wasm) {
        if let Ok(Payload::ImportSection(reader)) = payload {
            for import in reader {
                names.push(import.expect("valid import").name.to_string());
            }
        }
    }
    names
}

#[test]
fn aliases_section_maps_language_names_to_bridges() {
    let map = registry().language_to_bridge_map();
    assert_eq!(
        map.get("meter.reading").map(String::as_str),
        Some("_gauge_level")
    );
    assert_eq!(
        map.get("meter.adjust").map(String::as_str),
        Some("_gauge_set_level")
    );
}

#[test]
fn aliases_section_does_not_override_convention_mapping() {
    // `gauge.level` is auto-derived from the `_gauge_level` bridge; an alias
    // entry for the same name only fills gaps, so the derived target stays.
    let map = registry().language_to_bridge_map();
    assert_eq!(
        map.get("gauge.level").map(String::as_str),
        Some("_gauge_level")
    );
}

#[test]
fn zero_arg_alias_call_compiles_to_its_bridge_import() {
    let source = "start:\n\tstring r = meter.reading()\n\tprint(r)\n";
    let wasm = clean_language_compiler::compile_with_plugins(source, "test.cln", &registry())
        .unwrap_or_else(|errors| panic!("compile failed: {errors:?}"));
    let imports = import_names(&wasm);
    assert!(
        imports.iter().any(|i| i == "_gauge_level"),
        "`meter.reading()` must import `_gauge_level`; imports: {imports:?}"
    );
    assert!(
        !imports.iter().any(|i| i == "meter.reading"),
        "the alias name must not become an import; imports: {imports:?}"
    );
}

#[test]
fn alias_call_with_argument_compiles_to_its_bridge_import() {
    let source = "start:\n\tmeter.adjust(\"high\")\n\tprint(\"ok\")\n";
    let wasm = clean_language_compiler::compile_with_plugins(source, "test.cln", &registry())
        .unwrap_or_else(|errors| panic!("compile failed: {errors:?}"));
    let imports = import_names(&wasm);
    assert!(
        imports.iter().any(|i| i == "_gauge_set_level"),
        "`meter.adjust(..)` must import `_gauge_set_level`; imports: {imports:?}"
    );
}
