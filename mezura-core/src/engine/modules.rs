// The module a file was counted under, and the places where the walk changes which module it is in.
use std::{collections::HashMap, path::Path};

// Carried through the queue as an index and never as a name: a string key would be an allocation on
// every single file.
pub type ModuleId = u16;

// A directory's module is decided once on the way in and its children inherit it, so a run that
// nests nothing looks up nothing at all. Only a target lying inside another target can change the
// answer part way down, and those are the only paths these two tables hold.
#[derive(Debug,Default)]
pub struct Modules {
    // Empty when nothing was named, and then everything belongs to the single bucket 0
    names: Vec<Option<String>>,
    dir_boundaries: HashMap<String, ModuleId>,
    file_boundaries: HashMap<String, ModuleId>
}

impl Modules {
    // Built from the resolved paths and never from what was typed: where the comparison is case
    // sensitive, 'frontend=./Web' over a real './web' matches nothing, that module comes out empty,
    // and every file falls into '(unnamed)' with nothing said about why.
    pub fn of(targets: &[crate::engine::config::Target]) -> Self {
        if targets.iter().all(|x| x.module.is_none()) {
            return Modules::default();
        }

        let mut names : Vec<Option<String>> = Vec::new();
        for target in targets {
            if !names.contains(&target.module) {
                names.push(target.module.clone());
            }
        }
        // The unnamed one is a row like any other, and it is last because it is the leftover
        if let Some(position) = names.iter().position(Option::is_none) {
            let unnamed = names.remove(position);
            names.push(unnamed);
        }

        let roots = crate::engine::targets::topmost_targets(targets);
        let mut modules = Modules { names, ..Default::default() };
        for target in targets {
            if roots.contains(target) {
                continue;
            }
            let id = modules.id_of(&target.module);
            let key = crate::engine::targets::path_comparison_key(target.path.trim_end_matches('/'));
            if Path::new(&target.path).is_dir() {
                modules.dir_boundaries.insert(key, id);
            } else {
                modules.file_boundaries.insert(key, id);
            }
        }

        modules
    }

    fn id_of(&self, module: &Option<String>) -> ModuleId {
        self.names.iter().position(|x| x == module).unwrap_or(0) as ModuleId
    }

    pub fn count(&self) -> usize {
        self.names.len().max(1)
    }

    pub fn is_used(&self) -> bool {
        !self.names.is_empty()
    }

    pub fn name_of(&self, id: ModuleId) -> Option<&str> {
        self.names.get(id as usize).and_then(|x| x.as_deref())
    }

    pub fn of_target(&self, target: &crate::engine::config::Target) -> ModuleId {
        if self.is_used() {self.id_of(&target.module)} else {0}
    }

    pub fn has_dir_boundaries(&self) -> bool {
        !self.dir_boundaries.is_empty()
    }

    pub fn has_file_boundaries(&self) -> bool {
        !self.file_boundaries.is_empty()
    }

    pub fn at_dir(&self, path: &Path, inherited: ModuleId) -> ModuleId {
        self.at(&self.dir_boundaries, path, inherited)
    }

    pub fn at_file(&self, path: &Path, inherited: ModuleId) -> ModuleId {
        self.at(&self.file_boundaries, path, inherited)
    }

    fn at(&self, boundaries: &HashMap<String, ModuleId>, path: &Path, inherited: ModuleId) -> ModuleId {
        let Some(path) = path.to_str() else { return inherited };
        let path = crate::engine::targets::normalise_separators(path);
        boundaries.get(&crate::engine::targets::path_comparison_key(&path)).copied().unwrap_or(inherited)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The paths are the repository's own, because the table has to know whether a nested target is a
    // directory or a file to decide which of the two lookups will find it.
    fn modules_of(entries: &[&str]) -> Modules {
        let targets = entries.iter().map(|entry| match entry.split_once(' ') {
            Some((name, path)) => crate::engine::config::Target::named(name, path),
            None => crate::engine::config::Target::of(*entry)
        }).collect::<Vec<_>>();
    
        Modules::of(&targets)
    }

    #[test]
    fn a_run_that_names_nothing_has_one_bucket_and_no_lookups() {
        let modules = modules_of(&["./src", "./tests"]);
    
        assert!(!modules.is_used());
        assert_eq!(1, modules.count());
        assert_eq!(None, modules.name_of(0));
        assert!(!modules.has_dir_boundaries() && !modules.has_file_boundaries());
    }

    // What the report shows is decided by '--sort' and not by this order.
    #[test]
    fn the_leftovers_are_a_bucket_of_their_own_and_come_last() {
        let modules = modules_of(&["./src", "code ./src/lib.rs", "docs ./data"]);
    
        assert!(modules.is_used());
        assert_eq!(3, modules.count());
        assert_eq!(Some("code"), modules.name_of(0));
        assert_eq!(Some("docs"), modules.name_of(1));
        assert_eq!(None, modules.name_of(2));
    
        // One named target with nothing outside it, so there is no leftover row
        let modules = modules_of(&["code ./src"]);
        assert!(modules.is_used());
        assert_eq!(1, modules.count());
        assert_eq!(Some("code"), modules.name_of(0));
    }

    // A nested file target must not make every directory pay for a lookup, and a nested directory
    // must not make every file pay for one.
    #[test]
    fn only_a_target_inside_another_target_is_a_boundary() {
        let unrelated = modules_of(&["code ./src", "suite ./tests"]);
        assert!(!unrelated.has_dir_boundaries() && !unrelated.has_file_boundaries());
    
        let nested_dir = modules_of(&["./", "fixtures ./tests/fixtures"]);
        assert!(nested_dir.has_dir_boundaries() && !nested_dir.has_file_boundaries());
    
        let nested_file = modules_of(&["./src", "entry ./src/main.rs"]);
        assert!(!nested_file.has_dir_boundaries() && nested_file.has_file_boundaries());
    }

    // Matched with the platform's own idea of case: on Windows a boundary declared with a different
    // capitalisation would otherwise find nothing, and its module would come out empty with nothing
    // printed about why
    #[test]
    fn a_boundary_answers_for_its_own_path_and_leaves_the_rest_inherited() {
        let modules = modules_of(&["./", "fixtures ./tests/fixtures"]);
        let fixtures = modules.id_of(&Some("fixtures".to_owned()));
    
        assert_eq!(fixtures, modules.at_dir(Path::new("./tests/fixtures"), 7));
        assert_eq!(7, modules.at_dir(Path::new("./tests"), 7));
        assert_eq!(7, modules.at_dir(Path::new("./tests/fixtures/lang"), 7));
        // the same path as the platform hands it over during a walk
        assert_eq!(fixtures, modules.at_dir(Path::new(".\\tests\\fixtures"), 7));
        if cfg!(windows) {
            assert_eq!(fixtures, modules.at_dir(Path::new("./TESTS/Fixtures"), 7));
        }
    }
}
