/// ```
/// assert!(true)
/// ```
pub fn documented() {}

macro_rules! make_test {
    ($name:ident) => {
        #[test]
        fn $name() {}
    };
}

make_test!(generated);

#[cfg(test)]
#[path = "elsewhere.rs"]
mod tests;
