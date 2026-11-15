use std::cmp::Ordering;
use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::{Component, Components, Path, PathBuf};
use std::sync::Arc;

use crate::engine::NAMES_FOLD_CASE;
use crate::engine::is_the_same_name;
use crate::engine::targets::convert_to_absolute;

const GLOB_WILDCARDS : [char; 3] = ['*', '?', '['];

type ReadFile = dyn Fn(usize, &mut Vec<u8>) -> io::Result<()> + Send + Sync;

/// A set of files that is no folder on disk, counted as though it were one. A revision of a
/// repository is one, and so are an archive and the unsaved files of an editor.
///
/// A run is given its trees through [`crate::EngineConfig::trees`]. A path below the root of a tree
/// is read from that tree, and nothing below the root is read from the disk.
///
/// ```
/// use std::path::PathBuf;
/// use std::sync::Arc;
///
/// use mezura_core::{CountingModel, EngineConfig, Languages, MountedTree, Target, run};
///
/// let files = vec![PathBuf::from("src/main.rs"), PathBuf::from("src/lib.rs")];
/// let contents = vec![b"fn main() {}\n".to_vec(), b"pub fn add() {}\n".to_vec()];
/// let tree = Arc::new(MountedTree::new("D:/repo", None, files, move |at, buf| {
///     buf.extend_from_slice(&contents[at]);
///     Ok(())
/// }));
/// let config = EngineConfig { targets: vec![Target::of("D:/repo/src")], trees: vec![tree], ..Default::default() };
/// let (languages, _) = Languages::shipped(&config);
/// let result = run(&config, languages)?;
/// assert_eq!((2, 2), (result.total.files, result.total.calculate_code_lines(CountingModel::Content)));
/// # Ok::<(), mezura_core::RunError>(())
/// ```
pub struct MountedTree {
    root: PathBuf,
    repository: Option<PathBuf>,
    // The first is the root, and the entries of each are sorted by name as 'compare_names' orders them
    folders: Vec<Folder>,
    read_file: Box<ReadFile>
}

