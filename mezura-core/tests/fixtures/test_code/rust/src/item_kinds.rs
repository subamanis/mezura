#[cfg(test)]
const fn zero() -> u8 {
    0
}

#[cfg(test)]
extern "C" {
    fn c_call();
}

#[cfg(test)]
pub(crate) unsafe fn dangerous() {}

#[cfg(test)]
pub async fn later() {}

#[cfg(test)]
impl Zero {
    fn get(&self) -> u8 { 0 }
}

#[cfg(test)]
pub struct Holder {
    value: u8,
}

#[cfg(test)]
enum Either {
    Left,
    Right,
}

#[cfg(test)]
trait Speak {
    fn speak(&self);
}

#[cfg(test)]
macro_rules! shout {
    () => {};
}

fn after() {}
