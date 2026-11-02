use std::path::Path;

use chrono::{DateTime, Local};
use mezura_core::RunResult;

use super::badge::{BadgeShape, BadgeSpec, render_badge};
use super::config_manager::Configuration;
use super::json_printer::{create_document, escape};
use super::migration::get_shipped_theme_files;
use super::theme::{parse_theme_file, resolve};
use super::theme_files::{STOCK_LOOK_THEME, build_listing_key};

const TEMPLATE : &str = include_str!("../templates/report.html");
// Where the template takes what the run supplies, in the order the template holds them
const FONTS_MARKER : &str = "/*MEZURA_FONTS*/";
const CARD_MARKER : &str = "<!--MEZURA_CARD-->";
const BADGE_MARKER : &str = "<!--MEZURA_BADGE-->";
const THEMES_MARKER : &str = "/*MEZURA_THEMES*/";
const VIEW_MARKER : &str = "/*MEZURA_VIEW*/";
const DOCUMENT_MARKER : &str = "/*MEZURA_DOCUMENT*/";
const FONT_FACES : [(&str, &str, u16, &[u8]); 4] = [
    ("Inter", "normal", 400, include_bytes!("../templates/fonts/Inter-Regular.woff2")),
    ("Inter", "italic", 400, include_bytes!("../templates/fonts/Inter-Italic.woff2")),
    ("Inter", "normal", 600, include_bytes!("../templates/fonts/Inter-SemiBold.woff2")),
    ("JetBrains Mono", "normal", 400, include_bytes!("../templates/fonts/JetBrainsMono-Regular.woff2"))
];
const BASE64_ALPHABET : &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn print_as_html(result: &RunResult, datetime_now: &DateTime<Local>, config: &Configuration) {
    outln!("{}", create_page(result, datetime_now, config));
}

fn create_page(result: &RunResult, datetime_now: &DateTime<Local>, config: &Configuration) -> String {
    let fillings = [
        (FONTS_MARKER, create_font_faces()),
        (CARD_MARKER, render_badge(result, config, &BadgeSpec::of(BadgeShape::Card))),
        (BADGE_MARKER, render_badge(result, config, &BadgeSpec::of(BadgeShape::Pill))),
        (THEMES_MARKER, escape_for_script(&create_themes_document(config))),
        (VIEW_MARKER, create_view_document(config)),
        (DOCUMENT_MARKER, escape_for_script(&create_document(result, datetime_now, config)))
    ];

    fill_template(TEMPLATE, &fillings)
}

// The sort column, which the document shows only through its order
fn create_view_document(config: &Configuration) -> String {
    let sort = match config.view.sort_by.as_str() {
        "extra" | "blanks" => "third",
        "size" => "bytes",
        other => other
    };

    format!("{{\"sort\":\"{sort}\"}}")
}

// Each marker is looked for in the template only, so a
// path in the document that happens to spell a marker is left alone
fn fill_template(template: &str, fillings: &[(&str, String)]) -> String {
    let mut page = String::with_capacity(template.len() + fillings.iter().map(|(_, filling)| filling.len()).sum::<usize>());
    let mut rest = template;
    for (marker, filling) in fillings {
        let (before, after) = rest.split_once(marker)
                .unwrap_or_else(|| panic!("the report template lost its '{marker}' or holds it out of order"));
        page.push_str(before);
        page.push_str(filling);
        rest = after;
    }
    page.push_str(rest);

    page
}

// The run's theme first, under the shipped file's spelling whatever case it was typed in, since
// the page finds its light and dark partner by that spelling
fn create_themes_document(config: &Configuration) -> String {
    let typed_name = config.view.theme_name.clone().unwrap_or_else(|| STOCK_LOOK_THEME.to_owned());
    let mut shipped = get_shipped_theme_files().into_iter().filter_map(|(relative, contents)| {
        let name = Path::new(&relative).file_stem()?.to_str()?.to_owned();
        let contents = std::str::from_utf8(contents).ok()?;
        Some((name, resolve(&parse_theme_file(contents).0, &[], &[])))
    }).collect::<Vec<_>>();
    let active_name = shipped.iter().map(|(name, _)| name.clone())
            .find(|name| name.eq_ignore_ascii_case(&typed_name)).unwrap_or(typed_name);
    shipped.retain(|(name, _)| *name != active_name);
    shipped.sort_by_key(|(name, _)| build_listing_key(name));
    let mut themes = vec![(active_name.clone(), config.view.theme.clone())];
    themes.extend(shipped);

    let order = themes.iter().map(|(name, _)| format!("\"{}\"", escape(name))).collect::<Vec<_>>().join(",");
    let entries = themes.iter().map(|(name, theme)| {
        let tokens = theme.collect_all_tokens().iter()
                .map(|(token, value)| format!("\"{token}\":\"{}\"", escape(value))).collect::<Vec<_>>().join(",");
        format!("\"{}\":{{{tokens}}}", escape(name))
    }).collect::<Vec<_>>().join(",");

    format!("{{\"active\":\"{}\",\"order\":[{order}],\"themes\":{{{entries}}}}}", escape(&active_name))
}

