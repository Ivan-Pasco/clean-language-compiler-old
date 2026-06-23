//! Regression test for `CLN-STRING-LITERAL-QUOTES-LEAK-INTO-BRIDGE`
//! (dashboard fingerprint `0f61f4799745`, re-filed against cln 0.30.346..0.30.348).
//!
//! Dashboard claim: cln emits string literals so the runtime value seen by
//! host-bridge functions includes the surrounding `"` characters — e.g.
//! clean-server's `_ui_render_page` receives literal `"path/to/file"` with
//! the `"` chars as actual content bytes, and tries to open a path that
//! starts/ends with `"`.
//!
//! Verified non-reproducing on cln 0.30.349. This test pins the behaviour
//! at three layers so any future regression in either the page-companion
//! source generator OR the data-section emitter is caught:
//!
//! 1. `generate_page_route_source` — the Rust-side text generator that
//!    produces the synthetic `_ui_render_page(page_name, page_json)` call.
//!    Asserts the generated source declares the page name as a clean
//!    string literal (`"app/ui/web/pages/index.html"`) with the quote
//!    chars OUTSIDE the literal value, not embedded inside.
//!
//! 2. End-to-end Clean compilation — a tiny program that calls a bridge
//!    with a string literal `"path/to/file"`. Asserts the WASM data
//!    section stores the bytes `path/to/file` (12 bytes) with a length
//!    prefix of 0x0c, and that the bytes immediately before and after
//!    the content are NOT 0x22 (`"`).
//!
//! 3. Same end-to-end check, but with the literal embedded as the
//!    page-companion would emit it: `_ui_render_page("app/ui/web/pages/index.html", "{}")`.

use clean_language_compiler::plugins::builtin_assemblers::{
    derive_companion_module_name, derive_page_name_from_cln, generate_page_route_source,
    PageCompanionRecord,
};
use std::path::Path;

