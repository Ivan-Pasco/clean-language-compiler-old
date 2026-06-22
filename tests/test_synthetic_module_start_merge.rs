//! Reproducer for COMPILER-SYNTHETIC-MODULE-START-BLOCK-DROPPED
//! (dashboard fingerprint `4e740d00a254`).
//!
//! Asserts that the `start:` body inside a synthetic module returned by a
//! plugin's `assemble` hook via `AssembleOutput.injected_sources` is
//! merged into the program-level `_start` function so its side-effecting
//! calls actually execute at startup. Pre-fix the body was silently
//! dropped.

use std::sync::Arc;

use clean_language_compiler::ast::Statement;
use clean_language_compiler::compilation::{MultiFileCompiler, MultiFileCompilerConfig};
use clean_language_compiler::plugins::plugin_abi::{AssembleInput, AssembleOutput, InjectedSource};
use clean_language_compiler::plugins::{
    FrameworkBlock, FrameworkPlugin, PluginRegistry, PluginResult,
};

/// Mock plugin whose `assemble` injects one synthetic module that
/// contains a `start:` block with a side-effecting call plus a
/// `functions:` block. Both must survive into the merged HIR.
struct InjectingPlugin;

impl FrameworkPlugin for InjectingPlugin {
    fn name(&self) -> &'static str {
        "test.injector"
    }

    fn handles(&self) -> &'static [&'static str] {
        &["__test_injector_block__"]
    }

    fn expand(&self, _block: &FrameworkBlock) -> PluginResult<Vec<Statement>> {
        Ok(Vec::new())
    }

    fn assemble(&self, _input: &AssembleInput) -> PluginResult<AssembleOutput> {
        // Inject TWO synthetic modules, mirroring frame.ui's real shape
        // (__page_routes_generated for route registration + a diagnostic
        // module). Both must have their start: bodies merged.
        let mut out = AssembleOutput::default();
        out.injected_sources.push(InjectedSource {
            virtual_path: "__page_routes_generated.cln".to_string(),
            content: concat!(
                "start:\n",
                "\tprintl(\"routes-start-fired\")\n",
                "\n",
                "functions:\n",
                "\tstring _routes_init()\n",
                "\t\treturn \"routes\"\n",
            )
            .to_string(),
        });
        out.injected_sources.push(InjectedSource {
            virtual_path: "__plugin_diagnostic.cln".to_string(),
            content: concat!(
                "start:\n",
                "\tprintl(\"injected-start-fired\")\n",
                "\n",
                "functions:\n",
                "\tstring _plugin_diag()\n",
                "\t\treturn \"ok\"\n",
            )
            .to_string(),
        });
        Ok(out)
    }
}

#[test]
fn synthetic_module_start_block_is_merged_into_program_start() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    // Manifest with a target so manifest_info is built and assemble runs.
    let manifest_source = concat!(
        "package: TestApp\n",
        "\tversion: \"1.0.0\"\n",
        "\ttarget: web\n",
        "\t\tplugins: [test.injector]\n",
        "\t\tentry: app/pages/hello.cln\n",
    );
    std::fs::write(root.join("main.cln"), manifest_source).expect("write manifest");

    let pages_dir = root.join("app").join("pages");
    std::fs::create_dir_all(&pages_dir).expect("mkdir app/pages");

    // Entry .cln with a distinctive marker in its own start:. The merge
    // result must contain BOTH the entry's marker AND the synthetic
    // module's `injected-start-fired` printl.
    std::fs::write(
        pages_dir.join("hello.cln"),
        "start:\n\tprintl(\"entry-start-fired\")\n",
    )
    .expect("write entry");

    let mut registry = PluginRegistry::new();
    #[allow(deprecated)]
    registry
        .register(Arc::new(InjectingPlugin))
        .expect("register mock plugin");

    let config = MultiFileCompilerConfig::default().with_plugin_registry(Arc::new(registry));
    let compiler = MultiFileCompiler::with_config(config);

    let unit = compiler
        .build_from_file(root.join("main.cln"))
        .expect("build_from_file should succeed — synthetic module has valid Clean source");

    // Both synthetic modules must have been added — distinct paths,
    // distinct unique names.
    let has_routes = unit.modules.values().any(|m| {
        m.file_path
            .to_string_lossy()
            .contains("__page_routes_generated")
    });
    let has_diag = unit.modules.values().any(|m| {
        m.file_path
            .to_string_lossy()
            .contains("__plugin_diagnostic")
    });
    assert!(
        has_routes,
        "__page_routes_generated.cln synthetic module must be registered"
    );
    assert!(
        has_diag,
        "__plugin_diagnostic.cln synthetic module must be registered. Modules present: {:?}",
        unit.modules
            .values()
            .map(|m| (m.name.clone(), m.file_path.display().to_string()))
            .collect::<Vec<_>>()
    );

    // BOTH synthetic modules must have parsed a non-empty start function.
    for needle in ["__page_routes_generated", "__plugin_diagnostic"] {
        let m = unit
            .modules
            .values()
            .find(|m| m.file_path.to_string_lossy().contains(needle))
            .unwrap_or_else(|| panic!("module containing {needle} present"));
        let h = m
            .hir
            .as_ref()
            .unwrap_or_else(|| panic!("HIR built for {needle}"));
        let sf = h
            .start_function
            .as_ref()
            .unwrap_or_else(|| panic!("{needle} parsed start_function"));
        assert!(
            !sf.body.statements.is_empty(),
            "{needle} start_function body must have ≥1 statement"
        );
    }

    // The compile_multi_file_with_memory_tier merger appends every
    // non-entry module's start statements to the entry's. This test
    // operates at the CompilationUnit level (pre-merge) and asserts the
    // synthetic module makes it into that unit with a non-empty parsed
    // start body. The merge itself is exercised by the integration
    // pipeline test below.
}

