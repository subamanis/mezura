// What '@name' stands for inside '--tests', a preset the core ships or one of the user's own, which
// is a file in the 'test-presets' folder of the data directory.
use mezura_core::{is_a_path_pattern, split_negation};
use mezura_core::warnings::{Code, Warning};
use mezura_core::{TestPreset, get_test_presets, validate_test_patterns};

use crate::config_files::{find_block_name, read_names_in_dir, strip_byte_order_mark};
use crate::message_printer::wrap_message;
use crate::warning_collector::{ProgramCode, ProgramWarning};

const PATTERNS_HEADING : &str = "===> tests";
const PRESET_FILE_EXTENSION : &str = "txt";
const UNUSABLE_IN_A_NAME : [char; 7] = ['/', '\\', ',', '!', '@', '{', '}'];

#[derive(Debug, PartialEq, Eq, Default)]
pub struct ExpandedPatterns {
    pub patterns: Vec<String>,
    // The names a preset wrote, which never warn for having matched nothing. A place it wrote warns
    // as a typed one does, since it is one project's folder.
    pub from_presets: Vec<String>
}

#[derive(Debug, PartialEq, Eq)]
pub enum PresetError {
    Unknown(String),
    // The sentence saying why ends in its own full stop
    Unusable { name: String, why: String }
}

impl PresetError {
    // Wrapped here, since a suggestion below the sentence keeps its own indentation
    pub fn format_message(&self, presets_dir: &str) -> String {
        let reason = wrap_message(&self.format_reason(presets_dir));
        let Self::Unknown(name) = self else { return reason };
        let (shipped, yours) = collect_names_to_offer(presets_dir);
        let candidates = shipped.iter().copied().chain(yours.iter().map(String::as_str)).collect::<Vec<_>>();
        match crate::suggestions::format_suggestion(name, &candidates) {
            Some(suggestion) => format!("{reason}\n\n{suggestion}"),
            None => reason
        }
    }

