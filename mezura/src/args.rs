// Turning the text of a command line or a configuration file into values.
use std::collections::HashMap;

use mezura_core::{ForcedLanguages, format_module_scope, split_off_module_scope};

use crate::config_manager::ExplainedLines;

// A token is one argument of the command line or one line of a configuration file's '===> targets'
// block. Commas inside it list paths, and a name holds for the paths after it up to the next name or
// the end of the token. The error is the piece that could not be read, always a name with nothing
// after it.
pub fn parse_targets(tokens: &[String]) -> Result<Vec<(Option<String>, String)>, String> {
    let mut targets = Vec::new();
    for token in tokens {
        let mut module: Option<String> = None;
        for piece in split_at_commas_outside_braces(token) {
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

            match clean_path(path) {
                Some(path) => targets.push((module.clone(), path)),
                None => return Err(piece.to_owned())
            }
        }
    }

    Ok(targets)
}

pub struct Command<'a> {
    pub name: &'a str,
    pub arguments: &'a [String]
}

// An argument that starts with '--' begins a command, and the arguments before the first one are
// the targets
pub fn split_into_commands(args: &[String]) -> (&[String], Vec<Command<'_>>) {
    let starts = args.iter().enumerate().filter(|(_, arg)| arg.starts_with("--")).map(|(i, _)| i).collect::<Vec<_>>();
    let leading = &args[..starts.first().copied().unwrap_or(args.len())];
    let commands = starts.iter().enumerate().map(|(position, &at)| {
        let end = starts.get(position + 1).copied().unwrap_or(args.len());
        Command { name: &args[at][2..], arguments: &args[at + 1..end] }
    }).collect();
    (leading, commands)
}

pub fn has_command(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg.strip_prefix("--") == Some(name))
}

pub fn find_argument_of<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let at = args.iter().position(|arg| arg.strip_prefix("--") == Some(name))?;
    args.get(at + 1).map(String::as_str)
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
    split_at_commas_outside_braces(s).filter_map(clean_path).collect::<Vec<_>>()
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
    s.trim().parse::<usize>().ok().filter(|num| (min..=max).contains(num))
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

