use crate::*;
use std::{fmt::Debug};


pub fn round_1(num: f64) -> f64 {
    (num * 10.0).round() / 10.0
}

pub fn round_2(num: f64) -> f64 {
    (num * 100.0).round() / 100.0
}

pub fn parse_languages_to_vec(s: &str) -> Vec<String> {
    s.split(',').filter_map(|x| get_trimmed_if_not_empty(&remove_dot_prefix(x.trim()).to_lowercase())).collect::<Vec<_>>()
}

pub fn parse_paths_to_vec(s: &str) -> Vec<String> {
    s
    .split(',')
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
    let elements = s.split_whitespace().filter_map(|x| get_trimmed_if_not_empty(x)).collect::<Vec<_>>();
    if elements.len() != 2 {
        return None
    }

    if let Ok(val1) = elements[0].parse::<usize>() {
        if let Ok(val2) = elements[1].parse::<usize>() {
            if val1 >= min1 && val1 <= max1 && val2 >= min2 && val2 <= max2 {
                return Some((val1,val2));
            }
        }
    }
    
    None
}

pub fn is_valid_path(s: &str) -> bool {
    let p = Path::new(s.trim());
    p.is_dir() || p.is_file()
}

pub fn get_trimmed_if_not_empty(str: &str) -> Option<String> {
    let str = str.trim();
    if str.is_empty() {None}
    else {Some(str.to_owned())}
}

pub fn print_contents<T>(vec :&[T]) where T : Debug {
    if vec.is_empty() {
        println!("[]");
        return;
    }

    print!("{:?}",vec[0]);
    for item in vec.iter().skip(1) {
        print!(", \"{:?}\" ", item);
    }
}

pub fn extract_file_contents(file_path: &str) -> Option<String> {
    if Path::new(&file_path).is_file() {
        let mut contents = String::with_capacity(700);
        File::open(&file_path).unwrap().read_to_string(&mut contents);
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

#[macro_export]
macro_rules! hashmap {
    ($( $key: expr => $val: expr ),*) => {{
        #[allow(unused_mut)]
        let mut map = ::std::collections::HashMap::new();
        $( map.insert($key, $val); )*
        map
    }}
}


fn remove_dot_prefix(str: &str) -> &str {
    if let Some(stripped) = str.strip_prefix('.') {
        stripped
    } else {
        str
    }
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