use crate::*;


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


pub fn parse_languages_to_vec(s: &str) -> Vec<String> {
    fn remove_dot_prefix(str: &str) -> &str {
        if let Some(stripped) = str.strip_prefix('.') {
            stripped
        } else {
            str
        }
    }

    s.split(',')
    .filter_map(|x| get_trimmed_if_not_empty(&remove_dot_prefix(x.trim()).to_lowercase()))
    .collect::<Vec<_>>()
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

pub fn parse_colors_to_vec(s: &str) -> Option<Vec<Color>> {
    let entries = s.split_whitespace().collect::<Vec<_>>();
    if entries.is_empty() || entries.len() > 5 {
        return None;
    }

    let mut colors = Vec::with_capacity(5);
    for entry in entries {
        colors.push(parse_single_color(entry)?);
    }

    Some(colors)
}

fn parse_single_color(token: &str) -> Option<Color> {
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


pub fn with_seperators(i: usize) -> String {
    let mut s = String::new();
    let i_str = i.to_string();
    let a = i_str.chars().rev().enumerate();
    for (idx, val) in a {
        if idx != 0 && idx % 3 == 0 {
            s.insert(0, ',');
        }
        s.insert(0, val);
    }
    s
}

pub fn with_seperators_str(i_str: &str) -> String {
    let mut s = String::new();
    let a = i_str.chars().rev().enumerate();
    for (idx, val) in a {
        if idx != 0 && idx % 3 == 0 {
            s.insert(0, ',');
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
    fn test_parse_colors_to_vec() {
        assert_eq!(Some(vec![Color::TrueColor{r:255,g:0,b:0}]), parse_colors_to_vec("ff0000"));
        assert_eq!(Some(vec![Color::TrueColor{r:255,g:0,b:0}]), parse_colors_to_vec("#FF0000"));
        assert_eq!(Some(vec![Color::Cyan, Color::BrightMagenta, Color::TrueColor{r:1,g:2,b:3}]),
                parse_colors_to_vec("cyan BRIGHT-MAGENTA #010203"));
        assert_eq!(Some(vec![Color::BrightYellow]), parse_colors_to_vec("bright_yellow"));
        assert_eq!(5, parse_colors_to_vec("cyan magenta yellow 6ad9bd d7c9f0").unwrap().len());

        assert_eq!(None, parse_colors_to_vec(""));
        assert_eq!(None, parse_colors_to_vec("   "));
        assert_eq!(None, parse_colors_to_vec("a b c d e f"));
        assert_eq!(None, parse_colors_to_vec("ff000"));
        assert_eq!(None, parse_colors_to_vec("ff00000"));
        assert_eq!(None, parse_colors_to_vec("ff00zz"));
        assert_eq!(None, parse_colors_to_vec("ff0000 kaka"));
        assert_eq!(None, parse_colors_to_vec("brightest-yellow"));
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
