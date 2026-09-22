pub struct Config {
    name: String,
    #[cfg(test)]
    probe: u8,
    size: usize,
}

fn after() {}
