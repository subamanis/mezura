pub fn production() {}

mod support;

#[cfg(not(test))]
mod never;

mod nested {
    #[cfg(test)]
    mod deep;
}

#[cfg(test)]
mod tests;
