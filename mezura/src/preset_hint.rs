use std::path::{Path, PathBuf};

use mezura_core::RunResult;

// A file that says which ecosystem a project is in without saying where its tests are. A framework
// that bundles a tool names it too, since a Rails Gemfile never says minitest.
const GENERIC_PROJECT_FILES : [GenericProjectFile; 4] = [
    GenericProjectFile { file_name: "package.json",
            words_by_name_of_preset: &[("jest", &["jest", "react-scripts"]), ("vitest", &["vitest"]), ("mocha", &["mocha"])],
            names_of_languages: &["JavaScript", "TypeScript", "JSX", "TSX"], declares_workspace_members: true },
    GenericProjectFile { file_name: "pyproject.toml",
            words_by_name_of_preset: &[("pytest", &["pytest"]), ("unittest", &["unittest", "django"])],
            names_of_languages: &["Python"], declares_workspace_members: false },
    GenericProjectFile { file_name: "Gemfile",
            words_by_name_of_preset: &[("rspec", &["rspec"]), ("minitest", &["minitest", "rails"])],
            names_of_languages: &["Ruby"], declares_workspace_members: false },
    GenericProjectFile { file_name: "composer.json", words_by_name_of_preset: &[("phpunit", &["phpunit"])],
            names_of_languages: &["PHP"], declares_workspace_members: false },
];
const WORKSPACE_GLOBSTAR_DEPTH : usize = 3;

// The file is read last, so a run that would print no hint reads nothing
pub fn find_names_of_presets_to_suggest(result: &RunResult) -> Vec<Vec<&'static str>> {
    GENERIC_PROJECT_FILES.iter().filter_map(|generic| {
        let counted = generic.names_of_languages.iter()
                .any(|name_of_language| result.per_language.contains_key(*name_of_language));
        let tests_found = generic.names_of_languages.iter()
                .any(|name_of_language| result.tests.get(*name_of_language).is_some_and(|x| x.stats.lines > 0))
                || result.applied_presets.iter().any(|preset| preset.matched
                        && generic.words_by_name_of_preset.iter().any(|(name, _)| *name == preset.name.as_str()));
        if !counted || tests_found {
            return None;
        }
        let contents = result.targets.iter().flat_map(|target| read_project_files_of(Path::new(&target.path), generic))
                .collect::<Vec<_>>();
        let named = generic.words_by_name_of_preset.iter()
                .filter(|(_, words)| words.iter().any(|word| contents.iter().any(|text| contains_word(text, word))))
                .map(|(name, _)| *name).collect::<Vec<_>>();
        (!named.is_empty()).then_some(named)
    }).collect()
}

struct GenericProjectFile {
    file_name: &'static str,
    words_by_name_of_preset: &'static [(&'static str, &'static [&'static str])],
    names_of_languages: &'static [&'static str],
    declares_workspace_members: bool
}

// Only the declared members are read, since a search of the folders reaches node_modules and test fixtures
fn read_project_files_of(target: &Path, generic: &GenericProjectFile) -> Vec<Vec<u8>> {
    let Ok(own) = std::fs::read(target.join(generic.file_name)) else { return Vec::new() };
    let members = match generic.declares_workspace_members {
        true => find_workspace_members(target, &own),
        false => Vec::new()
    };
    let of_members = members.iter().filter_map(|member| std::fs::read(member.join(generic.file_name)).ok()).collect::<Vec<_>>();

    std::iter::once(own).chain(of_members).collect()
}

fn find_workspace_members(root: &Path, package_json: &[u8]) -> Vec<PathBuf> {
    let mut patterns = Vec::new();
    let package_json = package_json.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(package_json);
    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(package_json)
            && let Some(declared) = json.get("workspaces").map(|declared| declared.get("packages").unwrap_or(declared))
            && let Some(listed) = declared.as_array() {
        patterns.extend(listed.iter().filter_map(|pattern| pattern.as_str().map(str::to_owned)));
    }
    if let Ok(yaml) = std::fs::read_to_string(root.join("pnpm-workspace.yaml")) {
        patterns.extend(parse_pnpm_package_patterns(crate::config_files::strip_byte_order_mark(&yaml)));
    }
    let (excluded, included): (Vec<&String>, Vec<&String>) = patterns.iter().partition(|pattern| pattern.starts_with('!'));
    let excluded = excluded.iter().flat_map(|pattern| expand_directory_pattern(root, &pattern[1..])).collect::<Vec<_>>();
    let mut members = included.iter().flat_map(|pattern| expand_directory_pattern(root, pattern))
            .filter(|member| !excluded.contains(member)).collect::<Vec<_>>();
    members.sort();
    members.dedup();

    members
}

