//! Regression (compiler-old #27): errors inside code a plugin re-parses
//! (endpoint handler bodies, `component:` event handlers, `Model.find:`
//! blocks inside class methods) named `<plugin-fragment>:LINE:COL` or `:0:0`,
//! where LINE was relative to the generated fragment, and the snippet showed
//! that line of the entry file, which has nothing to do with the error.
//!
//! The diagnostic must name the user's file and the line the user wrote, and
//! the snippet must come from that file.
//!
//! Needs the installed frame.* plugins; each case skips when one is missing.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn plugins_installed(names: &[&str]) -> bool {
    let Some(home) = std::env::var_os("HOME") else {
        return false;
    };
    names.iter().all(|name| {
        PathBuf::from(&home)
            .join(".cleen/plugins")
            .join(name)
            .join("plugin.toml")
            .exists()
    })
}

fn write(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

/// Runs `cln <args>` in `dir`; returns (success, stdout + stderr).
fn cln(dir: &Path, args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_cln"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run cln");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.success(), text)
}

/// The rendered diagnostic block (from `error[CODE]` to the next blank line)
/// whose header line mentions `name`.
fn diagnostic<'a>(output: &'a str, code: &str, name: &str) -> &'a str {
    let header = format!("error[{}]", code);
    let mut from = 0;
    while let Some(found) = output[from..].find(&header) {
        let start = from + found;
        let rest = &output[start..];
        let end = rest.find("\n\n").unwrap_or(rest.len());
        if rest.lines().next().is_some_and(|l| l.contains(name)) {
            return &rest[..end];
        }
        from = start + header.len();
    }
    panic!(
        "no {} naming `{}` in compiler output:\n{}",
        header, name, output
    )
}

/// `diag` points at `file:line:` and shows `source_line` as the error line of
/// its snippet, and carries no fragment-relative location.
fn assert_points_at(diag: &str, file: &str, line: usize, source_line: &str) {
    let location = format!("{}:{}:", file, line);
    assert!(
        diag.lines()
            .any(|l| l.trim_start().starts_with("-->") && l.contains(&location)),
        "diagnostic must point at {}:\n{}",
        location,
        diag
    );
    assert!(
        !diag.contains("<plugin-fragment") && !diag.contains("--> :0:0"),
        "diagnostic must not carry a plugin-fragment location:\n{}",
        diag
    );
    let snippet_line = format!("{} |", line);
    assert!(
        diag.lines()
            .any(|l| l.trim_start().starts_with(&snippet_line) && l.contains(source_line)),
        "snippet must show line {} of {} (`{}`):\n{}",
        line,
        file,
        source_line,
        diag
    );
}

/// A project laid out like `cln init` server apps: `main.cln` holds the
/// package manifest, endpoints and classes live in their own files.
fn server_project(root: &Path) {
    write(
        root,
        "main.cln",
        "package: LocationApp\n\tversion: \"1.0.0\"\n\n\ttarget: server\n\t\tplugins: [frame.data, frame.server]\n\t\tentry: app/server/main.cln\n",
    );
    write(
        root,
        "app/server/main.cln",
        "plugins:\n\tframe.server\n\tframe.data\n\nserver:\n\tport: 3000\n\nstart:\n\tprintl(\"ready\")\n",
    );
    write(
        root,
        "app/data/models/Note.cln",
        "data Note:\n\tfields:\n\t\tinteger id primary generated\n\t\tstring title required\n\t\tinteger priority\n",
    );
}

/// The project's endpoints file: a comment line before the first route, two
/// handlers that start with the same line. `error_line` (1-based, 7 or 12)
/// calls an undeclared function.
fn endpoints_file(error_line: usize) -> String {
    let call = |line: usize, name: &str, ok: &str| {
        if line == error_line {
            format!("\t\tinteger {} = missingRouteHelper(a)\n", name)
        } else {
            format!("\t\tinteger {} = {}\n", name, ok)
        }
    };
    [
        "// Health endpoint\n".to_string(),                        // 1
        "\n".to_string(),                                          // 2
        "endpoints:\n".to_string(),                                // 3
        "\t// comment lines before the first route\n".to_string(), // 4
        "\tGET \"/health\":\n".to_string(),                        // 5
        "\t\tinteger a = 1\n".to_string(),                         // 6
        call(7, "b", "a + 1"),                                     // 7
        "\t\treturn json({ok: true, value: b})\n".to_string(),     // 8
        "\n".to_string(),                                          // 9
        "\tGET \"/ready\":\n".to_string(),                         // 10
        "\t\tinteger a = 1\n".to_string(),                         // 11
        call(12, "c", "a + 2"),                                    // 12
        "\t\treturn json({ok: true, value: c})\n".to_string(),     // 13
    ]
    .concat()
}