// Every '<' sits inside a string, so escaping them all keeps a '</script>' in a path from closing the script
fn escape_for_script(json: &str) -> String {
    json.replace('<', "\\u003c")
}

fn create_font_faces() -> String {
    FONT_FACES.iter().map(|(family, style, weight, bytes)| format!(
            "@font-face{{font-family:\"{family}\";font-style:{style};font-weight:{weight};font-display:swap;\
             src:url(data:font/woff2;base64,{}) format(\"woff2\")}}", encode_base64(bytes)))
            .collect::<Vec<_>>().join("\n")
}

fn encode_base64(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let triple = chunk.iter().enumerate().fold(0u32, |acc, (i, byte)| acc | (u32::from(*byte) << (16 - 8 * i)));
        let sextets = [(triple >> 18) & 63, (triple >> 12) & 63, (triple >> 6) & 63, triple & 63];
        for (i, sextet) in sextets.iter().enumerate() {
            encoded.push(if i <= chunk.len() {char::from(BASE64_ALPHABET[*sextet as usize])} else {'='});
        }
    }

    encoded
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::TimeZone;
    use mezura_core::Target;

    use super::*;
    use crate::test_support::{plain_result_of, plain_stats_of};

    #[test]
    fn the_page_carries_the_document_of_the_run_and_a_path_cannot_close_its_script() {
        let per_language = hashmap!["Rust".to_owned() => plain_stats_of(1, 100, 10, 8, 1, HashMap::new())];
        let result = plain_result_of(per_language, Vec::new(), vec![Target::of("D:/x/</script><b>/y")]);
        let config = Configuration::new(vec!["./src".to_owned()]);
        let now = Local.with_ymd_and_hms(2026, 11, 2, 1, 17, 0).unwrap();

        let page = create_page(&result, &now, &config);
        let document = create_document(&result, &now, &config);

        assert!(document.contains("</script>"));
        assert_eq!(TEMPLATE.matches("</script>").count(), page.matches("</script>").count());
        let embedded = page.split("id=\"mezura-document\">").nth(1).unwrap().split("</script>").next().unwrap();
        assert_eq!(document, embedded.replace("\\u003c", "<"));
    }

    #[test]
    fn the_theme_of_the_run_leads_the_themes_of_the_page_and_the_shipped_ones_follow() {
        let mut config = Configuration::new(vec!["./src".to_owned()]);
        config.view.theme_name = Some("Dracula".to_owned());

        let themes = create_themes_document(&config);

        assert!(themes.starts_with("{\"active\":\"Dracula\",\"order\":[\"Dracula\",\"Mezura\",\"Mezura-classic\",\"Catppuccin\","), "{themes}");
        assert_eq!(1, themes.matches("\"Dracula\":{").count());
        for name in ["Catppuccin-light", "Ocean", "Uncolored"] {
            assert!(themes.contains(&format!("\"{name}\":{{\"")), "{name} is not in the page");
        }

        config.view.theme_name = Some("dracula".to_owned());
        let typed_in_lower_case = create_themes_document(&config);
        assert!(typed_in_lower_case.starts_with("{\"active\":\"Dracula\","), "{typed_in_lower_case}");
        assert_eq!(1, typed_in_lower_case.matches("\"Dracula\":{").count());
    }

    #[test]
    fn every_marker_is_in_the_template_once_and_in_the_page_not_at_all() {
        let result = plain_result_of(HashMap::new(), Vec::new(), Vec::new());
        let config = Configuration::new(vec!["./src".to_owned()]);
        let page = create_page(&result, &Local.with_ymd_and_hms(2026, 11, 2, 1, 17, 0).unwrap(), &config);

        for marker in [FONTS_MARKER, BADGE_MARKER, CARD_MARKER, THEMES_MARKER, VIEW_MARKER, DOCUMENT_MARKER] {
            assert_eq!(1, TEMPLATE.matches(marker).count(), "{marker}");
            assert!(!page.contains(marker), "{marker} was left in the page");
        }
    }

    #[test]
    fn base64_is_encoded_as_the_standard_spells_it() {
        assert_eq!("", encode_base64(b""));
        assert_eq!("TQ==", encode_base64(b"M"));
        assert_eq!("TWE=", encode_base64(b"Ma"));
        assert_eq!("TWFu", encode_base64(b"Man"));
        assert_eq!("AP/+", encode_base64(&[0, 255, 254]));
    }
}