/// Mirror the HIR-merge loop from `compile_multi_file_with_memory_tier`
/// (lib.rs around line 2273) for non-entry modules so we can assert the
/// synthetic module's `start:` statements actually flow into a single
/// merged `start_function`. If this assertion fails the bug is in the
/// HIR-merge code; if it passes the bug is in some downstream stage
/// (Resolver / TypeChecker / MIR / codegen).
#[test]
fn synthetic_module_start_block_appears_in_merged_start_function() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    let manifest_source = concat!(
        "package: TestApp\n",
        "\tversion: \"1.0.0\"\n",
        "\ttarget: web\n",
        "\t\tplugins: [test.injector]\n",
        "\t\tentry: app/pages/hello.cln\n",
    );
    std::fs::write(root.join("main.cln"), manifest_source).expect("write manifest");

    let pages_dir = root.join("app").join("pages");
    std::fs::create_dir_all(&pages_dir).expect("mkdir app/pages");
    std::fs::write(
        pages_dir.join("hello.cln"),
        "start:\n\tprintl(\"entry-start-fired\")\n",
    )
    .expect("write entry");

    let mut registry = PluginRegistry::new();
    #[allow(deprecated)]
    registry
        .register(Arc::new(InjectingPlugin))
        .expect("register mock plugin");

    let config = MultiFileCompilerConfig::default().with_plugin_registry(Arc::new(registry));
    let compiler = MultiFileCompiler::with_config(config);

    let unit = compiler
        .build_from_file(root.join("main.cln"))
        .expect("build_from_file");

    // Replicate the same accumulation the production pipeline does:
    // walk compilation_order, collect every non-entry module's start
    // statements into `extra_start_stmts`, then attach to the merged
    // start_function.
    let mut start_function: Option<clean_language_compiler::hir::HirFunction> = None;
    let mut extra_start_stmts: Vec<clean_language_compiler::hir::HirStatement> = Vec::new();

    for module_id in &unit.compilation_order {
        let Some(module) = unit.get_module(*module_id) else {
            continue;
        };
        let Some(hir) = &module.hir else {
            continue;
        };
        if module.is_entry {
            start_function = hir.start_function.clone();
        } else if let Some(ref module_start) = hir.start_function {
            extra_start_stmts.extend(module_start.body.statements.iter().cloned());
        }
    }

    if !extra_start_stmts.is_empty() {
        if let Some(ref mut sf) = start_function {
            extra_start_stmts.append(&mut sf.body.statements);
            sf.body.statements = extra_start_stmts;
        }
    }

    let merged = start_function.expect("merged start function must exist");

    // Render the merged statements to a debug string so we can scan
    // for the synthetic module's printl literal. The literal lives
    // inside a HirStatement::Print { expression: HirExpression::String("injected-start-fired") }
    // and Debug recursion walks both fields, so the substring will
    // surface even though we don't inspect the exact node shape.
    let merged_dbg = format!("{:#?}", merged.body.statements);
    let has_entry_call = merged_dbg.contains("entry-start-fired");
    let has_routes_call = merged_dbg.contains("routes-start-fired");
    let has_injected_call = merged_dbg.contains("injected-start-fired");

    assert!(
        has_entry_call,
        "Entry module's printl literal must appear in the merged start function. \
         If even the entry is missing, the test fixture is wrong."
    );
    assert!(
        has_routes_call,
        "First synthetic module's (__page_routes_generated) printl literal must \
         appear in the merged start function. Body:\n{}",
        merged_dbg
    );
    assert!(
        has_injected_call,
        "COMPILER-SYNTHETIC-MODULE-START-BLOCK-DROPPED: second synthetic module's \
         (__plugin_diagnostic) printl literal `injected-start-fired` is missing from \
         the merged start function. When a plugin's assemble hook returns MULTIPLE \
         injected_sources, only the first survives — the start: bodies of subsequent \
         injections are lost. Merged start function body:\n{}",
        merged_dbg
    );
}