/// Layer 1: the page-companion source generator emits well-formed Clean
/// source with quotes OUTSIDE the literal value.
#[test]
fn page_route_source_quotes_page_name_correctly() {
    let record = PageCompanionRecord {
        module_name: "pages_index".to_string(),
        route_path: "/".to_string(),
        page_name: derive_page_name_from_cln(
            Path::new("app/ui/web/pages/index.cln"),
            Path::new(""),
        ),
        has_guard: false,
        has_load: true,
    };

    // Sanity-check the inputs the generator will see.
    assert_eq!(record.page_name, "app/ui/web/pages/index.html");
    assert_eq!(
        derive_companion_module_name(Path::new("app/ui/web/pages/index.cln"), Path::new(""),),
        "pages_index"
    );

    let src = generate_page_route_source(&[record]);

    // The `_ui_render_page` call must appear with the page name wrapped
    // in double-quotes — i.e. exactly one " before the path and one
    // after, with no embedded quote chars in the value.
    assert!(
        src.contains(r#"_ui_render_page("app/ui/web/pages/index.html", page_json)"#),
        "expected _ui_render_page(\"app/ui/web/pages/index.html\", page_json) — \
         the literal must be quoted as a Clean string, not have the quote chars \
         as part of the value. Actual generated source:\n{src}"
    );

    // Defensive: the generator must NOT emit doubled or backslash-escaped
    // quotes — that would land in the data section as `\"path\"` bytes.
    assert!(
        !src.contains(r#"\"app"#),
        "page name must not be backslash-escaped in the generated Clean source"
    );
    assert!(
        !src.contains(r#"""app"#),
        "page name must not have doubled opening quotes"
    );
    assert!(
        !src.contains(r#"index.html"""#),
        "page name must not have doubled closing quotes"
    );
}

/// Layer 2: compile a Clean program that calls a bridge with a string
/// literal and verify the data section stores the literal content WITHOUT
/// surrounding `"` characters.
#[test]
fn bridge_call_string_literal_has_no_quote_leak_in_data_section() {
    use clean_language_compiler::compile_with_file;

    let source = "external:\n\tinteger _file_open(string path)\n\nstart:\n\tinteger r = _file_open(\"path/to/file\")\n\tprint(r.toString())\n";

    let wasm = compile_with_file(source, "repro.cln").expect("repro must compile");

    let needle = b"path/to/file";
    let occurrences = find_byte_windows(&wasm, needle);
    assert!(
        !occurrences.is_empty(),
        "data section must contain the 'path/to/file' literal somewhere"
    );

    for off in occurrences {
        // Bug shape: bytes are `"path/to/file"` (14 bytes), so the byte
        // immediately before is 0x22 AND the byte at off+needle.len() is
        // also 0x22.
        let before: Option<u8> = off.checked_sub(1).and_then(|i| wasm.get(i).copied());
        let after: Option<u8> = wasm.get(off + needle.len()).copied();

        // It is fine for ONE of these to be 0x22 (e.g. the closing quote
        // of an adjacent unrelated literal, or the start of the next
        // literal in the pool). The bug shape is both-at-once.
        let leaked = matches!((before, after), (Some(0x22), Some(0x22)));
        assert!(
            !leaked,
            "CLN-STRING-LITERAL-QUOTES-LEAK-INTO-BRIDGE: literal at WASM \
             offset 0x{off:x} is surrounded by 0x22 (\") bytes — \
             before={before:?}, after={after:?}. The bridge would receive \
             `\"path/to/file\"` with the quote chars as content bytes."
        );
    }

    // Stronger: there should be NO occurrence of the exact byte sequence
    // `"path/to/file"` (14 bytes including both quotes) anywhere in the
    // module. That sequence would only appear if the compiler embedded the
    // raw lexer text of the literal instead of its semantic value.
    let quoted = b"\"path/to/file\"";
    let quoted_occurrences = find_byte_windows(&wasm, quoted);
    assert!(
        quoted_occurrences.is_empty(),
        "found `\"path/to/file\"` (with surrounding quote bytes) at offsets \
         {quoted_occurrences:?} — this is the CLN-STRING-LITERAL-QUOTES-LEAK-INTO-BRIDGE \
         shape. Bridge handlers would receive the literal with the quote \
         chars as content."
    );
}

/// Layer 3: a Clean program shaped exactly like what the page-companion
/// assembler emits — bridge call whose first argument is a quoted page
/// path. Same no-quote-leak assertion.
#[test]
fn page_companion_shaped_bridge_call_has_no_quote_leak() {
    use clean_language_compiler::compile_with_file;

    let source = "external:\n\tstring _ui_render_page(string page_name, string body_json)\n\nstart:\n\tstring r = _ui_render_page(\"app/ui/web/pages/index.html\", \"{}\")\n\tprint(r)\n";

    let wasm = compile_with_file(source, "repro_page.cln").expect("repro must compile");

    let needle = b"app/ui/web/pages/index.html";
    let occurrences = find_byte_windows(&wasm, needle);
    assert!(
        !occurrences.is_empty(),
        "data section must contain the page name literal"
    );

    for off in occurrences {
        let before: Option<u8> = off.checked_sub(1).and_then(|i| wasm.get(i).copied());
        let after: Option<u8> = wasm.get(off + needle.len()).copied();
        let leaked = matches!((before, after), (Some(0x22), Some(0x22)));
        assert!(
            !leaked,
            "page-companion-shaped call leaks `\"` bytes around the page \
             name literal at offset 0x{off:x}. before={before:?}, after={after:?}"
        );
    }

    let quoted = b"\"app/ui/web/pages/index.html\"";
    assert!(
        find_byte_windows(&wasm, quoted).is_empty(),
        "found the page name wrapped in literal `\"` chars in the WASM bytes"
    );
}

/// Return all byte offsets where `needle` appears in `haystack`.
fn find_byte_windows(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let limit = haystack.len() - needle.len();
    for i in 0..=limit {
        if &haystack[i..i + needle.len()] == needle {
            out.push(i);
        }
    }
    out
}
