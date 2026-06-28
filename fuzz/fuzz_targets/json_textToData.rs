//! Fuzz target: compiling `json.textToData("<arbitrary>")` must not panic.
//!
//! The runtime JSON parser lives in generated WASM and is not directly
//! reachable from Rust. The Rust-side surface that matters is the lexer +
//! parser handling of an arbitrary string literal embedded inside the source
//! we hand to users (LSP completions, MCP `check`, etc). This target ensures
//! that no crafted JSON-shaped string literal can wedge the front-end before
//! the WASM runtime ever runs.

#![no_main]

use libfuzzer_sys::fuzz_target;

use clean_language_compiler::parser::CleanParser;

fuzz_target!(|data: &[u8]| {
    let Ok(raw) = std::str::from_utf8(data) else {
        return;
    };

    // Escape so the random bytes embed inside a Clean string literal without
    // immediately terminating it. We deliberately do NOT escape every metachar
    // — the point is to push odd-but-valid string contents at the lexer.
    let mut escaped = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            c if (c as u32) < 0x20 => continue, // drop other controls
            c => escaped.push(c),
        }
    }

    let source = format!(
        "start:\n\tany v = json.textToData(\"{escaped}\")\n\tprint(json.dataToText(v))\n"
    );

    let _ = CleanParser::parse_program(&source);
});