impl MountedTree {
    /// A tree of `files`, the paths of its files relative to the root, each file listed once.
    ///
    /// - `root` is the absolute path the run finds the files under and hands them back with. A tree
    ///   counted by itself takes the path of the folder it copies. A root that names nothing on disk
    ///   is needed only where the same run counts the disk at that place too.
    /// - `repository` is the absolute path of the top folder of the repository on disk, when the
    ///   tree is a copy of the whole of it. The folders above the root are read above that folder,
    ///   so a build file or a test tool's file there applies to the tree as it does to the
    ///   repository. The ignore files above the root stay unread, as git reads none above a
    ///   repository. `None` reads nothing above the root.
    /// - `read_file` is called with the position of a file in `files` and an empty buffer, which it
    ///   fills with the bytes of that file. It is called from several threads at once and gives the
    ///   same bytes every time.
    ///
    /// A name is found the way the file system of the machine finds one, without regard to ASCII
    /// case on Windows and macOS, so `Pom.XML` is found for `pom.xml` there. Two files whose names
    /// differ only in case are both counted.
    pub fn new(root: &str, repository: Option<&str>, files: Vec<PathBuf>,
            read_file: impl Fn(usize, &mut Vec<u8>) -> io::Result<()> + Send + Sync + 'static) -> MountedTree
    {
        let mut folders = vec![Folder::default()];
        let mut folder_named : HashMap<(usize, &OsStr), usize> = HashMap::new();
        for (at, file) in files.iter().enumerate() {
            let mut names = file.components().filter_map(|component| match component {
                Component::Normal(name) => Some(name),
                _ => None
            }).peekable();
            let mut folder = 0;
            while let Some(name) = names.next() {
                if names.peek().is_none() {
                    folders[folder].entries.push(Entry { name: name.to_owned(), node: Node::File(at) });
                    break;
                }
                folder = *folder_named.entry((folder, name)).or_insert_with(|| {
                    let child = folders.len();
                    folders.push(Folder::default());
                    folders[folder].entries.push(Entry { name: name.to_owned(), node: Node::Folder(child) });
                    child
                });
            }
        }
        for folder in &mut folders {
            folder.entries.sort_by(|a, b| compare_names(a.name.as_encoded_bytes(), b.name.as_encoded_bytes()));
        }

        MountedTree {
            root: PathBuf::from(convert_to_absolute(root)),
            repository: repository.map(|repository| PathBuf::from(convert_to_absolute(repository))),
            folders,
            read_file: Box::new(read_file)
        }
    }

    /// Whether `path` names the root, a folder or a file of this tree.
    pub fn holds(&self, path: &Path) -> bool {
        self.find_node(path).is_some()
    }

    pub(crate) fn get_root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn get_repository(&self) -> Option<&Path> {
        self.repository.as_deref()
    }

    pub(crate) fn read_into(&self, path: &Path, buf: &mut Vec<u8>) -> io::Result<()> {
        let Some(Node::File(at)) = self.find_node(path) else {
            return Err(io::ErrorKind::NotFound.into());
        };
        buf.clear();
        (self.read_file)(at, buf)
    }

    fn list_folder_into(&self, directory: &Path, items: &mut Vec<DirItem>) -> io::Result<()> {
        let Some(Node::Folder(folder)) = self.find_node(directory) else {
            return Err(io::ErrorKind::NotFound.into());
        };
        items.extend(self.folders[folder].entries.iter().map(|entry| DirItem {
            name: entry.name.clone(),
            kind: match entry.node {
                Node::File(_) => ItemKind::File,
                Node::Folder(_) => ItemKind::Dir
            },
            size: 0
        }));
        Ok(())
    }

    fn is_root(&self, path: &Path) -> bool {
        self.find_names_below_root(path).is_some_and(|mut below| below.next().is_none())
    }

    fn find_node(&self, path: &Path) -> Option<Node> {
        let mut node = Node::Folder(0);
        for name in self.find_names_below_root(path)? {
            let Node::Folder(folder) = node else { return None };
            node = self.folders[folder].find_entry(name.as_os_str().as_encoded_bytes())?.node;
        }
        Some(node)
    }

    fn find_names_below_root<'p>(&self, path: &'p Path) -> Option<Components<'p>> {
        let mut components = path.components();
        let reaches_the_root = self.root.components().all(|expected| components.next().is_some_and(|found|
                is_the_same_name(found.as_os_str().as_encoded_bytes(), expected.as_os_str().as_encoded_bytes())));
        reaches_the_root.then_some(components)
    }

    // The paths 'glob::glob' gives on disk. A wildcard never matches a separator, and a trailing '**'
    // or a trailing separator takes folders only. The paths come sorted component by component, which
    // is the order of 'glob::glob' except where a '**' is followed by two or more components over
    // nested folders of one name. For '**/src/*.rs' over 'a/src/x.rs' and 'a/src/sub/src/y.rs' glob
    // gives 'x.rs' first, and this gives it second. The case of a name is folded where 'find_node'
    // folds it.
    fn find_glob_matches(&self, fixed_start: &Path, pattern: &str) -> Result<Vec<PathBuf>, glob::PatternError> {
        let (written, only_folders) = match pattern.strip_suffix(['/', '\\']) {
            Some(folders) => (folders, true),
            None => (pattern, pattern.ends_with("**"))
        };
        let compiled = glob::Pattern::new(written)?;
        let options = glob::MatchOptions { case_sensitive: !NAMES_FOLD_CASE, require_literal_separator: true,
                require_literal_leading_dot: false };
        let mut candidates = Vec::new();
        if let Some(node) = self.find_node(fixed_start) {
            // Rebuilt from its components, which drops a trailing separator that '*' would match on its own
            self.collect_paths_into(fixed_start.components().collect(), node, &mut candidates);
        }
        let mut matched = candidates.into_iter()
                .filter(|(path, node)| (!only_folders || matches!(node, Node::Folder(_))) && compiled.matches_path_with(path, options))
                .map(|(path, _)| path).collect::<Vec<_>>();
        matched.sort();
        Ok(matched)
    }

    fn collect_paths_into(&self, path: PathBuf, node: Node, found: &mut Vec<(PathBuf, Node)>) {
        if let Node::Folder(folder) = node {
            for entry in &self.folders[folder].entries {
                self.collect_paths_into(path.join(&entry.name), entry.node, found);
            }
        }
        found.push((path, node));
    }
}

impl std::fmt::Debug for MountedTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MountedTree").field("root", &self.root).field("repository", &self.repository).finish_non_exhaustive()
    }
}

// Two trees are equal when they are the same one. What a tree reads is a function of the caller's,
// and two functions cannot be compared.
impl PartialEq for MountedTree {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

pub(crate) struct TreeView<'a> {
    mounts: &'a [Arc<MountedTree>]
}

