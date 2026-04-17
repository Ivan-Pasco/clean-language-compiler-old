//! Post-codegen WASM validation with structured diagnostics.
//!
//! Runs `wasmparser::Validator` over the freshly-generated module and,
//! if validation fails, reports *which section the bad offset falls in*
//! plus a short hex window around it. Catches codegen bugs before the
//! bytes ever reach disk — avoids generic "failed to parse" errors
//! downstream in the server/runtime.
//!
//! Before 0.30.62 the compiler wrote whatever `MirCodeGenerator` produced
//! to disk unconditionally; invalid modules only surfaced when the server
//! (or wasmtime) tried to parse them, with no indication of where the
//! problem was.

use crate::error::CompilerError;
use wasmparser::{Parser, Payload, Validator};

/// Validate generated WASM bytes. Returns a `CompilerError::Codegen` with
/// a rich diagnostic (section, offset, surrounding bytes) if the module
/// is malformed.
pub fn validate_generated_wasm(bytes: &[u8]) -> Result<(), CompilerError> {
    let mut validator = Validator::new();
    if let Err(err) = validator.validate_all(bytes) {
        let offset = err.offset();
        let section = locate_section(bytes, offset);
        let context = hex_window(bytes, offset, 8);

        let msg = format!(
            "generated WebAssembly is invalid: {}; offset=0x{:x} ({}), section={}, bytes near offset=[{}]",
            err.message(),
            offset,
            offset,
            section,
            context,
        );

        return Err(CompilerError::codegen_error(
            msg,
            Some(
                "This is a compiler bug. The module failed wasmparser validation \
                 before it was written to disk. Please report via `report_error` \
                 (MCP) including the offset and section from this message."
                    .to_string(),
            ),
            None,
        ));
    }
    Ok(())
}

/// Walk the top-level section headers and return the name of the section
/// whose payload range covers `target_offset`. Falls back to "header" for
/// offsets before the first section and "unknown" if iteration fails.
fn locate_section(bytes: &[u8], target_offset: usize) -> String {
    if target_offset < 8 {
        return "module header".to_string();
    }

    let mut last: String = "module header".to_string();
    for payload in Parser::new(0).parse_all(bytes) {
        let payload = match payload {
            Ok(p) => p,
            Err(_) => break,
        };

        let (name, range) = match &payload {
            Payload::Version { range, .. } => ("version", range.clone()),
            Payload::TypeSection(r) => ("type", r.range()),
            Payload::ImportSection(r) => ("import", r.range()),
            Payload::FunctionSection(r) => ("function", r.range()),
            Payload::TableSection(r) => ("table", r.range()),
            Payload::MemorySection(r) => ("memory", r.range()),
            Payload::GlobalSection(r) => ("global", r.range()),
            Payload::ExportSection(r) => ("export", r.range()),
            Payload::StartSection { range, .. } => ("start", range.clone()),
            Payload::ElementSection(r) => ("element", r.range()),
            Payload::CodeSectionStart { range, .. } => ("code", range.clone()),
            Payload::DataSection(r) => ("data", r.range()),
            Payload::CustomSection(c) => {
                let name_owned = format!("custom({})", c.name());
                if c.range().contains(&target_offset) {
                    return name_owned;
                }
                last = name_owned;
                continue;
            }
            Payload::DataCountSection { range, .. } => ("data-count", range.clone()),
            Payload::TagSection(r) => ("tag", r.range()),
            _ => continue,
        };

        if range.contains(&target_offset) {
            return name.to_string();
        }
        last = name.to_string();
    }
    last
}

/// Produce a short hex window of bytes around `offset` for the diagnostic.
/// Keeps the output compact so it's readable inline in error messages.
fn hex_window(bytes: &[u8], offset: usize, radius: usize) -> String {
    let start = offset.saturating_sub(radius);
    let end = (offset + radius).min(bytes.len());
    bytes[start..end]
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_empty_module_passes() {
        // Minimal valid WASM module: header + version
        let bytes = [0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        assert!(validate_generated_wasm(&bytes).is_ok());
    }

    #[test]
    fn invalid_module_reports_offset_and_section() {
        // Bad magic number — first byte wrong
        let bytes = [0xff, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        let err = validate_generated_wasm(&bytes).expect_err("must fail");
        let msg = err.to_string();
        assert!(msg.contains("offset="), "missing offset in: {msg}");
        assert!(msg.contains("section="), "missing section in: {msg}");
    }

    #[test]
    fn truncated_code_section_is_diagnosed() {
        // Valid header + version then garbage that will fail validation
        let bytes = [
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // header
            0x0a, 0xff, 0xff, 0xff, 0xff, 0x0f, // code section with huge size
        ];
        let err = validate_generated_wasm(&bytes).expect_err("must fail");
        assert!(err.to_string().contains("offset="));
    }
}
