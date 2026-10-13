use std::path::{Path, PathBuf};

use crate::TestFileName;
use crate::engine::is_the_same_name;
use crate::engine::path_patterns::{PathPatternMatcher, PatternMatch};

// Every file of a row has to be present, since an Octave package carries a DESCRIPTION too.
// 'src/test' is the one directory two levels down, and the walk holds the 'src' as 'JvmSources'.
const BUILD_TOOLS : [BuildTool; 19] = [
    BuildTool { files: &["Cargo.toml"], test_directories: &["tests"] },
    BuildTool { files: &["pom.xml"], test_directories: &["src/test"] },
    BuildTool { files: &["build.gradle"], test_directories: &["src/test"] },
    BuildTool { files: &["build.gradle.kts"], test_directories: &["src/test"] },
    BuildTool { files: &["settings.gradle"], test_directories: &["src/test"] },
    BuildTool { files: &["settings.gradle.kts"], test_directories: &["src/test"] },
    BuildTool { files: &["build.sbt"], test_directories: &["src/test"] },
    BuildTool { files: &["Package.swift"], test_directories: &["Tests"] },
    BuildTool { files: &["mix.exs"], test_directories: &["test"] },
    BuildTool { files: &["pubspec.yaml"], test_directories: &["test", "integration_test"] },
    BuildTool { files: &["Project.toml"], test_directories: &["test"] },
    BuildTool { files: &["JuliaProject.toml"], test_directories: &["test"] },
    BuildTool { files: &["Makefile.PL"], test_directories: &["t"] },
    BuildTool { files: &["Build.PL"], test_directories: &["t"] },
    BuildTool { files: &["dist.ini"], test_directories: &["t"] },
    BuildTool { files: &["project.clj"], test_directories: &["test"] },
    BuildTool { files: &["rebar.config"], test_directories: &["test"] },
    BuildTool { files: &["elm.json"], test_directories: &["tests"] },
    BuildTool { files: &["DESCRIPTION", "NAMESPACE"], test_directories: &["tests"] },
];
// Every 'src/test' below one of these is a subproject's test sources or is never compiled
const JVM_BUILD_ROOTS : [&str; 3] = ["build.sbt", "settings.gradle", "settings.gradle.kts"];

// What the path rules said of a file before it is read. Under 'TakenBack' the markers that open
// lines still count and the markers that make a whole file test code do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TestsByPath {
    Nothing,
    WholeFile,
    TakenBack,
}

// A directory a build tool compiles for tests alone makes every file under it a test file. What a
// '!' pattern leaves is 'DeclaredNotTests', which also switches the toolchain's file names off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum TestScope {
    #[default]
    Ordinary,
    Tests,
    JvmSources,
    DeclaredNotTests,
}

impl TestScope {
    pub(crate) fn of_child(self, dir_name: &str, build_files: BuildFilesSeen, inside_jvm_build: bool) -> TestScope {
        if matches!(self, TestScope::Tests | TestScope::DeclaredNotTests) {
            return self;
        }
        let name = dir_name.as_bytes();
        if self == TestScope::JvmSources && is_the_jvm_test_directory(name) {
            return TestScope::Tests;
        }
        let mut scope = TestScope::Ordinary;
        for tool in BUILD_TOOLS.iter().filter(|tool| !build_files.is_empty() && build_files.holds_every_file_of(tool)) {
            for directory in tool.test_directories {
                match directory.split_once('/') {
                    None if is_the_same_name(name, directory.as_bytes()) => return TestScope::Tests,
                    Some((sources, _)) if is_the_same_name(name, sources.as_bytes()) => scope = TestScope::JvmSources,
                    _ => ()
                }
            }
        }
        if inside_jvm_build && is_the_jvm_sources_directory(name) {
            return TestScope::JvmSources;
        }
        scope
    }