fn parse_pnpm_package_patterns(yaml: &str) -> Vec<String> {
    let unquote = |item: &str| item.trim().trim_matches(['\'', '"']).to_owned();
    let mut patterns = Vec::new();
    let mut in_packages = false;
    for line in yaml.lines() {
        let line = line.split(" #").next().unwrap_or_default().trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if !line.starts_with([' ', '\t', '-']) {
            let listed = line.strip_prefix("packages:");
            in_packages = listed.is_some();
            if let Some(on_one_line) = listed.and_then(|rest| rest.trim().strip_prefix('[')?.strip_suffix(']')) {
                patterns.extend(on_one_line.split(',').map(unquote).filter(|item| !item.is_empty()));
            }
        } else if in_packages && let Some(item) = line.trim_start().strip_prefix('-') {
            patterns.push(unquote(item));
        }
    }

    patterns
}

// A '**' matches no folder as well as several, so 'packages/**' takes 'packages' itself
fn expand_directory_pattern(root: &Path, pattern: &str) -> Vec<PathBuf> {
    let mut found = vec![root.to_path_buf()];
    for segment in pattern.split('/').filter(|segment| !segment.is_empty() && *segment != ".") {
        found = found.into_iter().flat_map(|directory| match segment {
            "**" => std::iter::once(directory.clone()).chain(list_subdirectories(&directory, WORKSPACE_GLOBSTAR_DEPTH))
                    .collect::<Vec<_>>(),
            _ if segment.contains('*') => list_subdirectories(&directory, 1).into_iter()
                    .filter(|child| child.file_name().and_then(|name| name.to_str())
                            .is_some_and(|name| matches_name_pattern(name, segment)))
                    .collect(),
            _ => vec![directory.join(segment)]
        }).collect();
    }

    found
}

fn list_subdirectories(directory: &Path, depth: usize) -> Vec<PathBuf> {
    if depth == 0 {
        return Vec::new();
    }
    let children = std::fs::read_dir(directory).map(|entries| entries.flatten().map(|entry| entry.path())
            .filter(|path| path.is_dir() && path.file_name().and_then(|name| name.to_str())
                    .is_some_and(|name| !name.starts_with('.') && name != "node_modules"))
            .collect::<Vec<_>>()).unwrap_or_default();
    let below = children.iter().flat_map(|child| list_subdirectories(child, depth - 1)).collect::<Vec<_>>();

    children.into_iter().chain(below).collect()
}

fn matches_name_pattern(name: &str, pattern: &str) -> bool {
    match pattern.split_once('*') {
        None => name == pattern,
        Some((head, rest)) => name.strip_prefix(head).is_some_and(|after| (0..=after.len())
                .filter(|&at| after.is_char_boundary(at)).any(|at| matches_name_pattern(&after[at..], rest)))
    }
}

