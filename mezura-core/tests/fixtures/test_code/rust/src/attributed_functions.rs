fn helper() -> u8 { 1 }

#[test]
fn first() {
    assert_eq!(helper(), 1);
}

fn between() {}

#[test]
#[should_panic]
fn second() {
    panic!("{}", helper());
}

#[bench]
fn third(b: &mut Bencher) {
    b.iter(|| helper());
}
