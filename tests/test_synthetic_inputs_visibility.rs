//! Regression test for the visibility-flip bug in synthesized `*Inputs` classes.
//!
//! Discovered 2026-06-26 while migrating Clean Studio to the new
//! private-by-default visibility model (spec change 2026-06-25, compiler
//! enforcement from 0.30.361). The parser synthesizes a class named
//! `<ComponentName>Inputs` from the `inputs:` sub-section of a class body
//! (e.g. inside a frame.ui `component:` block, which lowers to `class X ...
//! inputs: ...`). Pre-fix, the synthesized class's fields used
//! `Field::new` which defaults to `Visibility::Private`. The framework
//! reads those fields externally via `inputs.<field>`, so under the new
//! model every component triggers SEM007 ('field' is private and cannot
//! be accessed from outside '*Inputs').
//!
//! By spec semantics, the `inputs:` block IS the component's public
//! interface — these fields must be public.
//!
//! Reproducer: parse a class with an `inputs:` sub-section and assert the
//! synthesized `<Name>Inputs` class's fields are `Visibility::Public`.

use clean_language_compiler::ast::Visibility;
use clean_language_compiler::parse_to_ast;

#[test]
fn synthesized_inputs_class_fields_are_public() {
    // Minimal reproduction of what frame.ui's expand_component macro
    // emits for `component: tag="flash-alert" inputs: string msg, string type`.
    // The class body contains an `inputs:` sub-section; the parser must
    // (a) synthesize a class named `FlashAlertInputs`, and
    // (b) mark its fields as Public so the framework can access them.
    let source = "\
class FlashAlert
\tinputs:
\t\tstring msg
\t\tstring type
";

    let program = parse_to_ast(source, "test.cln").expect("parse should succeed");

    let inputs_class = program
        .classes
        .iter()
        .find(|c| c.name == "FlashAlertInputs")
        .expect("synthesized FlashAlertInputs class must exist in parsed program");

    assert!(
        !inputs_class.fields.is_empty(),
        "FlashAlertInputs should have fields"
    );

    for field in &inputs_class.fields {
        assert_eq!(
            field.visibility,
            Visibility::Public,
            "field {} in synthesized {} class must be Public, was {:?}",
            field.name,
            inputs_class.name,
            field.visibility
        );
    }
}
