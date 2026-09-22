// #[test] in a comment
/* #[cfg(test)] in a block comment */
const A: &str = "#[test]";
const B: &str = "#![cfg(test)]";
fn f() {}
