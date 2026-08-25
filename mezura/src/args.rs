// Turning the text of a command line or a configuration file into values.
use std::collections::HashMap;

use mezura_core::{ForcedLanguages, format_module_scope, split_off_module_scope};

use crate::config_manager::ExplainedLines;

// A command line, where a space separates one target from the next only once a module is named. It
// cannot separate them unconditionally: by the time the arguments arrive the shell has split them
// and eaten the quotes, so a space inside a path and a space between two paths look identical, and
// whoever names no module needs a path with a space in it to go on working.
pub fn parse_targets(s: &str) -> Result<Vec<(Option<String>, String)>, String> {
    let separated = split_targets(s, char::is_whitespace);
    let declares_a_module = separated.iter().flat_map(|token| token.split(','))
            .any(|piece| split_off_module_name(piece.trim()).is_some());

    if declares_a_module {
        targets_of(separated)
    } else {
        targets_of(vec![s.to_owned()])
    }
}

// The '===> targets' block of a configuration file, where the line separates one target from the
// next and a space never does. That is the way out of the one thing a command line cannot express:
// a spaced path in a run that names modules. A trailing comma continues the list onto the next line.
pub fn parse_targets_in_block(block: &str) -> Result<Vec<(Option<String>, String)>, String> {
    targets_of(split_targets(block, |character| character == '\n'))
}

// 'rust,ios/swift'. Kept in the written form, module and all, so that a configuration file's block
// can be read one line at a time and turned into the scoped value once at the end. Only the language
// itself is lowercased: a module is matched exactly, the way the target that declares it is.
pub fn parse_languages_to_vec(s: &str) -> Vec<String> {
    s.split(',').filter_map(|entry| {
        let (module, name) = split_off_module_scope(entry.trim());
        Some(format_module_scope(module, &get_trimmed_if_not_empty(&remove_dot_prefix(name.trim()).to_lowercase())?))
    }).collect::<Vec<_>>()
}

pub fn parse_paths_to_vec(s: &str) -> Vec<String> {
    s.split(',').filter_map(cleaned_path).collect::<Vec<_>>()
}

// 'm=matlab,.pl=perl,Makefile=make', and 'ios/m=objective-c' for a rule that holds inside one
// module only. Lowercased here so the map is keyed the way the lookup asks for it, except for the
// module, which is matched exactly as the target that declares it is. The leading dot is left alone,
// which is what lets a whole filename be named at all: the extension side strips it when it keys its
// map, while '.gitignore' keeps every dot it was written with, being a name and not an extension.
pub fn parse_forced_languages(s: &str) -> Option<ForcedLanguages> {
    let mut written = HashMap::new();
    for pair in s.split(',').filter_map(get_trimmed_if_not_empty) {
        let (claimed, language) = pair.split_once('=')?;
        let (module, claimed) = split_off_module_scope(claimed.trim());
        let (module, claimed, language) = (module.map(str::trim), claimed.trim(), language.trim());
        // A slash with nothing on one side of it is a module with no extension after it, or an
        // extension with no module before it, and neither is a rule anybody can act on.
        if claimed.is_empty() || language.is_empty() || claimed.contains('/')
                || module.is_some_and(str::is_empty) {
            return None;
        }
        written.insert(format_module_scope(module, &claimed.to_lowercase()), language.to_owned());
    }

    if written.is_empty() {None} else {Some(ForcedLanguages::of_written_form(&written))}
}

// Sorted, because this is written into config files and into the log entries a later run compares
// itself against, and a map's order would make the same setting look like a changed one
pub fn forced_languages_to_string(forced: &ForcedLanguages) -> String {
    let mut pairs = forced.to_written_form().into_iter()
            .map(|(claimed, language)| format!("{claimed}={language}")).collect::<Vec<_>>();
    pairs.sort();
    pairs.join(",")
}

// '--explain' with nothing after it, one line number, or two with '..' between them, which is what
// separates a range everywhere else in the program: '--diff v2..v3' and a theme's 'red..blue'. An
// end left off is open, and a last line past the end of the file is not a mistake, since whoever
// asks does not know how long the file is.
pub fn parse_explained_lines(s: &str) -> Option<ExplainedLines> {
    let text = s.trim();
    if text.is_empty() {
        return Some(ExplainedLines::WHOLE_FILE);
    }

    let (first, last) = match text.split_once("..") {
        Some((first, last)) => (parse_end_of_a_range(first, 1)?, parse_end_of_a_range(last, usize::MAX)?),
        None => {
            let only = text.trim().parse::<usize>().ok()?;
            (only, only)
        }
    };
    if first == 0 || last < first {
        return None;
    }

    Some(ExplainedLines {first, last})
}

