use super::*;

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

#[inline]
pub fn get_file_extension(path: &Path) -> Option<&str> {
    match path.extension() {
        Some(x) => x.to_str(),
        None => None
    }
}

#[inline]
pub fn with_seperators(i: usize) -> String{
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

pub fn wait_for_input() {
    print!("\nPress any key to exit...");
    let _ = io::stdout().flush();
    let mut s = String::new();
    let _ = io::stdin().read_line(&mut s);
    println!();
}