//! Regression: the browser (client-mode) build of a file that declares
//! endpoints failed with CONC002.
//!
//! A client build drops the module's `start:` body — the server code, including
//! the route registrations that mark endpoint handlers as request handlers
//! (plugin-contract.md §3.6). The handler functions themselves stayed in the
//! program, so CONC002 saw `req.*` used "outside a request handler". A
//! function registered as a request handler is server-only code and must not
//! reach the client artifact (bridge-host-classes.md §6, artifacts.md §3).
//!
//! Needs the installed frame.server plugin; skips when it is not available.

use std::fs;
use std::path::PathBuf;

fn frame_server_installed() -> bool {
    std::env::var_os("HOME")
        .map(|home| {
            PathBuf::from(home)
                .join(".cleen/plugins/frame.server/plugin.toml")
                .exists()
        })
        .unwrap_or(false)
}

#[test]
fn client_build_of_file_with_endpoints_omits_handlers() {
    if !frame_server_installed() {
        eprintln!("skipping: frame.server plugin not installed");
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    let entry = tmp.path().join("app.cln");
    fs::write(
        &entry,
        "plugins:\n\tframe.server\n\nendpoints:\n\tGET \"/hello\":\n\t\tstring who = req.query(\"who\")\n\t\treturn http.respond(200, \"text/plain\", \"hi \" + who)\n\nstart:\n\tprintl(\"boot\")\n",
    )
    .unwrap();

    let result = clean_language_compiler::compile_multi_file_client_mode(
        &entry,
        vec![tmp.path().to_path_buf()],
        0,
    );
    let bytes = match result {
        Ok(bytes) => bytes,
        Err(errors) => panic!(
            "client build must not see the server-only endpoint handler: {:?}",
            errors.iter().map(|e| e.to_string()).collect::<Vec<_>>()
        ),
    };

    // No request-context bridge may reach the browser module.
    let mut imports = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(&bytes) {
        if let Ok(wasmparser::Payload::ImportSection(reader)) = payload {
            for import in reader.into_iter().flatten() {
                imports.push(import.name.to_string());
            }
        }
    }
    assert!(
        !imports.iter().any(|name| name.starts_with("_req_")),
        "client build imports request bridges: {:?}",
        imports
    );
}
