use std::{fs, io};

// Where the page's themes are written into the template, the installed ones at the first and the
// shipped ones at the second. Both are comments in the HTML, so the template opens on its own.
const THEME_LIST_MARKER : &str = "/*MEZURA_SYSTEM_THEMES*/";
#[cfg(test)]
const SHIPPED_LIST_MARKER : &str = "/*MEZURA_SHIPPED_THEMES*/";

// None means no such file, which is a mistake in the name. A theme that exists always loads,
// carrying whatever its parser could not read.
pub fn load_theme(name: &str, themes_dir: &str) -> Option<super::theme::ThemeFile> {
    let entries = fs::read_dir(themes_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|x| x.to_str()) else { continue };
        if !stem.eq_ignore_ascii_case(name.trim()) {
            continue;
        }

        let contents = fs::read_to_string(&path).ok()?;
        return Some(super::theme::parse_theme_file(&contents));
    }

    None
}

// Flattened on purpose: a theme file is meant to be handed to someone else, so it carries values
// and not a reference to whatever it was built on top of.
pub fn save_theme_to_file(themes_dir: &str, name: &str, theme: &super::theme::Theme) -> io::Result<()> {
    let styles = theme.find_non_default_tokens().into_iter().map(|(token, value)| (token.to_owned(), value)).collect::<Vec<_>>();
    fs::create_dir_all(themes_dir)?;
    fs::write(themes_dir.to_owned() + name + ".txt", super::theme::create_theme_file_contents(&styles))
}

// The two directories are arguments, as they are for the loading and saving above, so that a test
// can point the whole of this at a folder of its own.
pub fn generate_theme_editor_page(themes_dir: &str, data_dir: &str) -> io::Result<String> {
    let template = include_str!("../docs/theme-editor/index.html");
    // The directory as well, so the page can say where the files it is showing actually are. The
    // published copy has no such directory and says so instead.
    let page = template.replace(THEME_LIST_MARKER,
            &format!("SYSTEM_THEMES = [{}];THEMES_DIR = \"{}\";",
                    build_themes_js(themes_dir)?, js_escape(themes_dir)));

    let out_path = data_dir.to_owned() + "theme-editor.html";
    fs::write(&out_path, page)?;

    Ok(out_path)
}

// Carries every token a theme moves and not only its language colors, and resolves each file first,
// so one that names a token twice hands over the value that would be printed.
fn build_themes_js(themes_dir: &str) -> io::Result<String> {
    let mut entries: Vec<(String, Vec<(&'static str, String)>)> = Vec::new();
    for entry in fs::read_dir(themes_dir)?.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|x| x.to_str()) else { continue };
        let Ok(contents) = fs::read_to_string(&path) else { continue };
        let resolved = super::theme::resolve(&super::theme::parse_theme_file(&contents).0, &[], &[]);
        entries.push((stem.to_owned(), resolved.find_non_default_tokens()));
    }
    entries.sort_by_key(|x| x.0.to_lowercase());

    Ok(entries.iter().map(|(name, styles)| {
        format!("{{name:\"{}\",styles:{{{}}}}}", js_escape(name),
            styles.iter().map(|(token, value)| format!("\"{token}\":\"{}\"", js_escape(value)))
                    .collect::<Vec<_>>().join(","))
    }).collect::<Vec<_>>().join(","))
}

fn js_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"").replace('<', "\\u003c")
}

#[cfg(test)]
mod tests {
    // The page exists twice: embedded above so 'cargo package' can carry it, and at
    // 'docs/theme-editor/index.html' which GitHub Pages publishes, carrying the shipped themes since
    // nobody opening it on the web has run the program. Neither can replace the other, since Pages
    // serves only from the repository's 'docs/' and the package refuses paths outside its own.
    #[test]
    fn the_published_theme_editor_is_the_embedded_one_carrying_the_shipped_themes() {
        let template = include_str!("../docs/theme-editor/index.html");
        let shipped_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data").join("themes");
        let expected = template.replace(super::SHIPPED_LIST_MARKER,
                &format!("SHIPPED_THEMES = [{}];",
                        super::build_themes_js(&shipped_dir.to_string_lossy()).unwrap()));
        let published_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..").join("docs").join("theme-editor").join("index.html");

        // Its own switch and not MEZURA_UPDATE_GOLDEN: refreshing test fixtures must not be able to
        // republish the site as a side effect.
        if std::env::var_os("MEZURA_UPDATE_PUBLISHED").is_some() {
            std::fs::write(&published_path, &expected).unwrap();
            return;
        }

        assert!(template.contains(super::SHIPPED_LIST_MARKER),
                "the marker the shipped themes replace is not in the template any more, so the \
                 published page would be left with the one theme the template opens with");
        let published = std::fs::read_to_string(&published_path).unwrap().replace("\r\n", "\n");
        assert_eq!(published, expected.replace("\r\n", "\n"),
                "docs/theme-editor/index.html no longer matches the embedded template and the themes \
                 in data/themes. Regenerate it with \
                 MEZURA_UPDATE_PUBLISHED=1 cargo test -p mezura published_theme_editor");
    }

