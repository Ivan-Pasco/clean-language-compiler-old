//! Fuzz target: the parser must never panic on arbitrary input.
//!
//! Parsing happens in two layers in this compiler: the pest grammar
//! (`CleanParser::parse_program`) and the hand-rolled token parser. We exercise
//! the public entry that downstream tools (LSP, MCP server, `cln check`) use,
//! so a crash here is a crash users can hit.

#![no_main]

use libfuzzer_sys::fuzz_target;

use clean_language_compiler::parser::CleanParser;

fuzz_target!(|data: &[u8]| {
    // The parser only accepts UTF-8; skip non-UTF-8 noise rather than burn
    // cycles re-discovering "not valid UTF-8".
    if let Ok(source) = std::str::from_utf8(data) {
        // We do not care about Ok/Err — only that this returns without
        // panicking or recursing forever. Timeouts surface in libfuzzer's
        // own time-limit handling.
        let _ = CleanParser::parse_program(source);
    }
});