    // Unwrapped and with no suggestion, for a message that puts words of its own around it
    pub fn format_reason(&self, presets_dir: &str) -> String {
        match self {
            Self::Unknown(name) => {
                let (shipped, yours) = collect_names_to_offer(presets_dir);
                let and_yours = if yours.is_empty() {String::new()} else {format!(", and yours are {}", yours.join(", "))};
                format!("'@{name}' names no test preset. The shipped ones are {}{and_yours}.", shipped.join(", "))
            },
            Self::Unusable { name, why } => format!("The test preset '@{name}' could not be used. {why}")
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct UserPreset {
    pub description: String,
    pub patterns: Vec<String>
}

// A '!' in front takes the preset back by putting one before each of its patterns. A pattern that
// already takes something back is dropped, since negating it again would declare what it took back.
pub fn expand_presets(written: Vec<String>, presets_dir: &str) -> Result<ExpandedPatterns, PresetError> {
    expand_with(written, |name| match find_shipped_preset(name) {
        Some(preset) => Ok(copy_patterns_of(preset)),
        None => read_user_preset(presets_dir, name).map(|preset| preset.patterns)
    })
}

pub fn read_user_preset(presets_dir: &str, name: &str) -> Result<UserPreset, PresetError> {
    if !can_name_a_preset(name) {
        return Err(PresetError::Unknown(name.to_owned()));
    }
    let path = build_preset_path(presets_dir, name);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Err(PresetError::Unknown(name.to_owned())),
        Err(error) => return Err(build_unusable(name, format!("'{path}' could not be read: {error}.")))
    };
    let lines = strip_byte_order_mark(&text).lines().collect::<Vec<_>>();
    let Some(heading) = lines.iter().position(|line| find_block_name(line) == Some(crate::config_manager::TESTS)) else {
        return Err(build_unusable(name, format!("'{path}' has no '{PATTERNS_HEADING}' line for its patterns to go under.")));
    };
    let written = lines[heading + 1..].iter().take_while(|line| !line.trim().is_empty())
            .flat_map(|line| crate::args::parse_paths_to_vec(line)).collect::<Vec<_>>();
    if written.is_empty() {
        return Err(build_unusable(name, format!("'{path}' holds no pattern under '{PATTERNS_HEADING}'.")));
    }
    let expanded = expand_with(written, |inner| find_shipped_preset(inner).map(copy_patterns_of)
            .ok_or_else(|| build_unusable(name, format!("It names '@{inner}', and a preset of your own \
                    can name only a shipped one."))))?;
    validate_test_patterns(&expanded.patterns).map_err(|error| build_unusable(name, error.to_string()))?;

    Ok(UserPreset { description: lines[..heading].join("\n").trim().to_owned(), patterns: expanded.patterns })
}

pub fn write_user_preset(presets_dir: &str, name: &str, patterns: &[String]) -> std::io::Result<String> {
    std::fs::create_dir_all(presets_dir)?;
    let path = build_preset_path(presets_dir, name);
    std::fs::write(&path, format!("{PATTERNS_HEADING}\n{}\n", patterns.join("\n")))?;

    Ok(path)
}

pub fn collect_user_preset_names(presets_dir: &str) -> Vec<String> {
    read_names_in_dir(presets_dir, Some(PRESET_FILE_EXTENSION))
}

pub fn is_a_shipped_name(name: &str) -> bool {
    find_shipped_preset(name).is_some()
}

// A name that can be typed back as '@name' inside a comma separated list of patterns
pub fn can_name_a_preset(name: &str) -> bool {
    !name.is_empty() && !name.contains(UNUSABLE_IN_A_NAME) && !name.contains(char::is_whitespace)
}

pub fn collect_shadowed_file_warnings(presets_dir: &str) -> Vec<ProgramWarning> {
    collect_user_preset_names(presets_dir).into_iter().filter(|name| is_a_shipped_name(name)).map(|name| {
        let path = build_preset_path(presets_dir, &name);
        ProgramWarning::new(ProgramCode::PresetFileShadowed, &path, format!("'{path}' is never read, since '@{}' names a shipped \
                test preset. Give the file another name to use it as a preset of your own.", name.to_lowercase()))
    }).collect()
}

pub fn is_about_a_preset_pattern(warning: &Warning, from_presets: &[String]) -> bool {
    warning.code == Code::PatternMatchedNothing && from_presets.contains(&warning.subject)
}

fn expand_with(written: Vec<String>, find: impl Fn(&str) -> Result<Vec<String>, PresetError>)
-> Result<ExpandedPatterns, PresetError>
{
    const PRESET_SIGIL : char = '@';
    let mut expanded = ExpandedPatterns::default();
    for pattern in written {
        let (negation, text) = split_negation(&pattern);
        let Some(name) = text.strip_prefix(PRESET_SIGIL) else {
            expanded.patterns.push(pattern);
            continue;
        };
        for named in find(name)? {
            let added = if negation.is_empty() {
                named
            } else {
                match split_negation(&named) {
                    ("", rest) => format!("{negation}{rest}"),
                    _ => continue
                }
            };
            if !is_a_path_pattern(&added) {
                expanded.from_presets.push(added.clone());
            }
            expanded.patterns.push(added);
        }
    }

    Ok(expanded)
}

fn collect_names_to_offer(presets_dir: &str) -> (Vec<&'static str>, Vec<String>) {
    let shipped = get_test_presets().iter().map(|preset| preset.name).collect();
    let yours = collect_user_preset_names(presets_dir).into_iter().filter(|name| !is_a_shipped_name(name)).collect();
    (shipped, yours)
}

// Without regard to case, since on Windows and macOS 'Jest.txt' is the file 'jest.txt' would be
fn find_shipped_preset(name: &str) -> Option<&'static TestPreset> {
    get_test_presets().iter().find(|preset| preset.name.eq_ignore_ascii_case(name))
}

fn copy_patterns_of(preset: &TestPreset) -> Vec<String> {
    preset.patterns.iter().map(|pattern| (*pattern).to_owned()).collect()
}

fn build_unusable(name: &str, why: String) -> PresetError {
    PresetError::Unusable { name: name.to_owned(), why }
}

fn build_preset_path(presets_dir: &str, name: &str) -> String {
    format!("{presets_dir}{name}.{PRESET_FILE_EXTENSION}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn written(patterns: &[&str]) -> Vec<String> {
        patterns.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn a_preset_is_written_out_where_it_stands_and_a_bang_before_it_takes_each_of_its_patterns_back() {
        let dir = std::env::temp_dir().join("mezura-test-presets-expansion");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dir = crate::paths::normalise_separators(&dir.to_string_lossy()).into_owned() + "/";
        std::fs::write(dir.clone() + "mine.txt", "Our layout.\n\n===> tests\nspec/, !spec/fixtures/\n@xt\n").unwrap();
        std::fs::write(dir.clone() + "nested.txt", "===> tests\n@mine\n").unwrap();
        std::fs::write(dir.clone() + "placed.txt", "===> tests\n./spec/, conftest.py\n").unwrap();

        let expanded = expand_presets(written(&["@pytest", "!tests/data/", "**/@types/"]), &dir).unwrap();
        assert_eq!(written(&["test_*.py", "*_test.py", "conftest.py", "!tests/data/", "**/@types/"]), expanded.patterns,
                "a preset was not written out in its place, or a pattern with '@' inside it was read as one");
        assert_eq!(written(&["test_*.py", "*_test.py", "conftest.py"]), expanded.from_presets);
        assert_eq!(written(&["conftest.py"]), expand_presets(written(&["@placed"]), &dir).unwrap().from_presets,
                "a place a preset wrote would never say that it is missing");
        assert_eq!(written(&["!*Tests/", "!*.Test/"]), expand_presets(written(&["!@dotnet"]), &dir).unwrap().patterns);
        std::fs::write(dir.clone() + "Jest.txt", "===> tests\nmine/\n").unwrap();
        assert_eq!(copy_patterns_of(find_shipped_preset("jest").unwrap()), expand_presets(written(&["@Jest"]), &dir).unwrap().patterns,
                "a file of the user's own was read under a shipped name, which '--show-presets' says never happens");
        assert_eq!(vec![dir.clone() + "Jest.txt"], collect_shadowed_file_warnings(&dir).into_iter()
                .map(|warning| warning.subject).collect::<Vec<_>>());

        assert_eq!(written(&["spec/", "!spec/fixtures/", "xt/"]), expand_presets(written(&["@mine"]), &dir).unwrap().patterns);
        assert_eq!(written(&["!spec/", "!xt/"]), expand_presets(written(&["!@mine"]), &dir).unwrap().patterns,
                "taking a preset back declared what it took back");
        assert_eq!("Our layout.", read_user_preset(&dir, "mine").unwrap().description);
        write_user_preset(&dir, "saved", &written(&["xt/", "!xt/smoke/"])).unwrap();
        assert_eq!(written(&["xt/", "!xt/smoke/"]), expand_presets(written(&["@saved"]), &dir).unwrap().patterns,
                "a saved preset did not read back as what was saved");

        assert_eq!(Err(PresetError::Unknown("nope".to_owned())), expand_presets(written(&["@nope"]), &dir));
        assert_eq!(Err(PresetError::Unknown("../mine".to_owned())), expand_presets(written(&["@../mine"]), &dir));
        assert!(matches!(expand_presets(written(&["@nested"]), &dir), Err(PresetError::Unusable { .. })),
                "a preset of the user's own named another one of theirs");

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
