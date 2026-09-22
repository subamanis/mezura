#[cfg(test)]
use std::collections::HashMap;

#[cfg(test)]
const OPEN: [u8; 3] = [b'{', b'{', b'{'];

#[cfg(test)]
static BRACE: &str = "{";

#[cfg(test)]
type Map = HashMap<u8, u8>;

#[cfg(test)]
extern crate alloc;

#[cfg(test)]
mod helpers;

#[cfg(test)]
pub use helpers::*;

fn after() {
    #[cfg(test)]
    let x = if true { 1 } else { 2 };
    let _ = x;
}
