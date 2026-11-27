//! plugin-contract.md §5.3: v1 plugin output with `start:` or plain statements
//! is merged into the user's own `start:` (Ivan-Pasco/clean-language-compiler-old#72).

mod plugin_fixture;

use plugin_fixture::{install_plugin, text, Project};

const GREET_IN_START: &str =
    "plugins:\n\tacme.greet\n\nstart:\n\tprintl(\"a\")\n\tgreet:\n\t\thello\n\tprintl(\"b\")\n";

#[test]
fn v1_plain_statement_output_merges_into_users_start() {
    let p = Project::new();
    install_plugin(
        p.home.path(),
        "acme.greet",
        &["greet"],
        "printl(\"expanded\")",
        "",
    );
    p.write("greet.cln", GREET_IN_START);
    let out = p.compile("greet.cln");
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(p.run(), "a\nexpanded\nb\n");
}

#[test]
fn v1_start_block_output_merges_into_users_start() {
    let p = Project::new();
    install_plugin(
        p.home.path(),
        "acme.greet",
        &["greet"],
        "start:\n\tprintl(\"expanded\")",
        "",
    );
    p.write("greet.cln", GREET_IN_START);
    let out = p.compile("greet.cln");
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(p.run(), "a\nexpanded\nb\n");
}

#[test]
fn v1_top_level_block_output_merges_into_users_start() {
    let p = Project::new();
    install_plugin(
        p.home.path(),
        "acme.greet",
        &["greet"],
        "printl(\"expanded\")",
        "",
    );
    p.write(
        "greet.cln",
        "plugins:\n\tacme.greet\n\nstart:\n\tprintl(\"a\")\n\ngreet:\n\thello\n",
    );
    let out = p.compile("greet.cln");
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(p.run(), "a\nexpanded\n");
}
