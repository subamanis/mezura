use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub(crate) struct TreeView;

impl TreeView {
    pub(crate) fn list_dir_into(&self, directory: &Path, items: &mut Vec<DirItem>) -> io::Result<()> {
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
        path.is_file()
    }

    pub(crate) fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    pub(crate) fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    pub(crate) fn is_link(&self, path: &Path) -> bool {
        path.is_symlink()
    }

    pub(crate) fn get_file_size(&self, path: &Path) -> io::Result<u64> {
        fs::metadata(path).map(|metadata| metadata.len())
    }

    pub(crate) fn read_file(&self, path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }

    pub(crate) fn is_repository_root(&self, directory: &Path) -> bool {
        directory.join(".git").exists()
    }

    pub(crate) fn expand_glob(&self, pattern: &str) -> Result<impl Iterator<Item = PathBuf>, glob::PatternError> {
        glob::glob(pattern).map(Iterator::flatten)
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
