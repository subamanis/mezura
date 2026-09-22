fn pick(n: u8) -> u8 {
    match n {
        #[cfg(test)]
        0 => 9,
        1 => 1,
        _ => 2,
    }
}

fn after() {}
