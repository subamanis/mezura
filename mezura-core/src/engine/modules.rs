// The module a file was counted under, and the places where the walk changes its mind about which
// one it is in.
use std::{collections::HashMap, path::Path};


// The module a file was counted under, carried through the queue as an index and never as a name.
// A composite string key would be an allocation on every single file, which is what the whole
// performance work of v3 was spent removing.
pub type ModuleId = u16;

// The names, in the order they were declared, and the places where the walk changes its mind about
// which module it is in.
//
// The module of a directory is decided once, on the way in, and its children inherit it, so a run
// that nests nothing looks up nothing at all: every root carries its own module and the walk never
// asks again. Only a target that lies inside another target can change the answer part way down,
// and those are the only paths this table holds.
#[derive(Debug,Default)]
pub struct Modules {
    // Empty when nothing was named, and then everything belongs to the single bucket 0
    names: Vec<Option<String>>,
    dir_boundaries: HashMap<String, ModuleId>,
    file_boundaries: HashMap<String, ModuleId>
}

impl Modules {
    // The boundaries are built from the resolved paths and never from what was typed, because
    // 'starts_with' and an equality on a path are case sensitive on every platform: on Windows a
    // 'frontend=./Web' over a real './web' would match nothing, the module would come out empty and
    // every file would fall into '(unnamed)' with nothing printed to say why.
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

    // The module of a target, for the roots the traversal is handed
    pub fn of_target(&self, target: &crate::engine::config::Target) -> ModuleId {
        if self.is_used() {self.id_of(&target.module)} else {0}
    }

    pub fn has_dir_boundaries(&self) -> bool {
        !self.dir_boundaries.is_empty()
    }

    pub fn has_file_boundaries(&self) -> bool {
        !self.file_boundaries.is_empty()
    }

    // Called only when the run declared a target inside another one, which is the only way a child
    // can belong somewhere other than where its parent does
    pub fn at_dir(&self, path: &Path, inherited: ModuleId) -> ModuleId {
        self.at(&self.dir_boundaries, path, inherited)
    }

    pub fn at_file(&self, path: &Path, inherited: ModuleId) -> ModuleId {
        self.at(&self.file_boundaries, path, inherited)
    }

    fn at(&self, boundaries: &HashMap<String, ModuleId>, path: &Path, inherited: ModuleId) -> ModuleId {
        let Some(path) = path.to_str() else { return inherited };
        boundaries.get(&crate::engine::targets::path_comparison_key(&path.replace('\\', "/"))).copied().unwrap_or(inherited)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    // 'name path' declares the module, a bare path declares none. The paths are the repository's
    // own, because a boundary is only a boundary if it is on disk: the table has to know whether a
    // nested target is a directory or a file to decide which of the two lookups will find it.
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

    // The order is the order they were declared in, except that the leftovers are last because they
    // are the leftovers. What the report shows is decided by '--sort' and not by this.
    #[test]
    fn the_leftovers_are_a_bucket_of_their_own_and_come_last() {
        let modules = modules_of(&["./src", "code ./src/lib.rs", "docs ./data"]);
    
        assert!(modules.is_used());
        assert_eq!(3, modules.count());
        assert_eq!(Some("code"), modules.name_of(0));
        assert_eq!(Some("docs"), modules.name_of(1));
        assert_eq!(None, modules.name_of(2));
    
        // One name and there is a second axis, with nothing left over to put in an unnamed row
        let modules = modules_of(&["code ./src"]);
        assert!(modules.is_used());
        assert_eq!(1, modules.count());
        assert_eq!(Some("code"), modules.name_of(0));
    }

    // The lookup that a walk pays for is the one that can find something. A nested file target must
    // not make every directory pay, and a nested directory must not make every file pay.
    #[test]
    fn only_a_target_inside_another_target_is_a_boundary() {
        let unrelated = modules_of(&["code ./src", "suite ./tests"]);
        assert!(!unrelated.has_dir_boundaries() && !unrelated.has_file_boundaries());
    
        let nested_dir = modules_of(&["./", "fixtures ./tests/fixtures"]);
        assert!(nested_dir.has_dir_boundaries() && !nested_dir.has_file_boundaries());
    
        let nested_file = modules_of(&["./src", "entry ./src/main.rs"]);
        assert!(!nested_file.has_dir_boundaries() && nested_file.has_file_boundaries());
    }

    // A path that does not match falls through to what the parent was, and the match is made on the
    // resolved path with the platform's own idea of case, or a boundary declared with a different
    // capitalisation would find nothing and its module would come out empty with nothing printed
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