// '-' and '_' join a word, so 'eslint-plugin-jest' and '@testing-library/jest-dom' do not name jest
fn contains_word(text: &[u8], word: &str) -> bool {
    let word = word.as_bytes();
    let is_word_byte = |byte: &u8| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'-';
    text.windows(word.len()).enumerate().any(|(at, window)| window.eq_ignore_ascii_case(word)
            && !text[..at].last().is_some_and(is_word_byte)
            && !text.get(at + word.len()).is_some_and(is_word_byte))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use mezura_core::Target;

    use super::*;

    #[test]
    fn a_workspace_root_names_the_tools_of_the_members_it_declares_and_of_nothing_else() {
        let root = std::env::temp_dir().join("mezura-workspace-members");
        let _ = std::fs::remove_dir_all(&root);
        let write = |path: &str, contents: &str| {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, contents).unwrap();
        };
        let suggested_at = |folder: &str| {
            let typescript = crate::test_support::plain_stats_of(1, 40, 4, 3, 1, HashMap::new());
            let result = crate::test_support::plain_result_of(HashMap::from([("TypeScript".to_owned(), typescript)]),
                    Vec::new(), vec![Target::of(crate::paths::normalise_separators(&root.join(folder).to_string_lossy()))]);
            find_names_of_presets_to_suggest(&result)
        };
        write("npm/package.json", r#"{"workspaces": ["packages/*", "!packages/legacy"], "devDependencies": {"prettier": "^3"}}"#);
        write("npm/packages/web/package.json", r#"{"devDependencies": {"jest": "^29"}}"#);
        write("npm/packages/legacy/package.json", r#"{"devDependencies": {"mocha": "^10"}}"#);
        write("npm/packages/cli/package.json", r#"{"scripts": {"test": "tap"}}"#);
        write("npm/examples/demo/package.json", r#"{"devDependencies": {"vitest": "^2"}}"#);
        write("pnpm/package.json", r#"{"private": true}"#);
        write("pnpm/pnpm-workspace.yaml", "packages:\n# the apps\n  - 'apps/**'\n\n  - \"libs/*-core\" # the cores\n\
                onlyBuiltDependencies:\n  - esbuild\n");
        write("pnpm/apps/site/web/package.json", r#"{"devDependencies": {"vitest": "^2"}}"#);
        write("pnpm/apps/node_modules/mocha/package.json", r#"{"name": "mocha"}"#);
        write("pnpm/libs/date-core/package.json", r#"{"devDependencies": {"jest": "^29"}}"#);
        write("pnpm/libs/date-ui/package.json", r#"{"devDependencies": {"mocha": "^10"}}"#);
        write("pnpm/esbuild/package.json", r#"{"devDependencies": {"mocha": "^10"}}"#);
        write("globstar/package.json", r#"{"private": true}"#);
        write("globstar/pnpm-workspace.yaml", "packages:\n  - 'tools/*/**'\n");
        write("globstar/tools/lint/package.json", r#"{"devDependencies": {"mocha": "^10"}}"#);
        write("bom/package.json", "\u{feff}{\"workspaces\": [\"packages/*\"]}");
        write("bom/packages/web/package.json", r#"{"devDependencies": {"jest": "^29"}}"#);
        write("bompnpm/package.json", r#"{"private": true}"#);
        write("bompnpm/pnpm-workspace.yaml", "\u{feff}packages:\n  - 'packages/*'\n");
        write("bompnpm/packages/web/package.json", r#"{"devDependencies": {"jest": "^29"}}"#);
        write("oneline/package.json", r#"{"private": true}"#);
        write("oneline/pnpm-workspace.yaml", "packages: ['apps/*', \"libs/*\"] # all of them\n");
        write("oneline/apps/web/package.json", r#"{"devDependencies": {"jest": "^29"}}"#);
        write("oneline/libs/core/package.json", r#"{"devDependencies": {"mocha": "^10"}}"#);

        assert_eq!(vec![vec!["jest"]], suggested_at("npm"),
                "a member taken back by '!', a folder the root does not declare or a member running tap named a tool");
        write("npm/package.json", r#"{"workspaces": {"packages": ["packages/web"]}}"#);
        assert_eq!(vec![vec!["jest"]], suggested_at("npm"), "the members under 'workspaces.packages' were not read");
        assert_eq!(vec![vec!["jest", "vitest"]], suggested_at("pnpm"),
                "pnpm-workspace.yaml was misread past a comment, a blank line, a '**', a '*-core' or another key's list");
        assert_eq!(vec![vec!["mocha"]], suggested_at("globstar"), "a '**' did not take the folder it stands for itself");
        assert_eq!(vec![vec!["jest"]], suggested_at("bom"), "a package.json opening with a byte order mark lost its members");
        assert_eq!(vec![vec!["jest"]], suggested_at("bompnpm"),
                "a pnpm-workspace.yaml opening with a byte order mark lost its members");
        assert_eq!(vec![vec!["jest", "mocha"]], suggested_at("oneline"), "a list written on the line of 'packages:' was not read");

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_tool_is_named_only_by_a_whole_word_with_hyphens_and_underscores_inside_words() {
        assert!(contains_word(br#""jest": "^29""#, "jest"));
        assert!(contains_word(b"jest", "jest"), "a word filling the whole text was missed at an edge");
        assert!(contains_word(br#""@vitest/ui""#, "vitest"));
        assert!(contains_word(b"[tool.pytest.ini_options]", "pytest"));
        assert!(contains_word(br#""Django>=5""#, "django"), "the comparison was case sensitive");
        assert!(!contains_word(br#""eslint-plugin-jest": "^28""#, "jest"));
        assert!(!contains_word(br#""@testing-library/jest-dom""#, "jest"));
        assert!(!contains_word(br#"gem "rspec-rails""#, "rails"));
        assert!(!contains_word(b"pytest_asyncio", "pytest"));
    }
}
