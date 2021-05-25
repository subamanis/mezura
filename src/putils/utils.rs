use super::*;

pub fn print_contents<T>(vec :&[T]) where T : Debug {
    if vec.is_empty() {
        println!("[]");
        return;
    }

    print!("{:?}",vec[0]);
    for item in vec {
        print!(", \"{:?}\" ", item);
    }
}

pub fn get_contents<T>(vec: &[T]) -> String
  where T : Debug,  {
    if vec.is_empty() {
        return "[]".to_string();
    }

    let mut s  = format!("{:?}",vec[0]);
    if vec.len() == 1 {return s;}
    for item in vec {
        s.push_str(&format!("{:?}",item));
    }

    s
}

#[inline]
pub fn get_file_name(path: &Path) -> Option<&str> {
    match path.file_name() {
        Some(x) => match x.to_str() {
            Some(y) => Some(y),
            None => None
        },
        None => None
    }
}

#[inline]
pub fn get_file_extension(path: &Path) -> Option<&str> {
    match path.extension() {
        Some(x) => match x.to_str() {
            Some(y) => Some(y),
            None => None
        },
        None => None
    }
}

pub fn wait_for_input() {
    print!("\nPress any key to continue...");
    io::stdout().flush();
    let mut s = String::new();
    io::stdin().read_line(&mut s);
}