cfg_if::cfg_if! {
    if #[cfg(test)] {
        fn a() {}
    } else {
        fn b() {}
    }
}