pub fn parse_usize_value(s: &str, min: usize, max: usize) -> Option<usize> {
    if let Ok(num) = s.trim().parse::<usize>() {
        if num <= max && num >= min {
            Some(num)
        } else {
            None
        }
    } else {
        None
    }
}

pub fn parse_two_usize_values(s: &str, min1: usize, max1: usize, min2: usize, max2: usize) -> Option<(usize,usize)> {
    let elements = s.split_whitespace().filter_map(get_trimmed_if_not_empty).collect::<Vec<_>>();
    if elements.len() != 2 {
        return None
    }

    if let Ok(val1) = elements[0].parse::<usize>()
        && let Ok(val2) = elements[1].parse::<usize>()
        && val1 >= min1 && val1 <= max1 && val2 >= min2 && val2 <= max2 {
        return Some((val1,val2));
    }
    
    None
}

pub fn get_trimmed_if_not_empty(str: &str) -> Option<String> {
    let str = str.trim();
    if str.is_empty() {None}
    else {Some(str.to_owned())}
}

// Where a command begins: '--' at the start of the line or after whitespace. A '--' inside a word
// belongs to the word, since folder names that encode a hierarchy carry them, and splitting on the
// substring would cut such a target into a piece that does not exist and a command that does not
// parse. The pieces come back shaped as `line.split("--")` shapes them: one leading piece before any
// command, then one piece per command.
pub fn split_into_command_segments(line: &str) -> Vec<&str> {
    let bytes = line.as_bytes();
    let mut boundaries = Vec::new();
    let mut at_token_start = true;
    let mut i = 0;
    while i < bytes.len() {
        if at_token_start && bytes[i..].starts_with(b"--") {
            boundaries.push(i);
            i += 2;
            at_token_start = false;
            continue;
        }
        // Multibyte characters have no ASCII whitespace inside them, so walking bytes cannot split
        // one, and '-' itself is ASCII
        at_token_start = bytes[i].is_ascii_whitespace();
        i += 1;
    }

    let mut segments = Vec::with_capacity(boundaries.len() + 1);
    segments.push(&line[..boundaries.first().copied().unwrap_or(line.len())]);
    for (position, boundary) in boundaries.iter().enumerate() {
        let end = boundaries.get(position + 1).copied().unwrap_or(line.len());
        segments.push(&line[boundary + 2..end]);
    }
    segments
}

// The position of '--name' written as a command: after whitespace or at the start, and with
// whitespace or the end after the name, so that a path containing the text never matches and
// '--help' does not answer for '--helpme'.
pub fn find_command(line: &str, name: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(found) = line[from..].find("--") {
        let at = from + found;
        let starts_a_token = at == 0 || line[..at].chars().last().is_some_and(char::is_whitespace);
        let after_marker = &line[at + 2..];
        if starts_a_token && after_marker.starts_with(name) {
            let after_name = &after_marker[name.len()..];
            if after_name.is_empty() || after_name.chars().next().is_some_and(char::is_whitespace) {
                return Some(at);
            }
        }
        from = at + 2;
    }
    None
}

// A name holds for the rest of the comma list it opened, so 'frontend=./web,./ui' is one module of
// two directories, and it stops where that list ends so nothing after a named target joins it by
// accident. Inside the list a name still starts a new one, which is what lets a saved configuration
// write 'frontend=./web,backend=./api' and read it back as the two targets it was. The error is the
// piece that could not be read, always a name with nothing after it.
fn parse_end_of_a_range(text: &str, when_left_off: usize) -> Option<usize> {
    let text = text.trim();
    if text.is_empty() {
        return Some(when_left_off);
    }

    text.parse::<usize>().ok()
}

fn targets_of(tokens: Vec<String>) -> Result<Vec<(Option<String>, String)>, String> {
    let mut targets = Vec::new();
    for token in tokens {
        let mut module: Option<String> = None;
        for piece in token.split(',') {
            let piece = piece.trim();
            if piece.is_empty() {
                continue;
            }

            let path = match split_off_module_name(piece) {
                Some((name, path)) => {
                    module = Some(name.to_owned());
                    path
                },
                None => piece
            };

            match cleaned_path(path) {
                Some(path) => targets.push((module.clone(), path)),
                None => return Err(piece.to_owned())
            }
        }
    }

    Ok(targets)
}

