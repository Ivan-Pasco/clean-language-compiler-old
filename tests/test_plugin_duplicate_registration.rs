//! plugin-contract.md §1.2 rules 5 and 6: duplicate block names and
//! conflicting bridge signatures are PLUGIN002 (Ivan-Pasco/clean-language-compiler-old#72).

mod plugin_fixture;

use plugin_fixture::{install_plugin, text, Project};

#[test]
fn two_plugins_with_conflicting_bridge_signatures_are_plugin002() {
    let p = Project::new();
    install_plugin(
        p.home.path(),
        "acme.bra",
        &[],
        "",
        "  { name = \"_acme_ping\", params = [\"integer\"], returns = \"integer\", hosts = [\"all\"] },",
    );
    install_plugin(
        p.home.path(),
        "acme.brb",
        &[],
        "",
        "  { name = \"_acme_ping\", params = [\"string\"], returns = \"string\", expand_strings = true, hosts = [\"all\"] },",
    );
    p.write(
        "br.cln",
        "plugins:\n\tacme.bra\n\tacme.brb\n\nstart:\n\tprintl(_acme_ping(\"x\"))\n",
    );
    let out = p.compile("br.cln");
    let all = text(&out);
    assert!(!out.status.success(), "{}", all);
    assert!(all.contains("PLUGIN002"), "{}", all);
    assert!(
        all.contains("acme.bra") && all.contains("acme.brb"),
        "{}",
        all
    );
    assert!(all.contains("_acme_ping"), "{}", all);
    assert!(
        !all.contains("compiler bug") && !all.contains("COM001"),
        "must not reach code generation: {}",
        all
    );
}

#[test]
fn two_plugins_handling_the_same_block_are_plugin002() {
    let p = Project::new();
    install_plugin(p.home.path(), "acme.one", &["greet"], "printl(\"1\")", "");
    install_plugin(p.home.path(), "acme.two", &["greet"], "printl(\"2\")", "");
    p.write(
        "dup.cln",
        "plugins:\n\tacme.one\n\tacme.two\n\nstart:\n\tgreet:\n\t\thello\n",
    );
    let out = p.compile("dup.cln");
    let all = text(&out);
    assert!(!out.status.success(), "{}", all);
    assert!(all.contains("PLUGIN002"), "{}", all);
    assert!(!all.contains("PLUGIN001"), "{}", all);
    assert!(
        all.contains("acme.one") && all.contains("acme.two"),
        "{}",
        all
    );
    assert!(all.contains("greet"), "{}", all);
}
