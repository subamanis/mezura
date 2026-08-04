// Reading and writing theme files, and the page that lets somebody build one by eye.
use std::{fs, io};



// None means the theme is not there at all, which is a mistake in the name and not in the file.
// A theme that exists always loads, carrying whatever its parser could not read.
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

// Flattened on purpose: the reason a theme file exists is that it can be handed to someone else, so
// it carries values and not a reference to whatever it was built on top of.
pub fn save_theme_to_file(themes_dir: &str, name: &str, theme: &super::theme::Theme) -> io::Result<()> {
    let styles = theme.non_default_tokens().into_iter().map(|(token, value)| (token.to_owned(), value)).collect::<Vec<_>>();
    fs::create_dir_all(themes_dir)?;
    fs::write(themes_dir.to_owned() + name + ".txt", super::theme::theme_file_contents(&styles))
}

pub fn generate_theme_editor_page() -> io::Result<String> {
    fn js_escape(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"").replace('<', "\\u003c")
    }

    let template = include_str!("../docs/theme-editor/index.html");

    let mut entries: Vec<(String, Vec<String>)> = Vec::new();
    for entry in fs::read_dir(&crate::paths::PERSISTENT_APP_PATHS.themes_dir)?.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|x| x.to_str()) else { continue };
        let Ok(contents) = fs::read_to_string(&path) else { continue };
        // The page edits the language slots only, so the rest of the theme is resolved and dropped
        let resolved = super::theme::resolve(&super::theme::parse_theme_file(&contents).0, &[], &[]);
        entries.push((stem.to_owned(), resolved.language_colors().iter().map(super::theme::color_to_config_string).collect()));
    }
    entries.sort_by_key(|x| x.0.to_lowercase());

    let themes_js = entries.iter().map(|(name, tokens)| {
        format!("{{name:\"{}\",tokens:[{}]}}", js_escape(name),
            tokens.iter().map(|t| format!("\"{}\"", js_escape(t))).collect::<Vec<_>>().join(","))
    }).collect::<Vec<_>>().join(",");

    let page = template.replace("/*MEZURA_SYSTEM_THEMES*/", &format!("SYSTEM_THEMES = [{themes_js}];"));

    let out_path = crate::paths::PERSISTENT_APP_PATHS.data_dir.clone() + "theme-editor.html";
    fs::write(&out_path, page)?;

    Ok(out_path)
}


#[cfg(test)]
mod tests {


    // The template exists twice: the copy above, embedded so 'cargo package' can carry it, and
    // 'docs/theme-editor/index.html' at the repository root, which GitHub Pages publishes as
    // https://subamanis.github.io/mezura/theme-editor/ and the README links to. Neither can replace
    // the other: Pages only serves from the repository's 'docs/', and the package refuses paths
    // outside its own directory. Same guard as the README command list against the help texts.
    #[test]
    fn the_published_theme_editor_is_the_embedded_one() {
        let template = include_str!("../docs/theme-editor/index.html");
        let published_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..").join("docs").join("theme-editor").join("index.html");

        if std::env::var_os("MEZURA_UPDATE_GOLDEN").is_some() {
            std::fs::write(&published_path, template).unwrap();
            return;
        }

        let published = std::fs::read_to_string(&published_path).unwrap().replace("\r\n", "\n");
        assert_eq!(published, template.replace("\r\n", "\n"),
                "docs/theme-editor/index.html no longer matches the embedded template it publishes. \
                 Regenerate it with MEZURA_UPDATE_GOLDEN=1 cargo test -p mezura published_theme_editor");
    }

    #[test]
    fn test_load_theme() {
        let dir = std::env::temp_dir().join("mezura_theme_test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Mytheme.txt"), "language-1 = cyan\nlanguage-2 = bright-magenta\ncode-label = bright-yellow italic\n").unwrap();
        std::fs::write(dir.join("Broken.txt"), "language-1 = kaka\nheading = white bold\n").unwrap();
        let dir_str = dir.to_str().unwrap();

        let expected = vec![("language-1".to_owned(), "cyan".to_owned()), ("language-2".to_owned(), "bright-magenta".to_owned()),
                ("code-label".to_owned(), "bright-yellow italic".to_owned())];
        let (loaded, errors) = super::super::theme_files::load_theme("mytheme", dir_str).unwrap();
        assert!(errors.is_empty());
        assert_eq!(expected, loaded);
        assert_eq!(expected, super::super::theme_files::load_theme("MYTHEME", dir_str).unwrap().0);
        assert!(super::super::theme_files::load_theme("nonexistant", dir_str).is_none());

        // A theme that is there always loads, carrying what could not be read. Only a name that
        // points at no file at all is a failure, since only that one is a mistake in the command.
        let (broken, errors) = super::super::theme_files::load_theme("broken", dir_str).unwrap();
        assert_eq!(vec![("heading".to_owned(), "white bold".to_owned())], broken);
        assert_eq!(vec![super::super::theme::ThemeParseError::InvalidValue("language-1".to_owned(), "kaka".to_owned())], errors);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // The file --save-theme writes has to reproduce the look on its own, which is the whole reason
    // it is flattened instead of pointing at whatever it was built on top of
    #[test]
    fn test_a_saved_theme_reloads_into_the_same_theme() {
        let dir = std::env::temp_dir().join("mezura_theme_save_test");
        let _ = std::fs::remove_dir_all(&dir);
        let dir_str = dir.to_str().unwrap().to_owned() + "/";

        let original = super::super::theme::resolve(&[("language-1".to_owned(), "cyan".to_owned())],
                &[("heading".to_owned(), "ff0080 reverse".to_owned())], &[("code-number".to_owned(), "dim".to_owned())]);
        super::super::theme_files::save_theme_to_file(&dir_str, "written", &original).unwrap();

        let (styles, errors) = super::super::theme_files::load_theme("written", &dir_str).unwrap();
        assert!(errors.is_empty());
        assert_eq!(original, super::super::theme::resolve(&styles, &[], &[]));

        std::fs::remove_dir_all(&dir).unwrap();
    }
}