// One target ends and the next begins at a separator, while a comma continues the list of paths
// that belong to the same one. A comma with a gap around it is still a comma, so that gap does not
// separate: 'tests=./a, ./b' keeps both paths under 'tests' instead of losing the second one to
// nobody. Which character separates is the caller's to say, since only the caller knows where the
// text came from.
fn split_targets(s: &str, is_separator: impl Fn(char) -> bool) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut gap = false;

    for character in s.chars() {
        if character == '"' {
            in_quotes = !in_quotes;
        } else if !in_quotes && is_separator(character) {
            gap = !current.is_empty();
            continue;
        }

        if gap {
            gap = false;
            if character != ',' && !current.ends_with(',') {
                tokens.push(std::mem::take(&mut current));
            }
        }
        current.push(character);
    }
    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

// 'frontend=./web'. The name is whatever comes before the first '=', and only when it could not be
// part of a path itself: '=' is a legal character in a file name on Linux, so anything that looks
// like a path or a glob pattern is left alone and read as one.
fn split_off_module_name(piece: &str) -> Option<(&str, &str)> {
    let (name, path) = piece.split_once('=')?;
    let name = name.trim();
    if name.is_empty() || name.contains(['/', '\\', '.', '*', '?', '[', ']', '{', '}']) {
        return None;
    }

    Some((name, path))
}

fn cleaned_path(piece: &str) -> Option<String> {
    let cleansed = &super::paths::normalise_separators(piece.trim()).into_owned();
    get_trimmed_if_not_empty(cleansed.strip_prefix('"').unwrap_or(cleansed).strip_suffix('"').unwrap_or(cleansed))
}