fn parse_end_of_a_range(text: &str, when_left_off: usize) -> Option<usize> {
    let text = text.trim();
    if text.is_empty() {
        return Some(when_left_off);
    }

    text.parse::<usize>().ok()
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

fn clean_path(piece: &str) -> Option<String> {
    let cleansed = &super::paths::normalise_separators(piece.trim()).into_owned();
    get_trimmed_if_not_empty(cleansed.strip_prefix('"').unwrap_or(cleansed).strip_suffix('"').unwrap_or(cleansed))
}

// The comma of a glob's '{a,b}' belongs to the glob
fn split_at_commas_outside_braces(s: &str) -> impl Iterator<Item = &str> {
    let mut depth = 0u32;
    s.split(move |character: char| {
        match character {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ',' => return depth == 0,
            _ => ()
        }
        false
    })
}

fn remove_dot_prefix(str: &str) -> &str {
    str.strip_prefix('.').unwrap_or(str)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        crate::test_support::split_typed_line(line)
    }

    #[test]
    fn an_argument_opening_with_two_dashes_begins_a_command_and_nothing_else_does() {
        let described = |args: &[String]| {
            let (leading, commands) = split_into_commands(args);
            (leading.to_vec(), commands.iter().map(|c| (c.name.to_owned(), c.arguments.to_vec())).collect::<Vec<_>>())
        };
        let (leading, commands) = described(&args("./src --threads 2 8 --top 5"));
        assert_eq!(args("./src"), leading);
        assert_eq!(vec![("threads".to_owned(), args("2 8")), ("top".to_owned(), args("5"))], commands);

        let (leading, commands) = described(&args("--help"));
        assert!(leading.is_empty());
        assert_eq!(vec![("help".to_owned(), Vec::new())], commands);

        let (leading, commands) = described(&args("C:/t/my--project --threads 1 2"));
        assert_eq!(args("C:/t/my--project"), leading);
        assert_eq!(1, commands.len());

        let (_, commands) = described(&["--log".to_owned(), "a note --about it".to_owned()]);
        assert_eq!(vec![("log".to_owned(), vec!["a note --about it".to_owned()])], commands);

        assert!(has_command(&args("./src --help style"), "help"));
        assert!(!has_command(&args("./a--version-dir"), "version"));
        assert!(!has_command(&args("--helpme"), "help"));
        assert!(!has_command(&args("--save-theme dark"), "save"));
        assert_eq!(Some("full"), find_argument_of(&args("--changelog full"), "changelog"));
        assert_eq!(None, find_argument_of(&args("./src --output"), "output"));
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
        assert_eq!(vec!["a/a".to_owned(),"b/b".to_owned()], parse_paths_to_vec("a/a,b/b"));
        assert_eq!(vec!["a".to_owned(),"b/b".to_owned()], parse_paths_to_vec(" a  ,  b/b "));
        assert_eq!(vec!["*.{spec,test}.js".to_owned(), "src/{a,b}/".to_owned(), "c".to_owned()],
                parse_paths_to_vec("*.{spec,test}.js, src/{a,b}/,c"), "the comma of a glob's braces split the list");
        assert_eq!(vec!["a{b,c".to_owned()], parse_paths_to_vec("a{b,c"), "a brace left open takes the rest as one pattern");

        if cfg!(windows) {
            assert_eq!(vec!["a/a".to_owned(),"b/b".to_owned()], parse_paths_to_vec("a\\a,b\\b"));
        } else {
            assert_eq!(vec!["a\\a".to_owned(),"b\\b".to_owned()], parse_paths_to_vec("a\\a,b\\b"));
        }
    }

    #[test]
    pub fn each_token_is_one_target_and_a_name_covers_the_paths_after_it_in_its_own_token() {
        let targets = |tokens: &[&str]| parse_targets(&tokens.iter().map(|x| x.to_string()).collect::<Vec<_>>()).unwrap();
        let named = |name: &str, path: &str| (Some(name.to_owned()), path.to_owned());
        let plain = |path: &str| (None, path.to_owned());

        assert_eq!(vec![plain("C:/Users/John Smith/proj")], targets(&["C:/Users/John Smith/proj"]));
        assert_eq!(vec![plain("a/a"), plain("b/b")], targets(&["a/a, b/b"]));
        assert_eq!(vec![plain("a/a"), plain("b/b")], targets(&["a/a,", "b/b"]));
        assert_eq!(vec![plain("./src"), plain("./tests")], targets(&["./src", "./tests"]));
        assert_eq!(vec![plain("./py/{src,tests}"), named("web", "./{ui,api}")], targets(&["./py/{src,tests}, web=./{ui,api}"]));

        assert_eq!(vec![named("frontend", "./web"), named("backend", "./api")], targets(&["frontend=./web", "backend=./api"]));
        assert_eq!(vec![plain("./project"), named("tests", "./project/tests")], targets(&["./project", "tests=./project/tests"]));
        assert_eq!(vec![named("tests", "./project/tests"), plain("./project")], targets(&["tests=./project/tests", "./project"]));
        assert_eq!(vec![named("f", "./web"), named("f", "./ui"), named("b", "./api")], targets(&["f=./web,./ui", "b=./api"]));
        assert_eq!(vec![named("f", "./web"), named("f", "./ui")], targets(&["f=./web, ./ui"]));
        assert_eq!(vec![named("f", "./web"), plain("./ui")], targets(&["f=./web,", "./ui"]));
        assert_eq!(vec![named("frontend", "C:/my path/web"), named("backend", "C:/my path/api")],
                targets(&["frontend=C:/my path/web", "backend=C:/my path/api"]));
        assert_eq!(vec![plain("C:/my path/web"), plain("D:/other")], targets(&["\"C:/my path/web\"", "D:/other"]));
        assert_eq!(Err("code=".to_owned()), parse_targets(&["code=".to_owned()]));
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
