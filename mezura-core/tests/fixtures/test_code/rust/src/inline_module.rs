pub fn a() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        assert!(a() == ());
    }
}

pub fn b() {}