fn remove_dot_prefix(str: &str) -> &str {
    if let Some(stripped) = str.strip_prefix('.') {
        stripped
    } else {
        str
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_begin_at_token_boundaries_and_nowhere_else() {
        assert_eq!(vec!["./src ", "threads 2 8 ", "top 5"],
                split_into_command_segments("./src --threads 2 8 --top 5"));
        assert_eq!(vec!["", "help"], split_into_command_segments("--help"));
        assert_eq!(vec!["./src"], split_into_command_segments("./src"));

        assert_eq!(vec!["C:/t/D--dev-Rusty-mezura/scratch"],
                split_into_command_segments("C:/t/D--dev-Rusty-mezura/scratch"));
        assert_eq!(vec!["C:/t/my--project ", "threads 1 2"],
                split_into_command_segments("C:/t/my--project --threads 1 2"));
        assert_eq!(vec!["", "targets C:/t/a--b, C:/t/c--d ", "top 3"],
                split_into_command_segments("--targets C:/t/a--b, C:/t/c--d --top 3"));
    }

    #[test]
    fn a_command_is_found_only_as_a_whole_word() {
        assert_eq!(Some(0), find_command("--help", "help"));
        assert_eq!(Some(6), find_command("./src --help style", "help"));
        assert_eq!(None, find_command("./a--version-dir", "version"));
        assert_eq!(None, find_command("./x/some--help-docs", "help"));
        assert_eq!(None, find_command("--helpme", "help"));
        assert_eq!(None, find_command("--save-theme dark", "save"));
    }

    #[test]
    pub fn a_list_of_language_names_drops_the_empty_entries_and_the_spaces_around_each() {
        assert_eq!(Vec::<String>::new(), parse_languages_to_vec(","));
        assert_eq!(Vec::<String>::new(), parse_languages_to_vec(""));
        assert_eq!(Vec::<String>::new(), parse_languages_to_vec("  "));

        assert_eq!(vec!["a".to_owned(),"b".to_owned()], parse_languages_to_vec("a,b"));
        assert_eq!(vec!["a".to_owned(),"b".to_owned()], parse_languages_to_vec("  a ,  b "));
        assert_eq!(vec!["a".to_owned(),"b".to_owned()], parse_languages_to_vec(".A,.b "));
    }

    #[test]
    pub fn a_list_of_paths_is_split_on_commas_with_its_backslashes_turned_round() {
        assert_eq!(vec!["a/a".to_owned(),"b/b".to_owned()], parse_paths_to_vec("a\\a,b\\b"));
        assert_eq!(vec!["a".to_owned(),"b/b".to_owned()], parse_paths_to_vec(" a  ,  b\\b "));
    }

    fn targets(s: &str) -> Vec<(Option<String>, String)> {
        parse_targets(s).unwrap()
    }

    #[test]
    pub fn a_space_separates_targets_only_once_one_of_them_is_named() {
        let named = |name: &str, path: &str| (Some(name.to_owned()), path.to_owned());
        let plain = |path: &str| (None, path.to_owned());

        assert_eq!(vec![plain("C:/Users/John Smith/proj")], targets("C:/Users/John Smith/proj"));
        assert_eq!(vec![plain("a/a"), plain("b/b")], targets("a\\a, b\\b"));
        assert_eq!(vec![plain("./src ./tests")], targets("./src ./tests"));

        assert_eq!(vec![named("frontend", "./web"), named("backend", "./api")],
                targets("frontend=./web backend=./api"));
        assert_eq!(vec![plain("./project"), named("tests", "./project/tests")],
                targets("./project tests=./project/tests"));
        assert_eq!(vec![named("tests", "./project/tests"), plain("./project")],
                targets("tests=./project/tests ./project"));
        assert_eq!(vec![named("f", "./web"), named("f", "./ui"), named("b", "./api")],
                targets("f=./web,./ui b=./api"));
        assert_eq!(vec![named("f", "./web"), named("f", "./ui")], targets("f=./web, ./ui"));
    }

    #[test]
    pub fn a_line_is_what_ends_a_target_in_a_configuration_block() {
        let parsed = |s: &str| parse_targets_in_block(s).unwrap();
        assert_eq!(vec![(Some("frontend".to_owned()), "C:/my path/web".to_owned()),
                        (Some("backend".to_owned()), "C:/my path/api".to_owned())],
                parsed("frontend=C:/my path/web\nbackend=C:/my path/api"));

        assert_eq!(vec![(None, "C:/Users/John Smith/proj".to_owned()), (None, "D:/other".to_owned())],
                parsed("C:/Users/John Smith/proj,D:/other"));

        assert_eq!(vec![(Some("tests".to_owned()), "./api/tests".to_owned()),
                        (Some("tests".to_owned()), "./web/tests".to_owned())],
                parsed("tests=./api/tests,\n./web/tests"));
    }

    #[test]
    pub fn a_number_outside_its_range_or_not_a_whole_number_at_all_is_refused() {
        assert_eq!(None,parse_usize_value("0", 1, 8));
        assert_eq!(None,parse_usize_value("9", 1, 8));
        assert_eq!(None,parse_usize_value("0.2", 1, 8));
        assert_eq!(None,parse_usize_value("-1", 1, 8));
        assert_eq!(None,parse_usize_value("", 1, 8));
        assert_eq!(None,parse_usize_value(" ", 1, 8));
        assert_eq!(None,parse_usize_value("A", 1, 8));
        assert_eq!(Some(1),parse_usize_value("1", 1, 8));
        assert_eq!(Some(8),parse_usize_value("   8 ", 1, 8));

        assert_eq!(None,parse_two_usize_values("A", 1, 4, 1, 12));
        assert_eq!(None,parse_two_usize_values("A A", 1, 4, 1, 12));
        assert_eq!(None,parse_two_usize_values("1 A", 1, 4, 1, 12));
        assert_eq!(None,parse_two_usize_values("1 0", 1, 4, 1, 12));
        assert_eq!(None,parse_two_usize_values("5 12", 1, 4, 1, 12));
        assert_eq!(None,parse_two_usize_values("4 13", 1, 4, 1, 12));
        assert_eq!(Some((1,1)),parse_two_usize_values("1 1", 1, 4, 1, 12));
        assert_eq!(Some((1,1)),parse_two_usize_values("     1       1  ", 1, 4, 1, 12));
        assert_eq!(Some((4,12)),parse_two_usize_values("4 12", 1, 4, 1, 12));
        assert_eq!(Some((2,6)),parse_two_usize_values("2 6", 1, 4, 1, 12));
    }

    // '..' and not a dash or a space: the program separates a range that way in '--diff v2..v3' and
    // in a theme's 'red..blue', while a space between two numbers means two settings ('--threads').
    #[test]
    fn a_range_of_lines_is_written_the_way_every_other_range_in_the_program_is() {
        let range = |first, last| Some(ExplainedLines {first, last});

        assert_eq!(Some(ExplainedLines::WHOLE_FILE), parse_explained_lines(""));
        assert_eq!(range(10, 20), parse_explained_lines("10..20"));
        assert_eq!(range(13, 13), parse_explained_lines("13"));
        assert_eq!(range(10, usize::MAX), parse_explained_lines("10.."));
        assert_eq!(range(1, 20), parse_explained_lines("..20"));

        assert_eq!(None, parse_explained_lines("20..10"));
        assert_eq!(None, parse_explained_lines("0..10"));
        assert_eq!(None, parse_explained_lines("10-20"));
        assert_eq!(None, parse_explained_lines("10 20"));
        assert_eq!(None, parse_explained_lines("1..2..3"));
        assert_eq!(None, parse_explained_lines("ten"));
    }
}