impl<'a> TreeView<'a> {
    pub(crate) fn of(mounts: &'a [Arc<MountedTree>]) -> TreeView<'a> {
        TreeView { mounts }
    }

    pub(crate) fn list_dir_into(&self, directory: &Path, items: &mut Vec<DirItem>) -> io::Result<()> {
        if let Some(mount) = self.find_mount_of(directory) {
            return mount.list_folder_into(directory, items);
        }
        for entry in fs::read_dir(directory)?.flatten() {
            let Ok(file_type) = entry.file_type() else { continue };
            // Tested for a link first, since 'is_file' and 'is_dir' are both false for a junction on Windows.
            // Whatever is neither a link nor a file takes the directory arm of the scan, a fifo or a socket included.
            let kind = if file_type.is_symlink() {
                ItemKind::Link
            } else if file_type.is_file() {
                ItemKind::File
            } else {
                ItemKind::Dir
            };
            // Free on Windows, where the directory listing carries it. Elsewhere it would be one call per
            // file, and the consumer learns the size from the read itself.
            let size = match kind {
                ItemKind::File if cfg!(windows) => entry.metadata().map_or(0, |metadata| metadata.len()),
                ItemKind::File | ItemKind::Dir | ItemKind::Link => 0
            };
            items.push(DirItem { name: entry.file_name(), kind, size });
        }
        Ok(())
    }

    pub(crate) fn is_file(&self, path: &Path) -> bool {
        match self.find_mount_of(path) {
            Some(mount) => matches!(mount.find_node(path), Some(Node::File(_))),
            None => path.is_file()
        }
    }

    pub(crate) fn is_dir(&self, path: &Path) -> bool {
        match self.find_mount_of(path) {
            Some(mount) => matches!(mount.find_node(path), Some(Node::Folder(_))),
            None => path.is_dir()
        }
    }

    pub(crate) fn exists(&self, path: &Path) -> bool {
        match self.find_mount_of(path) {
            Some(mount) => mount.holds(path),
            None => path.exists()
        }
    }

    pub(crate) fn is_link(&self, path: &Path) -> bool {
        self.find_mount_of(path).is_none() && path.is_symlink()
    }

    // A tree knows no size before the read, so a file of one is read for it
    pub(crate) fn get_file_size(&self, path: &Path) -> io::Result<u64> {
        match self.find_mount_of(path) {
            Some(_) => self.read_file(path).map(|bytes| bytes.len() as u64),
            None => fs::metadata(path).map(|metadata| metadata.len())
        }
    }

    pub(crate) fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        match self.find_mount_of(path) {
            Some(mount) => {
                let mut bytes = Vec::new();
                mount.read_into(path, &mut bytes).map(|()| bytes)
            },
            None => fs::read(path)
        }
    }

    pub(crate) fn find_mount_of(&self, path: &Path) -> Option<&'a MountedTree> {
        self.mounts.iter().map(Arc::as_ref).find(|mount| mount.find_names_below_root(path).is_some())
    }

    // Above the root of a tree come the folders above the repository it copies
    pub(crate) fn find_parent_of(&self, directory: &'a Path) -> Option<&'a Path> {
        match self.find_mount_of(directory) {
            Some(mount) if mount.is_root(directory) => mount.repository.as_deref().and_then(Path::parent),
            _ => directory.parent()
        }
    }

    pub(crate) fn is_repository_root(&self, directory: &Path) -> bool {
        match self.find_mount_of(directory) {
            Some(mount) => mount.is_root(directory),
            None => directory.join(".git").exists()
        }
    }

    pub(crate) fn expand_glob(&self, pattern: &str) -> Result<Box<dyn Iterator<Item = PathBuf>>, glob::PatternError> {
        let fixed_start = Path::new(find_fixed_start_of(pattern));
        match self.find_mount_of(fixed_start) {
            Some(mount) => Ok(Box::new(mount.find_glob_matches(fixed_start, pattern)?.into_iter())),
            None => Ok(Box::new(glob::glob(pattern)?.flatten()))
        }
    }
}

pub(crate) struct DirItem {
    pub name: OsString,
    pub kind: ItemKind,
    pub size: u64
}

pub(crate) enum ItemKind {
    File,
    Dir,
    Link
}

#[derive(Default)]
struct Folder {
    entries: Vec<Entry>
}

impl Folder {
    // The exact spelling first, then the first of the names the machine takes for the same one
    fn find_entry(&self, name: &[u8]) -> Option<&Entry> {
        let start = self.entries.partition_point(|entry| compare_names(entry.name.as_encoded_bytes(), name).is_lt());
        let end = start + self.entries[start..].partition_point(|entry| compare_names(entry.name.as_encoded_bytes(), name).is_eq());
        let same = &self.entries[start..end];
        same.iter().find(|entry| entry.name.as_encoded_bytes() == name).or(same.first())
    }
}

