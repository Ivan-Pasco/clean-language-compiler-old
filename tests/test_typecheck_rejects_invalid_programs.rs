//! Type-checking regressions: programs that must be REJECTED with a
//! diagnostic, not compiled into a module that misbehaves at runtime (or
//! crashes the code generator, or hangs the compiler).
//!
//! Each case is written to its own unique temporary directory and compiled
//! with `compile_with_file`.

use clean_language_compiler::error::CompilerError;
use std::sync::mpsc;
use std::time::Duration;

/// Compile `source` from a fresh temp directory. The compile runs on a
/// worker thread so a compiler that never terminates fails the test instead
/// of hanging the suite.
fn compile(name: &str, source: &str) -> Result<Vec<u8>, Vec<CompilerError>> {
    let dir = std::env::temp_dir().join(format!(
        "clean_typecheck_reject_{}_{}_{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join(format!("{name}.cln"));
    std::fs::write(&path, source).expect("write source");
    let path_str = path.to_string_lossy().to_string();
    let source = source.to_string();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(clean_language_compiler::compile_with_file(
            &source, &path_str,
        ));
    });
    let result = rx
        .recv_timeout(Duration::from_secs(120))
        .unwrap_or_else(|_| panic!("compiling '{name}' did not terminate"));
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn expect_rejected(name: &str, source: &str, codes: &[&str]) {
    let errors = match compile(name, source) {
        Ok(_) => panic!("'{name}' must be rejected, but it compiled"),
        Err(errors) => errors,
    };
    let rendered: Vec<String> = errors.iter().map(|e| format!("{e:?}")).collect();
    assert!(
        rendered.iter().all(|e| !e.contains("Code Generation")),
        "'{name}' must be rejected by the type checker, not crash code generation: {rendered:?}"
    );
    assert!(
        codes
            .iter()
            .any(|code| rendered.iter().any(|e| e.contains(code))),
        "'{name}' must report one of {codes:?}, got: {rendered:?}"
    );
}

/// SEM008: a cycle that does not pass through the class being validated
/// (A → B → C → B) must be reported, and the compiler must terminate.
#[test]
fn inheritance_cycle_beyond_the_parent_is_reported() {
    expect_rejected(
        "cycle_beyond_parent",
        "start:\n\tprint(\"hi\")\n\nclass A is B\n\tpublic:\n\t\tinteger x\n\nclass B is C\n\tpublic:\n\t\tinteger y\n\nclass C is B\n\tpublic:\n\t\tinteger z\n",
        &["SEM008"],
    );
}

const POINT: &str =
    "class Point\n\tpublic:\n\t\tinteger x\n\n\tconstructor(integer v)\n\t\tx = v\n";

/// SEM001: a constructor argument must match the parameter type.
#[test]
fn constructor_argument_type_mismatch_is_rejected() {
    let src = format!("start:\n\tPoint p = Point(\"seven\")\n\tprintl(p.x.toString())\n\n{POINT}");
    expect_rejected("ctor_arg_type", &src, &["SEM001"]);
}

/// FUNC002: a constructor call must pass as many arguments as it declares.
#[test]
fn constructor_argument_count_mismatch_is_rejected() {
    let src = format!("start:\n\tPoint p = Point(1, 2)\n\tprintl(p.x.toString())\n\n{POINT}");
    expect_rejected("ctor_arg_count", &src, &["FUNC002"]);
    let src = format!("start:\n\tPoint p = Point()\n\tprintl(p.x.toString())\n\n{POINT}");
    expect_rejected("ctor_arg_missing", &src, &["FUNC002"]);
}

const COUNTER: &str = "class Counter\n\tpublic:\n\t\tinteger x\n\n\tconstructor(integer v)\n\t\tx = v\n\n\tfunctions:\n\t\tpublic:\n\t\t\tinteger plus(integer k)\n\t\t\t\treturn x + k\n";

/// SEM001: a method argument must match the parameter type.
#[test]
fn method_argument_type_mismatch_is_rejected() {
    let src = format!(
        "start:\n\tCounter c = Counter(1)\n\tprintl(c.plus(\"two\").toString())\n\n{COUNTER}"
    );
    expect_rejected("method_arg_type", &src, &["SEM001"]);
}

/// FUNC002: a method call must pass as many arguments as it declares.
#[test]
fn method_argument_count_mismatch_is_rejected() {
    let src =
        format!("start:\n\tCounter c = Counter(1)\n\tprintl(c.plus().toString())\n\n{COUNTER}");
    expect_rejected("method_arg_missing", &src, &["FUNC002"]);
    let src =
        format!("start:\n\tCounter c = Counter(1)\n\tprintl(c.plus(1, 2).toString())\n\n{COUNTER}");
    expect_rejected("method_arg_extra", &src, &["FUNC002"]);
}

/// SEM005: a private field may not be written from outside its class.
#[test]
fn private_field_write_outside_class_is_rejected() {
    expect_rejected(
        "private_field_write",
        "start:\n\tVault v = Vault(7)\n\tv.secret = 9\n\tprintl(v.x.toString())\n\nclass Vault\n\tinteger secret\n\n\tpublic:\n\t\tinteger x\n\n\tconstructor(integer v)\n\t\tx = v\n\t\tsecret = v\n",
        &["SEM005"],
    );
}

/// SEM004: unary minus applies to numbers only.
#[test]
fn negating_a_boolean_is_rejected() {
    expect_rejected(
        "negate_boolean",
        "start:\n\tinteger x = -true\n\tprintl(x.toString())\n",
        &["SEM004"],
    );
}

/// SEM001: every element of a list literal must have the list's element type.
#[test]
fn list_literal_with_mixed_element_types_is_rejected() {
    expect_rejected(
        "mixed_list_literal",
        "start:\n\tlist<integer> xs = [1, \"a\"]\n\tprintl(xs.length().toString())\n",
        &["SEM001"],
    );
}

/// SEM001: a value added to a list must have the list's element type.
#[test]
fn adding_a_wrong_typed_value_to_a_list_is_rejected() {
    expect_rejected(
        "list_add_wrong_type",
        "start:\n\tlist<integer> xs = [1, 2]\n\txs.add(\"s\")\n\tprintl(xs.length().toString())\n",
        &["SEM001"],
    );
}

/// SEM004: joining a list is the namespace function `list.join(items, sep)`;
/// the method form `items.join(sep)` is not part of the language
/// (method-style syntax: every operation has one name). It used to compile
/// and print "null" at runtime.
#[test]
fn list_join_method_form_is_rejected() {
    expect_rejected(
        "list_join_method",
        "start:\n\tlist<string> items = [\"a\", \"b\"]\n\tprint(items.join(\", \"))\n",
        &["SEM004"],
    );
}

/// SEM004: a list's behavior is declared with its type (`list<T>.unique`);
/// a list has no assignable properties. `items.type = "unique"` reached the
/// MIR builder as a class field store.
#[test]
fn assigning_a_list_property_is_rejected() {
    for element in ["string", "integer"] {
        expect_rejected(
            "list_property_assignment",
            &format!(
                "start:\n\tlist<{element}> items = []\n\titems.type = \"unique\"\n\tprint(items.size())\n"
            ),
            &["SEM004"],
        );
    }
}