    pub(crate) fn of_target(path: &Path) -> DirectoryScope {
        let (test_directory, inside_jvm_build) = find_test_directory_along(path);
        if test_directory.is_some() {
            return DirectoryScope { scope: TestScope::Tests, inside_jvm_build, ..DirectoryScope::default() };
        }
        let is_jvm_sources = path.file_name().zip(path.parent()).is_some_and(|(name, parent)| {
            let name = name.as_encoded_bytes();
            (inside_jvm_build && is_the_jvm_sources_directory(name)) || is_jvm_sources(name, parent)
        });
        let scope = if is_jvm_sources { TestScope::JvmSources } else { TestScope::Ordinary };
        DirectoryScope { scope, inside_jvm_build, ..DirectoryScope::default() }
    }

    // A pattern overrules a build tool, and among patterns the one written last does
    pub(crate) fn overruled_by(self, decided_by: Option<usize>, found: Option<PatternMatch>) -> (TestScope, Option<usize>) {
        match found {
            Some(found) if decided_by.is_none_or(|earlier| found.written_at > earlier) => {
                let scope = if found.negated { TestScope::DeclaredNotTests } else { TestScope::Tests };
                (scope, Some(found.written_at))
            },
            _ => (self, decided_by)
        }
    }

    pub(crate) fn judge_file(self, file_name: &str, names: &[TestFileName]) -> TestsByPath {
        match self {
            TestScope::Tests => TestsByPath::WholeFile,
            TestScope::DeclaredNotTests => TestsByPath::TakenBack,
            TestScope::Ordinary | TestScope::JvmSources => match names.iter().any(|name| name.matches(file_name)) {
                true => TestsByPath::WholeFile,
                false => TestsByPath::Nothing
            }
        }
    }
}

// The target's own listing is read by the walk, so a build file in it is not asked for here
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct DirectoryScope {
    pub scope: TestScope,
    pub inside_jvm_build: bool,
    pub decided_by: Option<usize>,
    // The offset into the absolute path that a name pattern reads from
    pub names_root: usize,
}

impl DirectoryScope {
    pub(crate) fn of_target(path: &Path, names_root: usize, patterns: &PathPatternMatcher) -> DirectoryScope {
        let mut found = TestScope::of_target(path);
        found.names_root = names_root;
        if !patterns.is_empty() && let Some(absolute) = path.to_str() {
            let declared = patterns.find_last_match_along(absolute, names_root, false, &mut Vec::new());
            (found.scope, found.decided_by) = found.scope.overruled_by(None, declared);
        }
        found
    }

    pub(crate) fn of_child(self, dir_name: &str, build_files: BuildFilesSeen, child_path: &Path,
            patterns: &PathPatternMatcher, found: &mut Vec<usize>) -> DirectoryScope
    {
        let inside_jvm_build = self.inside_jvm_build || build_files.opens_a_jvm_build();
        let scope = self.scope.of_child(dir_name, build_files, inside_jvm_build);
        let declared = patterns.find_last_match_of(child_path, self.names_root, true, found);
        let (scope, decided_by) = scope.overruled_by(self.decided_by, declared);
        DirectoryScope { scope, inside_jvm_build, decided_by, names_root: self.names_root }
    }

