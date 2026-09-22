#[cfg(
    test
)]
mod tests {
    fn f() {}
}

#[cfg(not(test))]
fn production() {}

#[cfg(all(test, unix))]
fn only_on_unix_tests() {}

#[cfg_attr(test, allow(dead_code))]
fn kept() {}

#[cfg_attr(test, test)]
fn made_a_test() {}

#[cfg_attr(feature = "x", test)]
fn under_a_feature() {}
