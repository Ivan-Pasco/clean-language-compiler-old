//! plugin-contract.md §5.4: a failed expansion points at the user's block
//! (Ivan-Pasco/clean-language-compiler-old#72).

mod plugin_fixture;

use plugin_fixture::{install_plugin, text, Project};

#[test]
fn expansion_failure_points_at_the_users_block() {
    let p = Project::new();
    install_plugin(p.home.path(), "acme.greet", &["greet"], "@@@ not clean", "");
    p.write(
        "greet5.cln",
        "plugins:\n\tacme.greet\n\nstart:\n\tprintl(\"a\")\n\ngreet:\n\thello\n",
    );
    let out = p.compile("greet5.cln");
    let all = text(&out);
    assert!(!out.status.success(), "{}", all);
    assert!(
        all.contains("greet5.cln:7:1"),
        "the diagnostic must point at the greet: block (line 7): {}",
        all
    );
    assert!(
        !all.contains("greet5.cln:1:1"),
        "the diagnostic must not fall back to line 1: {}",
        all
    );
    assert!(
        !all.contains("SYN001"),
        "the user's syntax is not what failed: {}",
        all
    );
}