    pub(crate) fn of_file(self, file_path: &Path, patterns: &PathPatternMatcher, found: &mut Vec<usize>) -> (TestScope, Option<usize>) {
        self.scope.overruled_by(self.decided_by, patterns.find_last_match_of(file_path, self.names_root, false, found))
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct BuildFilesSeen(u32);

impl BuildFilesSeen {
    pub(crate) fn is_a_build_file(name: &[u8]) -> bool {
        Self::find_index_of(name).is_some()
    }

    pub(crate) fn note(&mut self, name: &[u8]) {
        if let Some(index) = Self::find_index_of(name) {
            self.0 |= 1 << index;
        }
    }

    pub(crate) fn opens_a_jvm_build(self) -> bool {
        !self.is_empty() && JVM_BUILD_ROOTS.iter().any(|name| self.holds(name))
    }

    fn is_empty(self) -> bool {
        self.0 == 0
    }

    fn holds_every_file_of(self, tool: &BuildTool) -> bool {
        tool.files.iter().all(|file| self.holds(file))
    }

    fn holds(self, file: &str) -> bool {
        Self::find_index_of(file.as_bytes()).is_some_and(|index| self.0 & (1 << index) != 0)
    }

    fn find_index_of(name: &[u8]) -> Option<usize> {
        BUILD_TOOLS.iter().flat_map(|tool| tool.files).position(|file| is_the_same_name(name, file.as_bytes()))
    }
}

pub(crate) struct TestDirectory<'a> {
    pub directory: &'a Path,
    pub build_files: Vec<PathBuf>,
}

// The components are read by name, and the disk is asked only at one named like a test
// directory, plus the three root files at every directory above the path.
pub(crate) fn find_test_directory_along(path: &Path) -> (Option<TestDirectory<'_>>, bool) {
    let mut directories = path.ancestors().collect::<Vec<_>>();
    directories.reverse();
    let mut jvm_build_root = None;
    for directory in directories {
        if let (Some(name), Some(parent)) = (directory.file_name(), directory.parent()) {
            let name = name.as_encoded_bytes();
            if let Some(root) = &jvm_build_root && is_the_jvm_test_directory(name)
                    && parent.file_name().is_some_and(|above| is_the_jvm_sources_directory(above.as_encoded_bytes())) {
                return (Some(TestDirectory { directory, build_files: vec![PathBuf::clone(root)] }), true);
            }
            if let Some((holder, files)) = find_build_of_test_directory(name, parent) {
                let build_files = files.iter().map(|file| holder.join(file)).collect();
                return (Some(TestDirectory { directory, build_files }), jvm_build_root.is_some());
            }
        }
        if directory != path && let Some(file) = find_jvm_build_root(directory) {
            jvm_build_root = Some(directory.join(file));
        }
    }
    (None, jvm_build_root.is_some())
}

struct BuildTool {
    files: &'static [&'static str],
    test_directories: &'static [&'static str],
}

fn find_build_of_test_directory<'a>(name: &[u8], parent: &'a Path) -> Option<(&'a Path, &'static [&'static str])> {
    BUILD_TOOLS.iter().find_map(|tool| tool.test_directories.iter().find_map(|directory| {
        let (above, last) = match directory.rsplit_once('/') {
            Some((above, last)) => (Some(above), last),
            None => (None, *directory)
        };
        if !is_the_same_name(name, last.as_bytes()) {
            return None;
        }
        let holder = match above {
            None => Some(parent),
            Some(above) => parent.file_name()
                    .filter(|parent_name| is_the_same_name(parent_name.as_encoded_bytes(), above.as_bytes()))
                    .and_then(|_| parent.parent())
        };
        holder.filter(|holder| tool.files.iter().all(|file| holder.join(file).is_file())).map(|holder| (holder, tool.files))
    }))
}

fn is_jvm_sources(name: &[u8], parent: &Path) -> bool {
    BUILD_TOOLS.iter().any(|tool| tool.test_directories.iter()
            .filter_map(|directory| directory.split_once('/'))
            .any(|(sources, _)| is_the_same_name(name, sources.as_bytes())
                    && tool.files.iter().all(|file| parent.join(file).is_file())))
}

fn find_jvm_build_root(directory: &Path) -> Option<&'static str> {
    JVM_BUILD_ROOTS.into_iter().find(|file| directory.join(file).is_file())
}

fn is_the_jvm_sources_directory(name: &[u8]) -> bool {
    find_two_level_directories().any(|(sources, _)| is_the_same_name(name, sources.as_bytes()))
}

fn is_the_jvm_test_directory(name: &[u8]) -> bool {
    find_two_level_directories().any(|(_, test)| is_the_same_name(name, test.as_bytes()))
}

