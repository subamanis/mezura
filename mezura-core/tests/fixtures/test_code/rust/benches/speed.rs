use test::Bencher;

#[bench]
fn speed(b: &mut Bencher) {
    b.iter(|| 1);
}
