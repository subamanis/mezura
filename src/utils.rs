use std::{fmt::Debug, io::{self, Write}, path::Path};

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