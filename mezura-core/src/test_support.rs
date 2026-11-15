// Scaffolding shared by the test modules of several files, which is why it cannot live inside any
// one of them. Compiled only under 'cargo test'.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::MountedTree;

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
    pub(crate) const DATA_DIR      : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/");
    pub(crate) const LANGUAGES_DIR : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/data/languages/");
    pub(crate) const FIXTURES_DIR  : &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
}

// Languages that differ only in the extensions they claim, for the tests about who wins one.
pub(crate) fn languages_claiming(claims: &[(&str, &[&str])]) -> std::collections::HashMap<String, crate::Language> {
    crate::languages::keyed_by_name(claims.iter().map(|(name, extensions)|
            crate::Language::new(name, *extensions,
                    crate::StringRules::escaping_with(b'\\').with_symbols(["\""]), ["//"], &[], [])))
}

// A copy of 'folder' read under 'root', a path nothing on disk has, each file read from where it
// lies. It holds what the listing of a revision holds, so '.git' and a folder holding no file stay out.
pub(crate) fn mount_copy_of(folder: &Path, root: &str, repository: Option<&str>) -> Arc<MountedTree> {
    assert!(!Path::new(root).exists(), "{root} is on disk, so a read the tree missed would go unnoticed");
    let mut files = Vec::new();
    collect_files_below(folder, PathBuf::new(), &mut files);
    let originals = files.iter().map(|file| folder.join(file)).collect::<Vec<_>>();
    Arc::new(MountedTree::new(root, repository, files, move |at, buf| {
        std::io::Read::read_to_end(&mut std::fs::File::open(&originals[at])?, buf).map(|_| ())
    }))
}

fn collect_files_below(folder: &Path, below: PathBuf, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(folder.join(&below)).unwrap().flatten() {
        let path = below.join(entry.file_name());
        match entry.file_type().unwrap() {
            _ if entry.file_name() == ".git" => {},
            kind if kind.is_dir() => collect_files_below(folder, path, files),
            kind if kind.is_file() => files.push(path),
            _ => {}
        }
    }
}