    // The test above compares the two copies of the page to each other, so a marker renamed in both
    // of them keeps it green while the editor opens with no themes to pick from. This one runs the
    // generation instead, over a themes folder of its own so nothing else in this binary can write
    // into it while it reads.
    #[test]
    fn the_editor_page_is_written_with_the_themes_of_the_data_directory_in_it() {
        let dir = std::env::temp_dir().join("mezura_theme_editor_test");
        let (themes_dir, data_dir) = (dir.join("themes/"), dir.join("data/"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&themes_dir).unwrap();
        std::fs::create_dir_all(&data_dir).unwrap();
        // Neither value is the one mezura already uses for that token: a theme that declares the
        // default declares nothing, since the page has no way to tell it from a theme that is silent
        std::fs::write(themes_dir.join("Ocean.txt"), "language-1 = 22d3ee\n").unwrap();
        std::fs::write(themes_dir.join("Ember.txt"), "language-1 = red\npercent = ff0000 italic\n").unwrap();

        let path = super::generate_theme_editor_page(&themes_dir.to_string_lossy(),
                &data_dir.to_string_lossy()).unwrap();
        let page = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);

        assert!(!page.contains(super::THEME_LIST_MARKER),
                "the marker the theme list replaces is not in the template any more, so the page \
                 was written without one");
        assert!(page.contains("SYSTEM_THEMES = [{name:\"Ember\""), "the themes are not in the page");
        assert!(page.contains("{name:\"Ocean\""), "only one of the two themes reached the page");
        assert!(page.contains("\"language-1\":\"22d3ee\""), "a theme reached the page without the colors it declares");
        // A theme that dresses more than the overview used to arrive with everything but its five
        // language colors dropped, so the page showed it as indistinguishable from a plain one
        assert!(page.contains("\"percent\":\"ff0000 italic\""),
                "a token outside the overview did not reach the page, attributes and all");
        // Named on the page, so somebody who wants to keep what they tuned knows where to put it
        assert!(page.contains("THEMES_DIR = \""), "the page was not told where the themes it shows live");
    }

    #[test]
    fn a_theme_is_loaded_by_its_file_name_and_a_broken_line_in_it_is_reported() {
        let dir = std::env::temp_dir().join("mezura_theme_test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Mytheme.txt"), "language-1 = cyan\nlanguage-2 = bright-magenta\ncode-label = bright-yellow italic\n").unwrap();
        std::fs::write(dir.join("Broken.txt"), "language-1 = kaka\nheading = white bold\n").unwrap();
        let dir_str = dir.to_str().unwrap();

        let expected = vec![("language-1".to_owned(), "cyan".to_owned()), ("language-2".to_owned(), "bright-magenta".to_owned()),
                ("code-label".to_owned(), "bright-yellow italic".to_owned())];
        let (loaded, errors) = super::load_theme("mytheme", dir_str).unwrap();
        assert!(errors.is_empty());
        assert_eq!(expected, loaded);
        assert_eq!(expected, super::load_theme("MYTHEME", dir_str).unwrap().0);
        assert!(super::load_theme("nonexistant", dir_str).is_none());

        let (broken, errors) = super::load_theme("broken", dir_str).unwrap();
        assert_eq!(vec![("heading".to_owned(), "white bold".to_owned())], broken);
        assert_eq!(vec![crate::theme::ThemeParseError::InvalidValue("language-1".to_owned(), "kaka".to_owned())], errors);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_saved_theme_reloads_into_the_same_theme() {
        let dir = std::env::temp_dir().join("mezura_theme_save_test");
        let _ = std::fs::remove_dir_all(&dir);
        let dir_str = dir.to_str().unwrap().to_owned() + "/";

        let original = crate::theme::resolve(&[("language-1".to_owned(), "cyan".to_owned())],
                &[("heading".to_owned(), "ff0080 reverse".to_owned())], &[("code-number".to_owned(), "dim".to_owned())]);
        super::save_theme_to_file(&dir_str, "written", &original).unwrap();

        let (styles, errors) = super::load_theme("written", &dir_str).unwrap();
        assert!(errors.is_empty());
        assert_eq!(original, crate::theme::resolve(&styles, &[], &[]));

        std::fs::remove_dir_all(&dir).unwrap();
    }
}

