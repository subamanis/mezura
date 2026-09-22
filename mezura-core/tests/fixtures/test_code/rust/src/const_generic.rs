#[test]
fn sized() -> Wrapper<{ SIZE }> {
    Wrapper::new()
}

fn after() {}
