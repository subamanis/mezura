use std::{fmt::Debug, io::{self, Write}, ops::{Bound, RangeBounds}, path::Path};


trait StringUtils {
    fn substring(&self, start: usize, len: usize) -> &str;
    fn slice(&self, range: impl RangeBounds<usize>) -> &str;
}

impl StringUtils for str {
    fn substring(&self, start: usize, len: usize) -> &str {
        let mut char_pos = 0;
        let mut byte_start = 0;
        let mut it = self.chars();
        loop {
            if char_pos == start { break; }
            if let Some(c) = it.next() {
                char_pos += 1;
                byte_start += c.len_utf8();
            }
            else { break; }
        }
        char_pos = 0;
        let mut byte_end = byte_start;
        loop {
            if char_pos == len { break; }
            if let Some(c) = it.next() {
                char_pos += 1;
                byte_end += c.len_utf8();
            }
            else { break; }
        }
        &self[byte_start..byte_end]
    }

    fn slice(&self, range: impl RangeBounds<usize>) -> &str {
        let start = match range.start_bound() {
            Bound::Included(bound) | Bound::Excluded(bound) => *bound,
            Bound::Unbounded => 0,
        };
        let len = match range.end_bound() {
            Bound::Included(bound) => *bound + 1,
            Bound::Excluded(bound) => *bound,
            Bound::Unbounded => self.len(),
        } - start;
        self.substring(start, len)
    }
}

pub fn print_contents<T>(vec :&Vec<T>) where T : Debug {
    if vec.len() == 0 {
        println!("[]");
        return;
    }

    print!("{:?}",vec[0]);
    for item in vec {
        print!(", \"{:?}\" ", item);
    }
}

pub fn get_contents<T>(vec:& Vec<T>) -> String
  where T : Debug,  {
    if vec.len() == 0 {
        return "[]".to_string();
    }

    let mut s  = format!("{:?}",vec[0]);
    if vec.len() == 1 {return s;}
    for item in vec {
        s.push_str(&format!("{:?}",item));
    }

    s
}

pub fn wait_for_input() {
    print!("\nPress any key to continue...");
    io::stdout().flush();
    let mut s = String::new();
    io::stdin().read_line(&mut s);
}

macro_rules! hashmap {
    ($( $key: expr => $val: expr ),*) => {{
         let mut map = ::std::collections::HashMap::new();
         $( map.insert($key, $val); )*
         map
    }}
}