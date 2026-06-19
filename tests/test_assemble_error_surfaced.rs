//! Regression test for COMPILER-ASSEMBLE-ERROR-SWALLOWED
//! (fingerprint `3109748e8b0d46b6267d177698197f9123eab6309390fcd48fba0b7ed4863f9f`,
//! reported against compiler 0.30.316).
//!
//! Background: `PluginRegistry::run_assemble_hooks` previously used
//! `if let Ok(output) = plugin.assemble(input)` and discarded every `Err`.
//! A plugin whose `assemble` export trapped, returned invalid JSON, or
//! failed for any other reason produced a silently successful build with
//! empty `injected_sources` / `transformed_sources` and no diagnostic of
//! any kind — `cln compile` exited 0 with `Successfully compiled` and no
//! `pages_*_render` exports in the wasm. The end-user symptom was a
//! deployment that loaded fine but returned HTTP 200 + empty body for
//! every page that depended on the assembled output.
//!
//! Fix: `run_assemble_hooks` now returns `(AssembleOutput, Vec<(name, err)>)`.
//! The caller in `multi_file_compiler::build_from_file` converts every
//! plugin error into a `CompilerError::PluginError` and returns it from
//! the build. Successful plugins still contribute their work so a single
//! broken plugin does not erase the rest.
//!
//! This unit test exercises `run_assemble_hooks` directly with a mock
//! plugin that always fails. Pre-fix the call returned an empty
//! `AssembleOutput` and no error signal. Post-fix the call returns the
//! errors vec carrying the failure.

use std::sync::Arc;

use clean_language_compiler::ast::Statement;
use clean_language_compiler::plugins::plugin_abi::{AssembleInput, AssembleOutput};
use clean_language_compiler::plugins::{
    FrameworkBlock, FrameworkPlugin, PluginError, PluginRegistry, PluginResult,
};

/// Mock plugin that always fails on `assemble`. All other methods are
/// trait defaults except the required `name`/`handles`/`expand`.
struct AlwaysFailingAssemblePlugin;

impl FrameworkPlugin for AlwaysFailingAssemblePlugin {
    fn name(&self) -> &'static str {
        "test.failing"
    }

    fn handles(&self) -> &'static [&'static str] {
        &["test_block"]
    }

    fn expand(&self, _block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
        Ok(Vec::new())
    }

    fn assemble(&self, _input: &AssembleInput) -> PluginResult<AssembleOutput> {
        Err(PluginError::ExpansionFailed {
            plugin_name: self.name().to_string(),
            block_name: "assemble".to_string(),
            message: "synthetic failure for COMPILER-ASSEMBLE-ERROR-SWALLOWED regression test"
                .to_string(),
            location: None,
        })
    }
}

#[test]
fn run_assemble_hooks_surfaces_plugin_errors() {
    let mut registry = PluginRegistry::new();
    #[allow(deprecated)] // pre-builder API is the simplest path for mocking
    registry
        .register(Arc::new(AlwaysFailingAssemblePlugin))
        .expect("register mock plugin");

    let input = AssembleInput {
        source_files: Vec::new(),
        project_root: "/tmp/repro".to_string(),
        manifest_dir: "/tmp/repro".to_string(),
        has_frame_server: false,
    };

    let (output, errors) = registry.run_assemble_hooks(&input);

    assert!(
        output.injected_sources.is_empty(),
        "failing plugin produced no successful output"
    );
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one assemble error to be surfaced; got {errors:?}"
    );
    let (plugin_name, err) = &errors[0];
    assert_eq!(plugin_name, "test.failing");
    assert!(
        format!("{err}").contains("synthetic failure"),
        "error message should preserve the plugin's original detail; got `{err}`"
    );
}
