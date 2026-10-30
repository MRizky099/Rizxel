//! The user guide's shortcuts table (`docs/guide/shortcuts.md`) is generated
//! from the default keymap. This test fails when the two drift apart;
//! `RIZXEL_WRITE_GUIDE=1 cargo test -p rizxel-app --test guide_shortcuts`
//! rewrites the generated part.

use rizxel_app::{ActionGroup, ActionId, Keymap};

const BEGIN: &str = "<!-- BEGIN GENERATED: rizxel-app/tests/guide_shortcuts.rs -->";
const END: &str = "<!-- END GENERATED -->";

/// One table per keymap group, in the order of *Settings ▸ Keymap*.
fn generated() -> String {
    let keymap = Keymap::default();
    let mut out = String::new();
    for group in ActionGroup::ALL {
        out.push_str(&format!("\n## {}\n\n", group.label()));
        out.push_str("| Command | Shortcut |\n|---|---|\n");
        for action in ActionId::all().into_iter().filter(|a| a.group() == group) {
            let chords: Vec<String> = keymap
                .chords(action)
                .iter()
                // The guide spells the command key `Ctrl` on every platform.
                .map(|c| format!("`{}`", c.label().replace("Cmd+", "Ctrl+")))
                .collect();
            let keys = if chords.is_empty() {
                "—".to_owned()
            } else {
                chords.join(" or ")
            };
            out.push_str(&format!("| {} | {keys} |\n", action.label()));
        }
    }
    out.push('\n');
    out
}

#[test]
fn guide_shortcuts_match_the_default_keymap() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/guide/shortcuts.md");
    let page = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\r\n", "\n");
    let start = page.find(BEGIN).expect("begin marker") + BEGIN.len();
    let end = page.find(END).expect("end marker");
    let expected = generated();
    if std::env::var_os("RIZXEL_WRITE_GUIDE").is_some() {
        let updated = format!("{}{expected}{}", &page[..start], &page[end..]);
        std::fs::write(&path, updated).unwrap();
        return;
    }
    assert!(
        page[start..end] == expected,
        "docs/guide/shortcuts.md is out of date; rerun with RIZXEL_WRITE_GUIDE=1"
    );
}
