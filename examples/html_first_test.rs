//! Test HTML-first compilation
//!
//! Run with: cargo run --example html_first_test
//!
//! This test demonstrates the HTML-first compilation feature which:
//! - Scans for `.html` pages, pairing with optional companion `.cln` files
//! - Companion files provide guard() and load() server-side functions
//! - Builds a component registry from `.cln` component files
//! - Generates Clean code from HTML pages
//! - Compiles to WebAssembly

use clean_language_compiler::{
    build_component_registry, compile_html_first, discover_html_pages, HtmlFirstConfig,
};
use std::path::PathBuf;

fn main() {
    // Initialize tracing for debug output
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let project_dir = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "../clean-framework/examples/html-simple".to_string()),
    );

    println!(
        "Testing HTML-first compilation on: {}",
        project_dir.display()
    );

    // Step 1: Test page discovery
    println!("\n=== Step 1: Discovering HTML pages ===");
    let pages_dir = project_dir.join("app/pages");
    match discover_html_pages(&pages_dir) {
        Ok(pages) => {
            println!("Found {} pages:", pages.len());
            for page in &pages {
                println!("  - {} -> {}", page.file_path.display(), page.route_path);
                if let Some(title) = &page.metadata.title {
                    println!("    Title: {}", title);
                }
                if let Some(layout) = &page.metadata.layout {
                    println!("    Layout: {}", layout);
                }
                if !page.metadata.custom_tags.is_empty() {
                    println!("    Custom tags: {:?}", page.metadata.custom_tags);
                }
                if let Some(companion) = &page.companion {
                    println!(
                        "    Companion: {} (guard={}, load={})",
                        companion.module_name, companion.has_guard, companion.has_load
                    );
                }
            }
        }
        Err(errors) => {
            eprintln!("Failed to discover pages:");
            for err in errors {
                eprintln!("  - {:?}", err);
            }
            return;
        }
    }

    // Step 2: Test component registry
    println!("\n=== Step 2: Building component registry ===");
    let components_dir = project_dir.join("app/components");
    match build_component_registry(&components_dir) {
        Ok(registry) => {
            println!("Found {} components:", registry.components.len());
            for (tag, info) in &registry.components {
                println!(
                    "  - <{}> -> {} (props: {:?})",
                    tag, info.class_name, info.props
                );
            }
        }
        Err(errors) => {
            eprintln!("Failed to build component registry:");
            for err in errors {
                eprintln!("  - {:?}", err);
            }
        }
    }

    // Step 3: Full compilation
    println!("\n=== Step 3: Full HTML-first compilation ===");
    let config = HtmlFirstConfig {
        pages_dir: PathBuf::from("app/pages"),
        components_dir: PathBuf::from("app/components"),
        search_paths: vec![project_dir.clone()],
        opt_level: 0,
    };

    match compile_html_first(&project_dir, config) {
        Ok(wasm_bytes) => {
            println!("Compilation successful!");
            println!("Generated {} bytes of WebAssembly", wasm_bytes.len());

            // Optionally save the output
            let output_path = project_dir.join("app.wasm");
            if let Err(e) = std::fs::write(&output_path, &wasm_bytes) {
                eprintln!("Failed to write output: {}", e);
            } else {
                println!("Wrote output to: {}", output_path.display());
            }
        }
        Err(errors) => {
            eprintln!("Compilation failed:");
            for err in errors {
                eprintln!("  - {:?}", err);
            }
        }
    }
}
