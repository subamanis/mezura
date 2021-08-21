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

#[inline]
pub fn num_of_seperators(i: usize) -> usize {
    let mut input = i;
    let mut commas = 0;
    loop {
        input = input / 1000;
        if input == 0 {break;}
        commas += 1;
    }

    commas
}

pub fn wait_for_input() {
    print!("\nPress any key to exit...");
    let _ = io::stdout().flush();
    let mut s = String::new();
    let _ = io::stdin().read_line(&mut s);
    println!();
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
}