#[test]
fn unknown_function_in_endpoint_handler_names_the_handler_line() {
    if !plugins_installed(&["frame.server", "frame.data"]) {
        eprintln!("skipping: frame.server / frame.data not installed");
        return;
    }
    // Line 12 is in the second handler, which starts with the same line as
    // the first; its error must still name its own line.
    for error_line in [7, 12] {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        server_project(root);
        write(
            root,
            "app/server/api/health.cln",
            &endpoints_file(error_line),
        );

        let (ok, output) = cln(root, &["build", "main.cln"]);
        assert!(!ok, "build must fail:\n{}", output);
        let source_line = if error_line == 7 {
            "integer b = missingRouteHelper(a)"
        } else {
            "integer c = missingRouteHelper(a)"
        };
        assert_points_at(
            diagnostic(&output, "SEM007", "missingRouteHelper"),
            "app/server/api/health.cln",
            error_line,
            source_line,
        );
    }
}

#[test]
fn type_error_on_model_query_in_class_method_names_the_statement_line() {
    if !plugins_installed(&["frame.server", "frame.data"]) {
        eprintln!("skipping: frame.server / frame.data not installed");
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    server_project(root);
    write(
        root,
        "app/server/logic/Notes.cln",
        concat!(
            "// Notes helper\n",                  // 1
            "\n",                                 // 2
            "class Notes\n",                      // 3
            "\tfunctions:\n",                     // 4
            "\t\tstring urgent()\n",              // 5
            "\t\t\tstring result = Note.find:\n", // 6
            "\t\t\t\twhere:\n",                   // 7
            "\t\t\t\t\tpriority == 1\n",          // 8
            "\t\t\treturn result\n",              // 9
        ),
    );

    let (ok, output) = cln(root, &["build", "main.cln"]);
    assert!(!ok, "build must fail:\n{}", output);
    assert_points_at(
        diagnostic(&output, "SEM004", "result"),
        "app/server/logic/Notes.cln",
        6,
        "string result = Note.find:",
    );
}

#[test]
fn unknown_function_in_component_event_names_the_event_line() {
    if !plugins_installed(&["frame.ui", "frame.server"]) {
        eprintln!("skipping: frame.ui / frame.server not installed");
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    write(
        root,
        "app/ui/web/components/demo-box.cln",
        concat!(
            "plugins:\n",                                      // 1
            "\tframe.ui\n",                                    // 2
            "\tframe.server\n",                                // 3
            "\n",                                              // 4
            "component: tag=\"demo-box\"\n",                   // 5
            "\tstate:\n",                                      // 6
            "\t\tinteger count = 0\n",                         // 7
            "\n",                                              // 8
            "\tevents:\n",                                     // 9
            "\t\tvoid bump()\n",                               // 10
            "\t\t\tinteger n = missingEventHelper(1)\n",       // 11
            "\t\t\tstate.count = n\n",                         // 12
            "\n",                                              // 13
            "\thtml:\n",                                       // 14
            "\t\t<div onclick=\"bump\">{state.count}</div>\n", // 15
            "\n",                                              // 16
            "start:\n",                                        // 17
            "\tprintl(\"boot\")\n",                            // 18
        ),
    );

    let (ok, output) = cln(
        root,
        &[
            "compile",
            "app/ui/web/components/demo-box.cln",
            "-o",
            "demo-box.wasm",
        ],
    );
    assert!(!ok, "compile must fail:\n{}", output);
    assert_points_at(
        diagnostic(&output, "SEM007", "missingEventHelper"),
        "demo-box.cln",
        11,
        "integer n = missingEventHelper(1)",
    );
}
