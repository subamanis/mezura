fn run(flag: bool) -> u8 {
    #[cfg(test)]
    if flag {
        1
    } else if !flag {
        2
    }
    // a comment between the arms

    else {
        3
    }
}

fn after() {}
