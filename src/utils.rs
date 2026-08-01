use std::sync::OnceLock;

use crate::*;
use crate::config_manager::{DecimalSeparator, NumberSeparator};


#[macro_export]
macro_rules! hashmap {
    ($( $key: expr => $val: expr ),*) => {{
        #[allow(unused_mut)]
        let mut map = ::std::collections::HashMap::new();
        $( map.insert($key, $val); )*
        map
    }}
}


pub fn round_1(num: f64) -> f64 {
    (num * 10.0).round() / 10.0
}

pub fn round_2(num: f64) -> f64 {
    (num * 100.0).round() / 100.0
}


fn remove_dot_prefix(str: &str) -> &str {
    if let Some(stripped) = str.strip_prefix('.') {
        stripped
    } else {
        str
    }
}

pub fn parse_languages_to_vec(s: &str) -> Vec<String> {
    s.split(',')
    .filter_map(|x| get_trimmed_if_not_empty(&remove_dot_prefix(x.trim()).to_lowercase()))
    .collect::<Vec<_>>()
}

// 'm=matlab,.pl=perl'. The extension is lowercased here, like every other extension the program
// holds, so that the map it ends up in is keyed the same way the lookup asks for it. The language
// name is kept as it was typed and compared without case later, since that is what '--languages'
// already does with the names it is given.
pub fn parse_forced_languages(s: &str) -> Option<HashMap<String,String>> {
    let mut forced = HashMap::new();
    for pair in s.split(',').filter_map(get_trimmed_if_not_empty) {
        let (extension, language) = pair.split_once('=')?;
        let extension = remove_dot_prefix(extension.trim()).to_lowercase();
        let language = language.trim();
        if extension.is_empty() || language.is_empty() {
            return None;
        }
        forced.insert(extension, language.to_owned());
    }

    if forced.is_empty() {None} else {Some(forced)}
}

// Sorted, because it is written into config files and into the log entries that a later run compares
// itself against, and a map's order would make the same setting look like a changed one
pub fn forced_languages_to_string(forced: &HashMap<String,String>) -> String {
    let mut pairs = forced.iter().map(|(extension, language)| format!("{extension}={language}")).collect::<Vec<_>>();
    pairs.sort();
    pairs.join(",")
}

