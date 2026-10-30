//! Links into the user guide (`docs/guide/`, published to GitHub Pages):
//! *Help ▸ User Guide* (`F1`) and the option bar's `?` button.

use rizxel_engine::Tool;

/// The published guide (mdBook output of `docs/guide/`).
pub const GUIDE_URL: &str = "https://mrizky099.github.io/Rizxel/";
/// Where bug reports go.
pub const ISSUES_URL: &str = "https://github.com/MRizky099/Rizxel/issues";

/// Render mode's guide page.
pub const RENDER_PAGE: &str = "render-mode.html";

/// Absolute URL of a guide page (`""` is the front page).
pub fn page_url(page: &str) -> String {
    format!("{GUIDE_URL}{page}")
}

/// Guide page of a tool, as in `docs/guide/SUMMARY.md`.
pub fn tool_page(tool: Tool) -> String {
    let slug = tool.label().to_ascii_lowercase().replace(' ', "-");
    format!("tools/{slug}.html")
}

/// Opens a guide page in the system browser.
pub fn open(ctx: &egui::Context, page: &str) {
    ctx.open_url(egui::OpenUrl::new_tab(page_url(page)));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every page the app links to exists in the guide sources.
    #[test]
    fn linked_pages_exist() {
        let guide = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/guide");
        let mut pages: Vec<String> = Tool::ALL.into_iter().map(tool_page).collect();
        pages.push(RENDER_PAGE.to_owned());
        for page in pages {
            let source = guide.join(page.replace(".html", ".md"));
            assert!(source.is_file(), "{} is missing", source.display());
        }
        assert_eq!(tool_page(Tool::ColorPicker), "tools/color-picker.html");
    }
}