struct Entry {
    name: OsString,
    node: Node
}

#[derive(Clone, Copy)]
enum Node {
    File(usize),
    Folder(usize)
}

// Equal exactly where 'is_the_same_name' calls two names the same
fn compare_names(a: &[u8], b: &[u8]) -> Ordering {
    match NAMES_FOLD_CASE {
        true => a.iter().map(u8::to_ascii_lowercase).cmp(b.iter().map(u8::to_ascii_lowercase)),
        false => a.cmp(b)
    }
}

// The folders written before the first component that holds a wildcard, which is where 'glob::glob' starts to read
fn find_fixed_start_of(pattern: &str) -> &str {
    match pattern.find(GLOB_WILDCARDS) {
        Some(wildcard) => pattern[..wildcard].rfind(['/', '\\']).map_or("", |separator| &pattern[..=separator]),
        None => pattern
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::targets::spell_out;

    #[test]
    fn a_tree_holds_its_folders_and_files_and_finds_a_name_as_the_machine_does() {
        let root = spell_out(&std::env::temp_dir().join("mezura-tree-with-no-folder"));
        assert!(!Path::new(&root).exists());
        let files = ["a.rs", "A.rs", "src/lib.rs"].map(PathBuf::from).to_vec();
        let trees = [Arc::new(MountedTree::new(&root, None, files, |at, buf| {
            buf.extend_from_slice(format!("file {at}").as_bytes());
            Ok(())
        }))];
        let tree = &trees[0];
        let at = |path: &str| Path::new(&root).join(path);

        assert!(tree.holds(Path::new(&root)) && tree.holds(&at("src")) && tree.holds(&at("src/lib.rs")));
        assert!(!tree.holds(&at("missing")) && !tree.holds(&at("src/lib.rs/below")) && !tree.holds(&at("lib.rs")));
        assert_eq!(NAMES_FOLD_CASE, tree.holds(&at("SRC/LIB.RS")), "a name in another case");

        let view = TreeView::of(&trees);
        let mut listed = Vec::new();
        view.list_dir_into(Path::new(&root), &mut listed).unwrap();
        let mut names = listed.iter().map(|item| item.name.to_string_lossy().into_owned()).collect::<Vec<_>>();
        names.sort();
        assert_eq!(vec!["A.rs", "a.rs", "src"], names, "two names that differ only in case are both listed");
        let read = |path: &str| String::from_utf8(view.read_file(&at(path)).unwrap()).unwrap();
        assert_eq!(("file 0", "file 1"), (read("a.rs").as_str(), read("A.rs").as_str()), "a name read the file of another case");
        if NAMES_FOLD_CASE {
            assert_eq!("file 2", read("Src/Lib.rs"));
        }
    }

    // The order matters as much as the paths, since the matches of a pattern become targets in it
    #[test]
    fn a_pattern_matches_the_same_paths_in_the_same_order_in_a_mounted_copy_as_on_disk() {
        let root = std::env::temp_dir().join("mezura_mounted_glob");
        let _ = fs::remove_dir_all(&root);
        for file in ["a.rs", "b.txt", "src/a.rs", "src/b.rs", "src/core/x.rs", "src/core/deep/y.rs", "src/util/x.rs",
                "src/.hidden/x.rs", "docs/x.rs", "br [v2]/z.rs"] {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "fn a() {}\n").unwrap();
        }
        let on_disk = spell_out(&root);
        let mounted = format!("{on_disk}-mounted");
        let trees = [crate::mount_copy_of(&root, &mounted, None)];
        let expand = |base: &str, trees: &[Arc<MountedTree>], pattern: &str| TreeView::of(trees)
                .expand_glob(&format!("{base}/{pattern}")).unwrap()
                .map(|path| spell_out(&path)[base.len()..].to_owned()).collect::<Vec<_>>();

        for pattern in ["*", "**", "**/*.rs", "src/*/x.rs", "*/", "s?c/[ab].rs", "src/**", "src/**/", "src/**/*.rs",
                "**/.hidden/*", "src/a.rs", "missing/*", "*/core/**/*.rs", "br [[]v2]/*"] {
            assert_eq!(expand(&on_disk, &[], pattern), expand(&mounted, &trees, pattern), "'{pattern}'");
        }
        assert!(!expand(&mounted, &trees, "**/*.rs").is_empty());

        fs::remove_dir_all(&root).unwrap();
    }
}