fn find_two_level_directories() -> impl Iterator<Item = (&'static str, &'static str)> {
    BUILD_TOOLS.iter().flat_map(|tool| tool.test_directories).filter_map(|directory| directory.split_once('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_file_makes_the_test_directory_of_its_tool_and_no_directory_counts_by_its_name_alone() {
        use TestScope::{JvmSources, Ordinary, Tests};
        let seen = |files: &[&str]| files.iter().fold(BuildFilesSeen::default(), |mut seen, file| {
            seen.note(file.as_bytes());
            seen
        });
        let nothing = BuildFilesSeen::default();
        for tool in &BUILD_TOOLS {
            let beside = seen(tool.files);
            for directory in tool.test_directories {
                match directory.split_once('/') {
                    None => {
                        assert_eq!(Ordinary.of_child(directory, beside, false), Tests, "{directory} beside {:?}", tool.files);
                        assert_eq!(Ordinary.of_child(directory, nothing, false), Ordinary, "{directory} beside nothing");
                    },
                    Some((sources, test)) => {
                        assert_eq!(Ordinary.of_child(sources, beside, false), JvmSources, "{sources} beside {:?}", tool.files);
                        assert_eq!(Ordinary.of_child(sources, nothing, false), Ordinary);
                        assert_eq!(Ordinary.of_child(test, beside, false), Ordinary, "{test} beside {:?} with no {sources} above", tool.files);
                        assert_eq!(JvmSources.of_child(test, nothing, false), Tests);
                        assert_eq!(JvmSources.of_child("main", nothing, false), Ordinary);
                    }
                }
            }
        }
        assert_eq!(Ordinary.of_child("src", seen(&["Cargo.toml"]), false), Ordinary);
        assert_eq!(Ordinary.of_child("__tests__", seen(&["Cargo.toml"]), false), Ordinary);
        assert_eq!(Ordinary.of_child("tests", seen(&["DESCRIPTION"]), false), Ordinary, "an Octave package is not an R one");
        assert_eq!(Ordinary.of_child("tests", seen(&["package.json"]), false), Ordinary);
        assert_eq!(Tests.of_child("examples", nothing, false), Tests);
        assert_eq!(Tests.of_child("src", seen(&["pom.xml"]), false), Tests);
        assert_eq!(JvmSources.of_child("tests", seen(&["Cargo.toml"]), false), Tests);

        assert_eq!(Ordinary.of_child("src", nothing, true), JvmSources, "a 'src' anywhere under a declared build");
        assert_eq!(Ordinary.of_child("core", nothing, true), Ordinary);
        assert_eq!(Ordinary.of_child("test", nothing, true), Ordinary, "a 'test' with no 'src' above it");
        assert_eq!(Ordinary.of_child("tests", seen(&["Cargo.toml"]), true), Tests);
        assert_eq!(JvmSources.of_child("main", nothing, true), Ordinary);
        for root in JVM_BUILD_ROOTS {
            assert!(seen(&[root]).opens_a_jvm_build(), "{root}");
            assert!(BUILD_TOOLS.iter().any(|tool| tool.files == [root]), "{root} is not a row of the table");
        }
        assert!(!seen(&["pom.xml", "build.gradle"]).opens_a_jvm_build());
        assert!(!nothing.opens_a_jvm_build());

        let cased = if cfg!(any(windows, target_os = "macos")) { Tests } else { Ordinary };
        assert_eq!(Ordinary.of_child("Tests", seen(&["Cargo.toml"]), false), cased);
        assert_eq!(Ordinary.of_child("tests", seen(&["cargo.toml"]), false), cased);
        assert!(BuildFilesSeen::is_a_build_file(b"Cargo.toml"));
        assert!(!BuildFilesSeen::is_a_build_file(b"Cargo.lock"));

        let names = [TestFileName::of("*_test.go").unwrap()];
        assert_eq!(TestsByPath::WholeFile, Tests.judge_file("main.go", &names));
        assert_eq!(TestsByPath::WholeFile, Ordinary.judge_file("parser_test.go", &names));
        assert_eq!(TestsByPath::WholeFile, JvmSources.judge_file("parser_test.go", &names));
        assert_eq!(TestsByPath::Nothing, Ordinary.judge_file("parser.go", &names));
        assert_eq!(TestsByPath::Nothing, Ordinary.judge_file("parser_test.go", &[]));
        assert_eq!(TestsByPath::TakenBack, TestScope::DeclaredNotTests.judge_file("parser_test.go", &names),
                "a '!' left the toolchain's name on");
        assert_eq!(TestScope::DeclaredNotTests.of_child("tests", seen(&["Cargo.toml"]), false), TestScope::DeclaredNotTests,
                "a Cargo.toml below a '!' turned it back");
        assert_eq!(TestScope::DeclaredNotTests.of_child("src", nothing, true), TestScope::DeclaredNotTests);
    }

    #[test]
    fn a_pattern_overrules_a_build_tool_and_a_later_pattern_overrules_an_earlier_one() {
        use TestScope::{DeclaredNotTests, Ordinary, Tests};
        let declares = |written_at: usize| Some(PatternMatch { written_at, negated: false });
        let takes_back = |written_at: usize| Some(PatternMatch { written_at, negated: true });

        assert_eq!((Tests, Some(0)), Ordinary.overruled_by(None, declares(0)));
        assert_eq!((DeclaredNotTests, Some(2)), Tests.overruled_by(None, takes_back(2)), "a build tool's answer survived a '!'");
        assert_eq!((Tests, None), Tests.overruled_by(None, None));
        assert_eq!((DeclaredNotTests, Some(3)), Tests.overruled_by(Some(1), takes_back(3)));
        assert_eq!((Tests, Some(3)), Tests.overruled_by(Some(3), takes_back(1)), "an earlier pattern overruled a later one below it");
        assert_eq!((DeclaredNotTests, Some(1)), DeclaredNotTests.overruled_by(Some(1), None));

        let root = std::env::temp_dir().join("mezura_declared_scope");
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["proj/tests/fixtures", "proj/spec/helpers", "proj/vendor/tests"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in ["proj/Cargo.toml", "proj/vendor/Cargo.toml"] {
            std::fs::write(root.join(file), "").unwrap();
        }
        let root_str = root.to_str().unwrap().replace('\\', "/");
        let names_root = root_str.len() + 1;
        let patterns = PathPatternMatcher::compile(&["spec/".to_owned(), "!spec/helpers/".to_owned(), "!tests/fixtures/".to_owned(),
                "!vendor/".to_owned(), "!*_test.go".to_owned()], crate::engine::path_patterns::TakingBack::Allowed).unwrap();
        let of = |path: &str| DirectoryScope::of_target(&root.join(path), names_root, &patterns);
        let child = |holder: DirectoryScope, path: &str, build_files: &[&str]| {
            let seen = build_files.iter().fold(BuildFilesSeen::default(), |mut seen, file| { seen.note(file.as_bytes()); seen });
            let name = Path::new(path).file_name().unwrap().to_str().unwrap().to_owned();
            holder.of_child(&name, seen, &root.join(path), &patterns, &mut Vec::new())
        };
        let file = |holder: DirectoryScope, path: &str| holder.of_file(&root.join(path), &patterns, &mut Vec::new()).0;

        let proj = of("proj");
        assert_eq!((Ordinary, None, names_root), (proj.scope, proj.decided_by, proj.names_root));
        assert_eq!((Tests, Some(0)), { let x = of("proj/spec"); (x.scope, x.decided_by) });
        assert_eq!((DeclaredNotTests, Some(1)), { let x = of("proj/spec/helpers"); (x.scope, x.decided_by) },
                "the folders above the target were not read in order");
        assert_eq!((DeclaredNotTests, Some(2)), { let x = of("proj/tests/fixtures"); (x.scope, x.decided_by) });
        assert_eq!(Tests, of("proj/tests").scope);
        assert_eq!((DeclaredNotTests, Some(3)), { let x = of("proj/vendor/tests"); (x.scope, x.decided_by) },
                "a Cargo.toml below a '!' turned the folder back");

        let tests = child(proj, "proj/tests", &["Cargo.toml"]);
        assert_eq!((Tests, None), (tests.scope, tests.decided_by));
        assert_eq!(Tests, file(tests, "proj/tests/a.rs"));
        assert_eq!(DeclaredNotTests, child(tests, "proj/tests/fixtures", &[]).scope);
        let spec = child(proj, "proj/spec", &["Cargo.toml"]);
        assert_eq!((Tests, Some(0)), (spec.scope, spec.decided_by));
        assert_eq!(Tests, child(spec, "proj/spec/unit", &[]).scope);
        assert_eq!(DeclaredNotTests, child(spec, "proj/spec/helpers", &[]).scope);
        let vendor = child(proj, "proj/vendor", &["Cargo.toml"]);
        assert_eq!(DeclaredNotTests, child(vendor, "proj/vendor/tests", &["Cargo.toml"]).scope);
        assert_eq!(DeclaredNotTests, file(proj, "proj/parser_test.go"), "a '!' on a file name left the toolchain's name on");
        assert_eq!(Ordinary, file(proj, "proj/parser.go"));
        assert_eq!(DeclaredNotTests, file(spec, "proj/spec/parser_test.go"), "the later pattern on the file lost to the folder above");

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_target_asks_the_disk_only_where_a_component_is_named_like_a_test_directory() {
        use TestScope::{JvmSources, Ordinary, Tests};
        let root = std::env::temp_dir().join("mezura_test_scope_of_target");
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["cargo/tests/common", "cargo/src/tests", "bare/tests", "jvm/src/test/java", "jvm/src/main", "jvm/test",
                "octave/tests", "r/tests", "outer/tests/app", "cased/Tests", "sbt/core/src/test/scala", "sbt/core/src/main",
                "sbt/core/jvm/src/test", "sbt/api/test", "gradle/app/src/test", "gradle/app/src/main"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in ["cargo/Cargo.toml", "jvm/pom.xml", "octave/DESCRIPTION", "r/DESCRIPTION", "r/NAMESPACE", "cased/Cargo.toml",
                "sbt/build.sbt", "gradle/settings.gradle.kts"] {
            std::fs::write(root.join(file), "").unwrap();
        }
        let of = |path: &str| TestScope::of_target(&root.join(path)).scope;
        let inside = |path: &str| TestScope::of_target(&root.join(path)).inside_jvm_build;

        assert_eq!(of("cargo/tests"), Tests);
        assert_eq!(of("cargo/tests/common"), Tests);
        assert_eq!(of("cargo/src/tests"), Ordinary);
        assert_eq!(of("cargo/src"), Ordinary);
        assert_eq!(of("cargo"), Ordinary);
        assert_eq!(of("bare/tests"), Ordinary);
        assert_eq!(of("jvm/src/test/java"), Tests);
        assert_eq!(of("jvm/src/test"), Tests);
        assert_eq!(of("jvm/src/main"), Ordinary);
        assert_eq!(of("jvm/src"), JvmSources);
        assert_eq!(of("jvm/test"), Ordinary);
        assert!(!inside("jvm/src"), "a pom.xml declares no build for the folders around it");
        assert_eq!(of("octave/tests"), Ordinary);
        assert_eq!(of("r/tests"), Tests);
        assert_eq!(of("outer/tests/app"), Ordinary, "a folder above the project is never taken on its name");
        let cased = if cfg!(any(windows, target_os = "macos")) { Tests } else { Ordinary };
        assert_eq!(of("cased/Tests"), cased);

        assert_eq!((of("sbt"), inside("sbt")), (Ordinary, false), "the target's own listing is the walk's to read");
        assert_eq!((of("sbt/core"), inside("sbt/core")), (Ordinary, true));
        assert_eq!((of("sbt/core/src"), inside("sbt/core/src")), (JvmSources, true));
        assert_eq!(of("sbt/core/src/test"), Tests);
        assert_eq!(of("sbt/core/src/test/scala"), Tests);
        assert_eq!(of("sbt/core/src/main"), Ordinary);
        assert_eq!(of("sbt/core/jvm/src/test"), Tests);
        assert_eq!(of("sbt/api/test"), Ordinary, "a 'test' with no 'src' above it");
        assert_eq!(of("gradle/app/src/test"), Tests);
        assert_eq!((of("gradle/app/src/main"), inside("gradle/app/src/main")), (Ordinary, true));

        std::fs::remove_dir_all(&root).unwrap();
    }
}
