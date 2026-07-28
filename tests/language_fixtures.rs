use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use mezura::config_manager::Configuration;
use mezura::file_parser::KeywordMatcher;
use mezura::*;

const MARKER: &str = "mezura-expect";

// Each fixture declares, on its first line and in its own comment syntax, the counts mezura must
// produce for it. The counts are hand-verified, so a mismatch means either the parser regressed or
// the fixture is wrong; both are worth stopping for. The header line itself is a comment, so it is
// included in 'lines' and excluded from 'code'.
fn parse_expectations(first_line: &str) -> Option<HashMap<String, usize>> {
    let after_marker = first_line.split_once(MARKER)?.1;
    let mut expectations = HashMap::new();
    for entry in after_marker.split_whitespace() {
        let (key, value) = entry.split_once('=')?;
        expectations.insert(key.to_owned(), value.parse::<usize>().ok()?);
    }

    if expectations.is_empty() { None } else { Some(expectations) }
}

fn fixture_paths(root: &Path) -> Vec<std::path::PathBuf> {
    let mut paths = std::fs::read_dir(root)
        .unwrap_or_else(|x| panic!("cannot read the fixture directory {}: {x}", root.display()))
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

#[test]
fn language_fixtures_match_their_declared_counts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("lang");
    let language_map = Arc::new(io_handler::parse_supported_languages_to_map(&LOCAL_APP_PATHS.languages_dir).unwrap().0);
    let extension_map = make_extension_language_map(&language_map);
    // Built-in defaults only, so that a preference in the machine's own config file cannot change the counts
    let config = Configuration::new(Vec::new());

    let mut failures = Vec::new();
    let mut checked = 0;

    for path in fixture_paths(&root) {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let extension = path.extension().and_then(|x| x.to_str()).unwrap_or_default();

        let Some(lang_name) = find_language_of_extension(&extension_map, extension) else {
            failures.push(format!("{name}: no supported language claims the extension '{extension}'"));
            continue;
        };

        let contents = std::fs::read_to_string(&path).unwrap();
        let Some(expected) = parse_expectations(contents.lines().next().unwrap_or_default()) else {
            failures.push(format!("{name}: the first line must contain a '{MARKER} lines=N code=N ...' header"));
            continue;
        };

        let language = language_map.get(lang_name.as_ref()).unwrap();
        let keyword_matcher = KeywordMatcher::build(language);
        let mut buf = String::new();
        let stats = match file_parser::parse_file(&path, lang_name.as_ref(), &mut buf, language_map.clone(), keyword_matcher.as_ref(), &config) {
            Ok(stats) => stats,
            Err(x) => {
                failures.push(format!("{name}: could not be parsed: {x}"));
                continue;
            }
        };

        let mut actual = HashMap::from([
            ("lines".to_owned(), stats.lines),
            ("code".to_owned(), stats.code_lines),
            ("extra".to_owned(), stats.lines - stats.code_lines),
        ]);
        for (index, keyword) in language.keywords.iter().enumerate() {
            actual.insert(keyword.descriptive_name.clone(), stats.keyword_occurences[index]);
        }

        for (key, expected_value) in &expected {
            match actual.get(key) {
                Some(actual_value) if actual_value == expected_value => (),
                Some(actual_value) => failures.push(format!("{name} ({lang_name}): {key} expected {expected_value}, got {actual_value}")),
                None => {
                    let mut known = actual.keys().cloned().collect::<Vec<_>>();
                    known.sort();
                    failures.push(format!("{name} ({lang_name}): '{key}' is not a countable field. Available: {}", known.join(", ")));
                }
            }
        }

        // A keyword the fixture does not mention must be absent, otherwise a fixture could quietly
        // stop covering a keyword the moment someone forgets to declare it
        for (index, keyword) in language.keywords.iter().enumerate() {
            let occurrences = stats.keyword_occurences[index];
            if occurrences > 0 && !expected.contains_key(&keyword.descriptive_name) {
                failures.push(format!("{name} ({lang_name}): found {occurrences} '{}' but the header does not declare them",
                        keyword.descriptive_name));
            }
        }

        checked += 1;
    }

    assert!(checked > 0, "no fixtures were checked, is {} populated?", root.display());
    assert!(failures.is_empty(), "\n{} fixture check(s) failed:\n  {}\n", failures.len(), failures.join("\n  "));
}

#[test]
fn every_fixture_extension_resolves_to_exactly_one_language() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures").join("lang");
    let language_map = io_handler::parse_supported_languages_to_map(&LOCAL_APP_PATHS.languages_dir).unwrap().0;

    let mut claimants_of = HashMap::<String, Vec<String>>::new();
    for language in language_map.values() {
        for extension in &language.extensions {
            claimants_of.entry(extension.clone()).or_default().push(language.name.clone());
        }
    }

    for path in fixture_paths(&root) {
        let extension = path.extension().and_then(|x| x.to_str()).unwrap_or_default().to_owned();
        let claimants = claimants_of.get(&extension).cloned().unwrap_or_default();
        assert!(claimants.len() == 1, "the fixture extension '{extension}' is claimed by {} languages ({}), so its counts depend on the tie-break rule",
                claimants.len(), claimants.join(", "));
    }
}
