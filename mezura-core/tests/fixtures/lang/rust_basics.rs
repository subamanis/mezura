// mezura-expect lines=22 code=10 comments=3 structs=1 enums=1 traits=1
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
    }
}
