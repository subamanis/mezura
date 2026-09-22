use std::collections::BTreeMap;
use std::path::Path;

use mezura_core::{EngineConfig, Languages, Threads, run};

const FIXTURES_DIR : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/test_code");
const EXPECTED_FILE_NAME : &str = "expected.txt";
const TEST_DIRECTORY_NAMES : [&str; 3] = ["test", "tests", "__tests__"];
const CONSUMER_THREADS : usize = 4;

// Each tree is copied out of 'tests/' before it is counted. A target under a directory of that
// name is test code whole, and every file in it would come back that way.
#[test]
fn every_tree_of_test_code_is_counted_as_its_expected_file_says() {
    let scratch = std::env::temp_dir().join("mezura-test-code-trees");
    for component in scratch.components() {
        let name = component.as_os_str().to_string_lossy().to_lowercase();
        assert!(!TEST_DIRECTORY_NAMES.contains(&name.as_str()),
                "{} sits under a directory of tests and cannot hold the trees", scratch.display());
    }
    let _ = std::fs::remove_dir_all(&scratch);

    let mut trees = std::fs::read_dir(FIXTURES_DIR).unwrap()
            .map(|entry| entry.unwrap().path()).filter(|path| path.is_dir()).collect::<Vec<_>>();
    trees.sort();
    assert!(!trees.is_empty());
    let mut failures = Vec::new();
    for tree in &trees {
        let root = scratch.join(tree.file_name().unwrap());
        copy_tree(tree, &root);
        failures.extend(compare_tree(&root, &read_expectations(tree)));
    }
    std::fs::remove_dir_all(&scratch).unwrap();
    assert!(failures.is_empty(), "\n{}\n", failures.join("\n"));
}

struct Expectation {
    lines: usize,
    bytes: usize,
    whole: bool
}

// The bytes come from the file itself, so a checkout with other line endings does not move them
fn read_expectations(tree: &Path) -> BTreeMap<(String, String), Expectation> {
    let file = tree.join(EXPECTED_FILE_NAME);
    let text = std::fs::read_to_string(&file).unwrap();
    let mut language: Option<String> = None;
    let mut expected = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let at = format!("{}:{}", file.display(), index + 1);
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            language = Some(name.to_owned());
            continue;
        }
        let line = line.split('#').next().unwrap().trim();
        if line.is_empty() {
            continue;
        }
        let (path, spec) = line.split_once(' ').unwrap_or_else(|| panic!("{at}: nothing after the path"));
        let contents = std::fs::read(tree.join(path))
                .unwrap_or_else(|_| panic!("{at}: '{path}' is not in the tree"));
        let language = language.clone().unwrap_or_else(|| panic!("{at}: no [Language] heading above"));
        expected.insert((language, path.to_owned()), parse_expectation(spec.trim(), &contents, &at));
    }
    expected
}

fn parse_expectation(spec: &str, contents: &[u8], at: &str) -> Expectation {
    let lines_of_file = contents.split_inclusive(|byte| *byte == b'\n').collect::<Vec<_>>();
    let (lines, bytes) = match spec {
        "none" => (0, 0),
        "whole" => (lines_of_file.len(), contents.len()),
        ranges => {
            let (mut lines, mut bytes, mut last_taken) = (0, 0, 0);
            for range in ranges.split(',') {
                let (first, last) = range.split_once('-').unwrap_or((range, range));
                let parsed = (first.parse::<usize>(), last.parse::<usize>());
                let (Ok(first), Ok(last)) = parsed else { panic!("{at}: '{range}' is not a range of lines") };
                assert!(first > last_taken && first <= last && last <= lines_of_file.len(),
                        "{at}: '{range}' is out of order or past the {} lines of the file", lines_of_file.len());
                lines += last - first + 1;
                bytes += lines_of_file[first - 1..last].iter().map(|line| line.len()).sum::<usize>();
                last_taken = last;
            }
            (lines, bytes)
        }
    };
    Expectation { lines, bytes, whole: lines > 0 && lines == lines_of_file.len() }
}

fn compare_tree(root: &Path, expected: &BTreeMap<(String, String), Expectation>) -> Vec<String> {
    let root_str = root.to_string_lossy().replace('\\', "/");
    let tree = root.file_name().unwrap().to_string_lossy().into_owned();
    let counted = |detect_tests: bool| {
        let config = EngineConfig { detect_tests, collect_files: true,
                threads: Threads::new(1, CONSUMER_THREADS), ..EngineConfig::new([root_str.clone()]) };
        let (languages, _) = Languages::shipped(&config);
        run(&config, languages).unwrap()
    };
    let result = counted(true);
    let mut failures = Vec::new();

    let mut actual = BTreeMap::new();
    for (language, files) in &result.modules[0].files {
        for file in files {
            let path = file.path.strip_prefix(&format!("{root_str}/")).unwrap_or(&file.path).to_owned();
            let figures = file.tests.as_ref().map_or((0, 0), |tests| (tests.lines, tests.bytes));
            actual.insert((language.clone(), path), figures);
        }
    }
    for ((language, path), expectation) in expected {
        match actual.get(&(language.clone(), path.clone())) {
            Some(&(lines, bytes)) if (lines, bytes) == (expectation.lines, expectation.bytes) => {},
            Some(&(lines, bytes)) => failures.push(format!(
                    "{tree}/{path}: expected {} lines and {} bytes of test code, got {lines} lines and {bytes} bytes",
                    expectation.lines, expectation.bytes)),
            None => failures.push(format!("{tree}/{path}: not counted as {language}"))
        }
    }
    for (language, path) in actual.keys() {
        if !expected.contains_key(&(language.clone(), path.clone())) {
            failures.push(format!("{tree}/{path}: counted as {language} and has no line in {EXPECTED_FILE_NAME}"));
        }
    }

    let mut totals: BTreeMap<&str, (usize, usize, usize, usize)> = BTreeMap::new();
    for ((language, _), expectation) in expected {
        let total = totals.entry(language.as_str()).or_default();
        if expectation.lines > 0 {
            *total = (total.0 + 1, total.1 + expectation.lines, total.2 + expectation.bytes,
                    total.3 + usize::from(expectation.whole));
        }
    }
    for (language, total) in &totals {
        let found = result.tests.get(*language)
                .map(|tests| (tests.stats.files, tests.stats.lines, tests.stats.bytes, tests.whole_files));
        match (total.0, found) {
            (0, None) => {},
            (0, Some(found)) => failures.push(format!(
                    "{tree}: {language} has a test row of {found:?} and no file of it holds any")),
            (_, found) if found != Some(*total) => failures.push(format!(
                    "{tree}: {language} expected (files, lines, bytes, whole files) of {total:?}, got {found:?}")),
            _ => {}
        }
    }
    for language in result.tests.keys() {
        if !totals.contains_key(language.as_str()) {
            failures.push(format!("{tree}: {language} has a test row and no section in {EXPECTED_FILE_NAME}"));
        }
    }

    let off = counted(false);
    if !off.tests.is_empty() {
        failures.push(format!("{tree}: test code was found with detection off"));
    }
    for (language, stats) in &result.per_language {
        if off.per_language.get(language).map(|of_off| of_off.lines) != Some(stats.lines) {
            failures.push(format!("{tree}: the lines of {language} move with the detection switch"));
        }
    }
    failures
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else if entry.file_name() != EXPECTED_FILE_NAME {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}
