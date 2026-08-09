// mezura-expect lines=24 code=12 comments=3 structs=1 enums=1 traits=1
use std::fmt;

/* a block comment
   that spans two lines */
pub struct Point {
    x: i32,
}

pub enum Shape {
    Circle,
}

pub trait Draw {
    fn draw(&self);
}

impl Draw for Point {
    fn draw(&self) {
        println!("// still code");
        let raw = r#"say "hi" and stay code"#;
        let path = r#"C:\ends\in\backslash\"#;
    }
}