pub fn parse_paths_to_vec(s: &str) -> Vec<String> {
    s.split(',')
    .filter_map(|x| {
        let cleansed = &x.trim().replace("\\", "/");
        get_trimmed_if_not_empty(cleansed.strip_prefix('"').unwrap_or(cleansed).strip_suffix('"').unwrap_or(cleansed))
    })
    .collect::<Vec<_>>()
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

pub fn split_line_on_whitespace(line: &str) -> Vec<String> {
    line.split_whitespace().filter_map(get_trimmed_if_not_empty).collect::<Vec<_>>()
}


pub fn is_valid_path(s: &str) -> bool {
    let p = Path::new(s.trim());
    p.is_dir() || p.is_file()
}

pub fn has_glob_metacharacters(s: &str) -> bool {
    s.contains(['*', '?', '[', '{'])
}

// Paths are compared case-insensitively on Windows, where the file system is
pub fn path_comparison_key(path: &str) -> String {
    if cfg!(windows) {path.to_lowercase()} else {path.to_owned()}
}

fn is_ancestor_of(ancestor: &str, path: &str) -> bool {
    let ancestor = ancestor.trim_end_matches('/');
    path.len() > ancestor.len() + 1 && path.starts_with(ancestor)
            && path.as_bytes()[ancestor.len()] == b'/'
}

// Targets that are contained in other targets would have their files counted twice,
// so only the topmost of every overlapping group is kept.
pub fn remove_overlapping_paths(paths: Vec<String>) -> Vec<String> {
    let mut sorted = paths.into_iter().map(|x| (path_comparison_key(&x), x)).collect::<Vec<_>>();
    sorted.sort();
    sorted.dedup_by(|a, b| a.0 == b.0);

    let mut kept : Vec<(String,String)> = Vec::with_capacity(sorted.len());
    for (key, path) in sorted {
        if !kept.iter().any(|(kept_key,_)| is_ancestor_of(kept_key, &key)) {
            kept.push((key, path));
        }
    }

    kept.into_iter().map(|(_,path)| path).collect()
}

pub fn parse_single_color(token: &str) -> Option<Color> {
    match token.to_lowercase().replace('_', "-").as_str() {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "white" => Some(Color::White),
        "bright-black" => Some(Color::BrightBlack),
        "bright-red" => Some(Color::BrightRed),
        "bright-green" => Some(Color::BrightGreen),
        "bright-yellow" => Some(Color::BrightYellow),
        "bright-blue" => Some(Color::BrightBlue),
        "bright-magenta" => Some(Color::BrightMagenta),
        "bright-cyan" => Some(Color::BrightCyan),
        "bright-white" => Some(Color::BrightWhite),
        other => {
            let hex = other.strip_prefix('#').unwrap_or(other);
            if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            Some(Color::TrueColor {
                r: u8::from_str_radix(&hex[0..2], 16).ok()?,
                g: u8::from_str_radix(&hex[2..4], 16).ok()?,
                b: u8::from_str_radix(&hex[4..6], 16).ok()?
            })
        }
    }
}

pub fn color_to_config_string(color: &Color) -> String {
    match color {
        Color::TrueColor {r, g, b} => format!("{r:02x}{g:02x}{b:02x}"),
        named => format!("{:?}", named).chars().enumerate().flat_map(|(i, c)| {
            if i > 0 && c.is_ascii_uppercase() { vec!['-', c.to_ascii_lowercase()] }
            else { vec![c.to_ascii_lowercase()] }
        }).collect()
    }
}

pub fn build_exclude_matcher(exclude_patterns: &[String]) -> Result<globset::GlobSet, globset::Error> {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in exclude_patterns {
        let normalized = pattern.trim().replace('\\', "/");
        let normalized = normalized.trim_end_matches('/');
        let anchored = if normalized.starts_with("**/") {
            normalized.to_owned()
        } else {
            format!("**/{normalized}")
        };
        builder.add(globset::GlobBuilder::new(&anchored).literal_separator(true).build()?);
    }
    builder.build()
}

pub fn extract_file_contents(file_path: &str) -> Option<String> {
    if Path::new(&file_path).is_file() {
        let mut contents = String::with_capacity(700);
        File::open(file_path).ok()?.read_to_string(&mut contents).ok()?;
        if contents.trim().is_empty() {
            None
        } else {
            Some(contents)
        }
    } else {
        None
    }
}

pub fn get_file_extension(path: &Path) -> Option<&str> {
    match path.extension() {
        Some(x) => x.to_str(),
        None => None
    }
}


// Reached from every printed figure, so it is set once instead of being threaded through the
// printer, the same way the active theme is
static NUMBER_SEPARATOR : OnceLock<NumberSeparator> = OnceLock::new();

static DECIMAL_SEPARATOR : OnceLock<DecimalSeparator> = OnceLock::new();

pub fn set_number_separator(separator: NumberSeparator) {
    let _ = NUMBER_SEPARATOR.set(separator);
}

pub fn set_decimal_separator(separator: DecimalSeparator) {
    let _ = DECIMAL_SEPARATOR.set(separator);
}

// Applied to text that is already rounded, so that every rule about rounding stays written with a
// dot and only the last step decides what the reader sees
pub fn with_decimal_separator(text: String) -> String {
    match DECIMAL_SEPARATOR.get().copied().unwrap_or_default().character() {
        '.' => text,
        separator => text.replace('.', &separator.to_string())
    }
}

pub fn with_seperators(i: usize) -> String {
    with_seperators_str(&i.to_string())
}

pub fn with_seperators_str(i_str: &str) -> String {
    let Some(separator) = NUMBER_SEPARATOR.get().copied().unwrap_or_default().character() else {
        return i_str.to_owned();
    };

    let mut s = String::new();
    let a = i_str.chars().rev().enumerate();
    for (idx, val) in a {
        if idx != 0 && idx % 3 == 0 {
            s.insert(0, separator);
        }
        s.insert(0, val);
    }
    s
}

pub fn num_of_seperators(i: usize) -> usize {
    let mut input = i;
    let mut commas = 0;
    loop {
        input /= 1000;
        if input == 0 {break;}
        commas += 1;
    }

    commas
}



#[cfg(test)]
mod Tests{
    use super::*;

    #[test]
    pub fn test_num_of_seperators() {
        assert_eq!(1, num_of_seperators(1234));
        assert_eq!(0, num_of_seperators(124));
        assert_eq!(0, num_of_seperators(0));
        assert_eq!(1, num_of_seperators(123456));
        assert_eq!(2, num_of_seperators(1234567));
        assert_eq!(3, num_of_seperators(1234567890));
        assert_eq!(3, num_of_seperators(123456789012));
    }

    #[test]
    pub fn test_with_seperators() {
        assert_eq!("123",with_seperators(123));
        assert_eq!("1,234",with_seperators(1234));
        assert_eq!("12,345",with_seperators(12345));
        assert_eq!("1,234,567",with_seperators(1234567));
    }

    #[test]
    pub fn test_parse_languages_to_vec() {
        assert_eq!(Vec::<String>::new(), parse_languages_to_vec(","));
        assert_eq!(Vec::<String>::new(), parse_languages_to_vec(""));
        assert_eq!(Vec::<String>::new(), parse_languages_to_vec("  "));

        assert_eq!(vec!["a".to_owned(),"b".to_owned()], parse_languages_to_vec("a,b"));
        assert_eq!(vec!["a".to_owned(),"b".to_owned()], parse_languages_to_vec("  a ,  b "));
        assert_eq!(vec!["a".to_owned(),"b".to_owned()], parse_languages_to_vec(".A,.b "));
    }

    #[test]
    pub fn test_parse_paths_to_vec() {
        assert_eq!(vec!["a/a".to_owned(),"b/b".to_owned()], parse_paths_to_vec("a\\a,b\\b"));
        assert_eq!(vec!["a".to_owned(),"b/b".to_owned()], parse_paths_to_vec(" a  ,  b\\b "));
    }

    #[test]
    pub fn test_parse_usize_values() {
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
}
#[cfg(test)]
mod target_path_tests {
    use super::*;

    fn dedupe(paths: &[&str]) -> Vec<String> {
        remove_overlapping_paths(paths.iter().map(|x| x.to_string()).collect())
    }

    #[test]
    fn test_has_glob_metacharacters() {
        assert!(has_glob_metacharacters("src/*"));
        assert!(has_glob_metacharacters("a?b"));
        assert!(has_glob_metacharacters("[abc]"));
        assert!(has_glob_metacharacters("{a,b}"));
        assert!(has_glob_metacharacters("D:/dev/**/src"));

        assert!(!has_glob_metacharacters("src"));
        assert!(!has_glob_metacharacters("D:/dev/Rusty/mezura"));
        assert!(!has_glob_metacharacters("../a b/c-d.rs"));
    }

    #[test]
    fn test_remove_overlapping_paths_keeps_unrelated() {
        assert_eq!(Vec::<String>::new(), dedupe(&[]));
        assert_eq!(vec!["D:/a"], dedupe(&["D:/a"]));
        assert_eq!(vec!["D:/a", "D:/b"], dedupe(&["D:/b", "D:/a"]));
        assert_eq!(vec!["D:/a", "E:/a"], dedupe(&["D:/a", "E:/a"]));
    }

    #[test]
    fn test_remove_overlapping_paths_drops_identical() {
        assert_eq!(vec!["D:/a"], dedupe(&["D:/a", "D:/a"]));
        assert_eq!(vec!["D:/a", "D:/b"], dedupe(&["D:/b", "D:/a", "D:/b", "D:/a"]));
    }

    #[test]
    fn test_remove_overlapping_paths_drops_nested() {
        assert_eq!(vec!["D:/a"], dedupe(&["D:/a", "D:/a/b"]));
        assert_eq!(vec!["D:/a"], dedupe(&["D:/a/b", "D:/a"]));
        assert_eq!(vec!["D:/a"], dedupe(&["D:/a", "D:/a/b/c/d", "D:/a/b"]));
        assert_eq!(vec!["D:/a"], dedupe(&["D:/a", "D:/a/file.rs"]));
        assert_eq!(vec!["D:/a", "D:/b"], dedupe(&["D:/a", "D:/a/x", "D:/b", "D:/b/y/z"]));
    }

    #[test]
    fn test_remove_overlapping_paths_respects_component_boundaries() {
        // 'D:/ab' is not inside 'D:/a', despite the string prefix
        assert_eq!(vec!["D:/a", "D:/ab"], dedupe(&["D:/a", "D:/ab"]));
        assert_eq!(vec!["D:/a", "D:/a-b"], dedupe(&["D:/a", "D:/a-b"]));
        // the '-' sorts before the '/', so a naive scan against only the previous kept path
        // would let 'D:/a/b' through, even though it is inside 'D:/a'
        assert_eq!(vec!["D:/a", "D:/a-b"], dedupe(&["D:/a", "D:/a-b", "D:/a/b"]));
        assert_eq!(vec!["D:/a", "D:/a!b", "D:/a-b"], dedupe(&["D:/a/deep/one", "D:/a-b", "D:/a", "D:/a!b"]));
    }

    #[test]
    fn test_remove_overlapping_paths_handles_trailing_slashes_and_case() {
        assert_eq!(vec!["D:/a/"], dedupe(&["D:/a/", "D:/a/b"]));

        let result = dedupe(&["D:/Dev", "D:/dev/sub"]);
        if cfg!(windows) {
            assert_eq!(vec!["D:/Dev"], result);
        } else {
            assert_eq!(vec!["D:/Dev", "D:/dev/sub"], result);
        }
    }
}

#[cfg(test)]
mod exclude_matcher_tests {
    use super::*;

    #[test]
    fn test_name_patterns_match_at_any_depth() {
        let matcher = build_exclude_matcher(&["node_modules".to_owned(), "*.min.js".to_owned()]).unwrap();

        assert!(matcher.is_match("node_modules"));
        assert!(matcher.is_match("D:/proj/node_modules"));
        assert!(!matcher.is_match("D:/proj/node_modules_2"));
        assert!(matcher.is_match("D:/proj/app/bundle.min.js"));
        assert!(!matcher.is_match("D:/proj/app/bundle.js"));
        assert!(!matcher.is_match("D:/proj/appbundle.min.js/other.js"));
    }

    #[test]
    fn test_path_patterns_are_component_anchored() {
        let matcher = build_exclude_matcher(&["Rusty/mezura".to_owned(), "D:/dev/bench".to_owned()]).unwrap();

        assert!(matcher.is_match("D:/dev/Rusty/mezura"));
        assert!(!matcher.is_match("D:/dev/aRusty/mezura"));
        assert!(matcher.is_match("D:/dev/bench"));
        assert!(!matcher.is_match("D:/dev/benchx"));
    }

    #[test]
    fn test_backslashes_and_trailing_slashes_are_normalized() {
        let matcher = build_exclude_matcher(&["Rusty\\mezura\\bench".to_owned(), "target/".to_owned()]).unwrap();

        assert!(matcher.is_match("D:/dev/Rusty/mezura/bench"));
        assert!(matcher.is_match("D:/dev/proj/target"));
    }

        #[test]
    fn test_color_to_config_string() {
        assert_eq!("cyan", color_to_config_string(&Color::Cyan));
        assert_eq!("bright-magenta", color_to_config_string(&Color::BrightMagenta));
        assert_eq!("ff0080", color_to_config_string(&Color::TrueColor{r:255,g:0,b:128}));
    }

    #[test]
    fn test_invalid_glob_is_rejected() {
        assert!(build_exclude_matcher(&["[invalid".to_owned()]).is_err());
        assert!(build_exclude_matcher(&["valid".to_owned(), "[invalid".to_owned()]).is_err());
    }
}
