// Scaffolding shared by the test modules of several files, which is why it cannot live inside any
// one of them. Compiled only under 'cargo test'.

// Not '#[macro_export]', which is unconditional and would put this at the root of every crate that
// depends on us.
macro_rules! hashmap {
    ($( $key: expr => $val: expr ),*) => {{
        #[allow(unused_mut)]
        let mut map = ::std::collections::HashMap::new();
        $( map.insert($key, $val); )*
        map
    }}
}

// Anchored on the manifest and not on the working directory, which only happens to be the package
// root when cargo is the one running the test.
pub(crate) mod test_paths {
    pub const DATA_DIR      : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/");
    pub const LANGUAGES_DIR : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/languages/");
    pub const FIXTURES_DIR  : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
}

// Languages that differ only in the extensions they claim, for the tests about who wins one.
pub(crate) fn languages_claiming(claims: &[(&str, &[&str])]) -> std::collections::HashMap<String, crate::Language> {
    crate::languages::keyed_by_name(claims.iter().map(|(name, extensions)|
            crate::Language::new(name, *extensions,
                    crate::StringRules::escaping_with(b'\\').with_symbols(["\""]), ["//"], &[], [])))
}
