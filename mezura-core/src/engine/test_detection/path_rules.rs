use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{RepositoryCheckout, TestFileName};
use crate::engine::is_the_same_name;
use crate::engine::path_patterns::{PathPatternMatcher, PatternMatch, TakingBack};
use crate::engine::targets::spell_out;

use super::tool_configuration::{CONFTEST, PYTEST_CONFIGURATION_FILE_NAMES, read_pytest_configuration};

// Every file of a row has to be present, since an Octave package carries a DESCRIPTION too.
// 'src/test' is the one directory two levels down, and the walk holds the 'src' as 'JvmSources'.
// 'go.mod' names no directory beside it. 'testdata' is taken at every depth below it.
const BUILD_TOOLS : [BuildTool; 20] = [
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
    BuildTool { files: &[GO_MODULE_ROOT], test_directories: &[] },
];
// Every 'src/test' below one of these is a subproject's test sources or is never compiled
const JVM_BUILD_ROOTS : [&str; 3] = ["build.sbt", "settings.gradle", "settings.gradle.kts"];
const GO_MODULE_ROOT : &str = "go.mod";
const GO_TEST_DATA_DIRECTORY : &str = "testdata";
const JEST_PATTERNS : [&str; 17] = ["__tests__/", "*.test.js", "*.test.jsx", "*.test.ts", "*.test.tsx", "*.test.mjs",
        "*.test.cjs", "*.test.mts", "*.test.cts", "*.spec.js", "*.spec.jsx", "*.spec.ts", "*.spec.tsx", "*.spec.mjs",
        "*.spec.cjs", "*.spec.mts", "*.spec.cts"];
const VITEST_PATTERNS : [&str; 16] = ["*.test.js", "*.test.jsx", "*.test.ts", "*.test.tsx", "*.test.mjs", "*.test.cjs",
        "*.test.mts", "*.test.cts", "*.spec.js", "*.spec.jsx", "*.spec.ts", "*.spec.tsx", "*.spec.mjs", "*.spec.cjs",
        "*.spec.mts", "*.spec.cts"];
// A trigger names its tool without being read. It is the tool's own file, a gradle or Perl build
// file, or vite's, which vitest reads. A 'package.json' says nothing about where the tests are
// without being read. Pytest's configuration files are the one kind read, and only for the folders
// and names they declare. '.rspec' needs the 'Gemfile' beside it, or the one in a home directory
// makes every 'spec' under it test code.
pub(crate) const TEST_PRESETS : [TestPreset; 13] = [
    TestPreset { name: "jest", trigger_files: &["jest.config.js", "jest.config.ts", "jest.config.mjs", "jest.config.mts",
            "jest.config.cjs", "jest.config.cts", "jest.config.json"], companion_file: None, files_declaring_tests: &[],
            patterns: &JEST_PATTERNS },
    TestPreset { name: "vitest", trigger_files: &["vitest.config.js", "vitest.config.mjs", "vitest.config.cjs",
            "vitest.config.ts", "vitest.config.mts", "vitest.config.cts", "vitest.workspace.js", "vitest.workspace.mjs",
            "vitest.workspace.cjs", "vitest.workspace.ts", "vitest.workspace.mts", "vitest.workspace.cts",
            "vitest.workspace.json", "vite.config.js", "vite.config.mjs", "vite.config.cjs", "vite.config.ts",
            "vite.config.mts", "vite.config.cts"], companion_file: None, files_declaring_tests: &[], patterns: &VITEST_PATTERNS },
    TestPreset { name: "mocha", trigger_files: &[".mocharc.cjs", ".mocharc.js", ".mocharc.mjs", ".mocharc.yaml",
            ".mocharc.yml", ".mocharc.jsonc", ".mocharc.json"], companion_file: None, files_declaring_tests: &[], patterns: &["test/"] },
    TestPreset { name: "pytest", trigger_files: &[CONFTEST], companion_file: None, files_declaring_tests: &PYTEST_CONFIGURATION_FILE_NAMES,
            patterns: &["test_*.py", "*_test.py", CONFTEST] },
    TestPreset { name: "unittest", trigger_files: &[], companion_file: None, files_declaring_tests: &[], patterns: &["test*.py"] },
    TestPreset { name: "rspec", trigger_files: &[".rspec"], companion_file: Some("Gemfile"), files_declaring_tests: &[],
            patterns: &["spec/"] },
    TestPreset { name: "minitest", trigger_files: &[], companion_file: None, files_declaring_tests: &[], patterns: &["test/"] },
    TestPreset { name: "phpunit", trigger_files: &["phpunit.xml", "phpunit.xml.dist", "phpunit.dist.xml"],
            companion_file: None, files_declaring_tests: &[], patterns: &["tests/", "Tests/", "test/"] },
    TestPreset { name: "dotnet", trigger_files: &[], companion_file: None, files_declaring_tests: &[], patterns: &["*Tests/", "*.Test/"] },
    TestPreset { name: "gradle", trigger_files: &["build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts"],
            companion_file: None, files_declaring_tests: &[], patterns: &["src/*Test/", "src/testFixtures/"] },
    TestPreset { name: "jvm", trigger_files: &[], companion_file: None, files_declaring_tests: &[], patterns: &["src/test/"] },
    TestPreset { name: "xt", trigger_files: &["Makefile.PL", "Build.PL", "dist.ini"], companion_file: None, files_declaring_tests: &[],
            patterns: &["xt/"] },
    TestPreset { name: "odin", trigger_files: &[], companion_file: None, files_declaring_tests: &[], patterns: &["*_test.odin"] },
];
const TRIGGER_NAME_LENGTHS : [u64; 256] = build_trigger_name_lengths();
const BUILD_NAME_LENGTHS : [u64; 256] = build_build_name_lengths();

/// A test tool's default names for its test files, as the tool's documentation gives them.
///
/// The walk applies a preset on its own under the directory holding one of its
/// [`trigger_files`](Self::trigger_files), and reports that in [`crate::RunResult::applied_presets`].
/// A caller can also offer a preset by [`name`](Self::name) and put its
/// [`patterns`](Self::patterns) into [`crate::EngineConfig::test_patterns`], which a preset with
/// no trigger file is for. The shipped presets come from [`crate::get_test_presets`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct TestPreset {
    /// The tool, `jest`.
    pub name: &'static str,
    /// The files whose presence says the tool is in use, `jest.config.ts`, which are never read. One
    /// of them in a directory listing turns the preset on under that directory. Empty for a preset
    /// the walk never applies.
    pub trigger_files: &'static [&'static str],
    /// A file that has to sit beside the trigger for it to count, the `Gemfile` beside `.rspec`.
    pub companion_file: Option<&'static str>,
    /// The files read for where the tests are and what they are named, `pytest.ini`. One declaring
    /// the folders turns the preset on under them and nowhere else, and declared names take the
    /// place of [`patterns`](Self::patterns). Only pytest's are read, so the field is empty for every
    /// other tool.
    pub files_declaring_tests: &'static [&'static str],
    /// The patterns, in the shape [`crate::EngineConfig::test_patterns`] takes. `*.test.ts` is a
    /// file name and `__tests__/` a directory name, both matched at any depth under the trigger.
    pub patterns: &'static [&'static str],
}

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
    pub(crate) fn of_child(self, dir_name: &str, build_files: BuildFilesSeen, roots: RootsAbove) -> TestScope {
        if matches!(self, TestScope::Tests | TestScope::DeclaredNotTests) {
            return self;
        }
        let name = dir_name.as_bytes();
        if self == TestScope::JvmSources && is_the_jvm_test_directory(name) {
            return TestScope::Tests;
        }
        if roots.go_module && is_the_same_name(name, GO_TEST_DATA_DIRECTORY.as_bytes()) {
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
        if roots.jvm_build && is_the_jvm_sources_directory(name) {
            return TestScope::JvmSources;
        }
        scope
    }

    pub(crate) fn of_target<'a>(path: &'a Path, checkout: Option<&'a RepositoryCheckout>) -> (DirectoryScope, Option<&'a Path>) {
        let (test_directory, roots) = find_test_directory_along(path, checkout);
        if let Some(found) = test_directory {
            return (DirectoryScope { scope: TestScope::Tests, roots, ..DirectoryScope::default() }, Some(found.directory));
        }
        let is_jvm_sources = path.file_name().zip(find_parent_of(path, checkout)).is_some_and(|(name, parent)| {
            let name = name.as_encoded_bytes();
            (roots.jvm_build && is_the_jvm_sources_directory(name)) || is_jvm_sources(name, parent)
        });
        let scope = if is_jvm_sources { TestScope::JvmSources } else { TestScope::Ordinary };
        (DirectoryScope { scope, roots, ..DirectoryScope::default() }, None)
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

    fn is_undecided(self) -> bool {
        matches!(self, TestScope::Ordinary | TestScope::JvmSources)
    }
}

// A file at the root of a build that gives a directory name a meaning at every depth below it
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RootsAbove {
    pub jvm_build: bool,
    pub go_module: bool,
}

impl RootsAbove {
    fn carry_below(self, build_files: BuildFilesSeen) -> RootsAbove {
        RootsAbove {
            jvm_build: self.jvm_build || build_files.opens_a_jvm_build(),
            go_module: self.go_module || build_files.holds(GO_MODULE_ROOT)
        }
    }
}

// Whether the walk reads the directory's own listing. It does for a directory target. The folder
// holding a file target is read by nobody.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum OwnListing {
    ReadByTheWalk,
    Unread,
}

// A typed pattern is written after every preset and overrules one through 'decided_by', as it
// overrules a build tool
#[derive(Debug, Clone, Default)]
pub(crate) struct DirectoryScope {
    pub scope: TestScope,
    pub roots: RootsAbove,
    pub decided_by: Option<usize>,
    // The offset into the absolute path that a name pattern reads from
    pub names_root: usize,
    pub presets: Option<ActivePresets>,
    // Each counted file below marks them matched, since their folder pattern decided it
    pub decided_by_presets: Option<ActivePresets>,
}

impl DirectoryScope {
    pub(crate) fn of_target(path: &Path, names_root: usize, patterns: &PathPatternMatcher, climb: &PresetClimb,
            own_listing: OwnListing, checkout: Option<&RepositoryCheckout>) -> DirectoryScope
    {
        let (mut found, test_directory) = TestScope::of_target(path, checkout);
        found.names_root = names_root;
        found.presets = climb.find_presets_above(path, own_listing, checkout);
        let Some(absolute) = path.to_str() else { return found };
        if !patterns.is_empty() {
            let declared = patterns.find_last_match_along(absolute, names_root, false);
            (found.scope, found.decided_by) = found.scope.overruled_by(None, declared);
        }
        // The walk from above reaches a build's test directory through a folder a preset took first
        if found.decided_by.is_none() {
            let deciding = found.find_presets_deciding(absolute, false).into_iter()
                    .filter(|hit| test_directory.is_none_or(|directory| hit.is_above(directory)))
                    .map(|hit| hit.preset).collect::<Vec<_>>();
            if !deciding.is_empty() {
                found.scope = TestScope::Tests;
                found.decided_by_presets = Some(Arc::from(deciding));
            }
        }
        found
    }

    // The presets are asked after the typed patterns, so that a typed '!' leaves them undecided
    pub(crate) fn of_child(&self, dir_name: &str, build_files: BuildFilesSeen, child_path: &Path,
            patterns: &PathPatternMatcher, found: &mut Vec<usize>) -> DirectoryScope
    {
        let roots = self.roots.carry_below(build_files);
        let scope = self.scope.of_child(dir_name, build_files, roots);
        let declared = patterns.find_last_match_of(child_path, self.names_root, true, found);
        let (mut scope, decided_by) = scope.overruled_by(self.decided_by, declared);
        let mut decided_by_presets = match decided_by == self.decided_by {
            true => self.decided_by_presets.clone(),
            false => None
        };
        if scope.is_undecided() {
            let mut deciding = Vec::new();
            if self.find_presets_matching(child_path, true, found, |preset| deciding.push(preset.clone())) {
                scope = TestScope::Tests;
                decided_by_presets = Some(Arc::from(deciding));
            }
        }
        DirectoryScope { scope, roots, decided_by, names_root: self.names_root, presets: self.presets.clone(), decided_by_presets }
    }

    // The consumer marks the deciding presets matched once the file is counted, which a skipped file never is
    pub(crate) fn of_file(&self, file_path: &Path, patterns: &PathPatternMatcher, found: &mut Vec<usize>)
        -> (TestScope, Option<usize>, Option<ActivePresets>)
    {
        let declared = patterns.find_last_match_of(file_path, self.names_root, false, found);
        let (scope, decided_by) = self.scope.overruled_by(self.decided_by, declared);
        if scope.is_undecided() {
            let mut deciding = Vec::new();
            if self.find_presets_matching(file_path, false, found, |preset| deciding.push(preset.clone())) {
                return (TestScope::Tests, decided_by, Some(Arc::from(deciding)));
            }
        } else if decided_by == self.decided_by {
            return (scope, decided_by, self.decided_by_presets.clone());
        }
        (scope, decided_by, None)
    }

    pub(crate) fn with_presets_turned_on(&self, turned_on: &[Arc<ActivePreset>]) -> DirectoryScope {
        let presets = self.presets.iter().flat_map(|held| held.iter()).chain(turned_on).cloned().collect::<Vec<_>>();
        DirectoryScope { scope: self.scope, roots: self.roots, decided_by: self.decided_by, names_root: self.names_root,
                presets: Some(Arc::from(presets)), decided_by_presets: self.decided_by_presets.clone() }
    }

    // The earliest place along the path is where the walk decided, and every preset matching there decided
    pub(crate) fn find_presets_deciding(&self, absolute: &str, ends_with_a_file: bool) -> Vec<PresetMatch> {
        let hits = self.presets.as_deref().unwrap_or_default().iter().filter_map(|preset|
                preset.find_first_match_along(absolute, ends_with_a_file)
                .map(|(at, place)| PresetMatch { preset: preset.clone(), pattern: preset.names.patterns[at].clone(),
                        configured_by: preset.configured_by.filter(|_| at < preset.names.declared), place }))
                .collect::<Vec<_>>();
        let Some(earliest) = hits.iter().map(|hit| hit.place.len()).min() else { return hits };
        hits.into_iter().filter(|hit| hit.place.len() == earliest).collect()
    }

    fn find_presets_matching(&self, path: &Path, is_folder: bool, found: &mut Vec<usize>, mut each: impl FnMut(&Arc<ActivePreset>)) -> bool {
        let mut hit = false;
        for preset in self.presets.as_deref().unwrap_or_default() {
            if preset.matches(path, is_folder, found) {
                each(preset);
                hit = true;
            }
        }
        hit
    }
}

pub(crate) type ActivePresets = Arc<[Arc<ActivePreset>]>;

pub(crate) struct ActivePreset {
    pub preset: &'static TestPreset,
    pub trigger_file: &'static str,
    pub configured_by: Option<&'static str>,
    // Absolute, with forward slashes
    pub directory: String,
    names_root: usize,
    // Empty where the preset takes the whole of its directory
    scopes: Vec<String>,
    names: Arc<PresetNames>,
    matched: AtomicBool,
}

impl ActivePreset {
    pub(crate) fn has_matched(&self) -> bool {
        self.matched.load(Ordering::Relaxed)
    }

    pub(crate) fn record_match(&self) {
        if !self.has_matched() {
            self.matched.store(true, Ordering::Relaxed);
        }
    }

    fn matches(&self, path: &Path, is_folder: bool, found: &mut Vec<usize>) -> bool {
        if !self.scopes.is_empty() && !path.to_str().is_some_and(|absolute| self.is_in_scope(absolute)) {
            return false;
        }
        if self.configured_by.is_some() && (is_folder || !is_a_python_file(path)) {
            return false;
        }
        self.names.matcher.matches(path, self.names_root, is_folder, found)
    }

    // Pytest takes a declared name for '.py' files alone, so it never decides at a folder
    fn find_first_match_along(&self, absolute: &str, ends_with_a_file: bool) -> Option<(usize, String)> {
        if !self.is_in_scope(absolute) {
            return None;
        }
        if self.configured_by.is_some() {
            if !ends_with_a_file || !is_a_python_file(Path::new(absolute)) {
                return None;
            }
            return self.names.matcher.find_last_match(absolute, self.names_root, false, &mut Vec::new())
                    .map(|found| (found.written_at, absolute.to_owned()));
        }
        self.names.matcher.find_first_match_along(absolute, self.names_root, ends_with_a_file)
                .filter(|(_, place)| self.is_in_scope(place))
    }

    fn is_in_scope(&self, absolute: &str) -> bool {
        self.scopes.is_empty() || self.scopes.iter().any(|scope| is_inside_scope(absolute, scope))
    }
}

impl fmt::Debug for ActivePreset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} under {} ({})", self.preset.name, self.directory, self.trigger_file)
    }
}

pub(crate) struct PresetMatch {
    pub preset: Arc<ActivePreset>,
    pub pattern: String,
    // None where the pattern is one the configuration did not declare, the 'conftest.py' added beside its names
    pub configured_by: Option<&'static str>,
    // The folder or the file itself, whichever the pattern matched first along the path
    pub place: String,
}

impl PresetMatch {
    fn is_above(&self, directory: &Path) -> bool {
        let place = Path::new(&self.place);
        directory != place && directory.starts_with(place)
    }
}

// Each listing above a target is read once however many targets sit below it. A glob target is
// one target per matched file, and every one of them climbs the same folders.
#[derive(Default)]
pub(crate) struct PresetClimb {
    by_directory: RefCell<HashMap<PathBuf, Vec<Arc<ActivePreset>>>>,
    taken_back: PresetsTakenBack,
}

impl PresetClimb {
    pub(crate) fn new(taken_back: PresetsTakenBack) -> PresetClimb {
        PresetClimb { taken_back, ..PresetClimb::default() }
    }

    // Above a checkout, a preset reads names from the checkout's own folder down, the most its paths
    // can spell, so a preset two folders above the repository never matches the folder between them.
    pub(crate) fn find_presets_above(&self, path: &Path, own_listing: OwnListing, checkout: Option<&RepositoryCheckout>)
    -> Option<ActivePresets>
    {
        let mut directories = collect_folders_up_from(path, checkout).into_iter()
                .skip(usize::from(own_listing == OwnListing::ReadByTheWalk)).collect::<Vec<_>>();
        directories.reverse();
        let mut presets = Vec::new();
        for directory in directories {
            let (names_root, above_the_checkout) = match checkout {
                Some(checkout) if !directory.starts_with(&checkout.root) =>
                        (Path::new(&checkout.root).parent().map_or(0, find_names_root_below), Some(checkout)),
                _ => (find_names_root_below(directory), None)
            };
            presets.extend(self.read_presets_of(directory, names_root, above_the_checkout));
        }
        (!presets.is_empty()).then(|| Arc::from(presets))
    }

    pub(crate) fn into_turned_on(self) -> Vec<Arc<ActivePreset>> {
        self.by_directory.into_inner().into_values().flatten().collect()
    }

    // A folder above a checkout is the repository's, so what its configuration names moves into the checkout
    fn read_presets_of(&self, directory: &Path, names_root: usize, above_the_checkout: Option<&RepositoryCheckout>)
        -> Vec<Arc<ActivePreset>>
    {
        if let Some(known) = self.by_directory.borrow().get(directory) {
            return known.clone();
        }
        let mut turned_on = read_trigger_files_of(directory).turn_on_reading_names_from(directory, names_root, self.taken_back);
        if let Some(checkout) = above_the_checkout {
            for preset in &mut turned_on {
                if let Some(preset) = Arc::get_mut(preset) {
                    preset.scopes = preset.scopes.iter().map(|scope| move_into_checkout(scope, checkout)).collect();
                }
            }
        }
        self.by_directory.borrow_mut().insert(directory.to_path_buf(), turned_on.clone());
        turned_on
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct BuildFilesSeen(u32);

impl BuildFilesSeen {
    pub(crate) fn is_a_build_file(name: &[u8]) -> bool {
        has_a_listed_shape(name, &BUILD_NAME_LENGTHS) && Self::find_index_of(name).is_some()
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

// The trigger, companion and configuration files of every preset, one bit each in table order
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TriggerFilesSeen(u64);

impl TriggerFilesSeen {
    pub(crate) fn is_a_trigger_file(name: &[u8]) -> bool {
        has_a_listed_shape(name, &TRIGGER_NAME_LENGTHS) && Self::find_index_of(name).is_some()
    }

    pub(crate) fn note(&mut self, name: &[u8]) {
        if let Some(index) = Self::find_index_of(name) {
            self.0 |= 1 << index;
        }
    }

    pub(crate) fn turn_on(self, directory: &Path, taken_back: PresetsTakenBack) -> Vec<Arc<ActivePreset>> {
        if self.is_empty() {
            return Vec::new();
        }
        self.turn_on_reading_names_from(directory, find_names_root_below(directory), taken_back)
    }

    fn turn_on_reading_names_from(self, directory: &Path, names_root: usize, taken_back: PresetsTakenBack)
        -> Vec<Arc<ActivePreset>>
    {
        if self.is_empty() {
            return Vec::new();
        }
        let mut turned_on = Vec::new();
        for (at, preset) in TEST_PRESETS.iter().enumerate() {
            if taken_back.holds(at) {
                continue;
            }
            let build_active_preset = |trigger_file, declared: Option<(Vec<String>, &'static str)>, scopes| {
                let (names, configured_by) = match declared {
                    Some((names, file)) => (Arc::new(PresetNames::of_declared(names)), Some(file)),
                    None => (get_preset_names(at), None)
                };
                Arc::new(ActivePreset { preset, trigger_file, configured_by, directory: spell_out(directory), names_root, scopes,
                        names, matched: AtomicBool::new(false) })
            };
            let configuration = match preset.files_declaring_tests.iter().any(|file| self.holds(file)) {
                true => read_pytest_configuration(directory, |file| self.holds(file)),
                false => None
            };
            // Pushed before the trigger beside it, since the report folds the two and keeps the first
            if let Some(configuration) = &configuration && let Some(scopes) = &configuration.testpaths {
                let declared = configuration.python_files.clone().map(|names| (names, configuration.file));
                turned_on.push(build_active_preset(configuration.file, declared, scopes.clone()));
            }
            let Some(trigger_file) = preset.trigger_files.iter().find(|file| self.holds(file)) else { continue };
            if preset.companion_file.is_some_and(|companion| !self.holds(companion)) {
                continue;
            }
            let declared = configuration.as_ref()
                    .and_then(|configuration| configuration.collect_test_shaped_names().map(|names| (names, configuration.file)));
            turned_on.push(build_active_preset(trigger_file, declared, Vec::new()));
        }
        turned_on
    }

    fn is_empty(self) -> bool {
        self.0 == 0
    }

    fn holds(self, file: &str) -> bool {
        Self::find_index_of(file.as_bytes()).is_some_and(|index| self.0 & (1 << index) != 0)
    }

    fn find_index_of(name: &[u8]) -> Option<usize> {
        collect_noted_files().position(|file| is_the_same_name(name, file.as_bytes()))
    }
}

// A preset named with '!@name' is never turned on by a file
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PresetsTakenBack(u32);

impl PresetsTakenBack {
    pub(crate) fn take_back(&mut self, name: &str) -> bool {
        let Some(at) = TEST_PRESETS.iter().position(|preset| preset.name.eq_ignore_ascii_case(name)) else {
            return false;
        };
        self.0 |= 1 << at;
        true
    }

    fn holds(self, at: usize) -> bool {
        self.0 & (1 << at) != 0
    }
}

pub(crate) struct TestDirectory<'a> {
    pub directory: &'a Path,
    pub build_files: Vec<PathBuf>,
}

// The components are read by name, and the disk is asked only at one named like a test
// directory, plus the root files at every directory above the path.
pub(crate) fn find_test_directory_along<'a>(path: &'a Path, checkout: Option<&'a RepositoryCheckout>)
-> (Option<TestDirectory<'a>>, RootsAbove)
{
    let mut directories = collect_folders_up_from(path, checkout);
    directories.reverse();
    let mut jvm_build_root = None;
    // The walk takes 'src' as the sources of a build rooted above it, so 'src/test' needs a root above 'src'
    let mut jvm_build_root_above_parent = None;
    let mut go_module_root = None;
    for directory in directories {
        let roots = RootsAbove { jvm_build: jvm_build_root.is_some(), go_module: go_module_root.is_some() };
        if let (Some(name), Some(parent)) = (directory.file_name(), find_parent_of(directory, checkout)) {
            let name = name.as_encoded_bytes();
            if let Some(root) = &jvm_build_root_above_parent && is_the_jvm_test_directory(name)
                    && parent.file_name().is_some_and(|above| is_the_jvm_sources_directory(above.as_encoded_bytes())) {
                return (Some(TestDirectory { directory, build_files: vec![PathBuf::clone(root)] }), roots);
            }
            if let Some(root) = &go_module_root && is_the_same_name(name, GO_TEST_DATA_DIRECTORY.as_bytes()) {
                return (Some(TestDirectory { directory, build_files: vec![PathBuf::clone(root)] }), roots);
            }
            if let Some((holder, files)) = find_build_of_test_directory(name, parent, checkout) {
                let build_files = files.iter().map(|file| holder.join(file)).collect();
                return (Some(TestDirectory { directory, build_files }), roots);
            }
        }
        if directory != path {
            jvm_build_root_above_parent = jvm_build_root.clone();
            if let Some(file) = find_jvm_build_root(directory) {
                jvm_build_root = Some(directory.join(file));
            }
            if directory.join(GO_MODULE_ROOT).is_file() {
                go_module_root = Some(directory.join(GO_MODULE_ROOT));
            }
        }
    }
    (None, RootsAbove { jvm_build: jvm_build_root.is_some(), go_module: go_module_root.is_some() })
}

struct BuildTool {
    files: &'static [&'static str],
    test_directories: &'static [&'static str],
}

struct PresetNames {
    patterns: Vec<String>,
    declared: usize,
    matcher: PathPatternMatcher,
}

impl PresetNames {
    // Pytest never collects 'conftest.py' as a test module, and it is test code all the same
    fn of_declared(mut patterns: Vec<String>) -> PresetNames {
        let declared = patterns.len();
        if !patterns.iter().any(|pattern| pattern == CONFTEST) {
            patterns.push(CONFTEST.to_owned());
        }
        PresetNames::compile(patterns, declared)
    }

    fn compile(patterns: Vec<String>, declared: usize) -> PresetNames {
        let matcher = PathPatternMatcher::compile(&patterns, TakingBack::Refused).expect("the patterns of a preset compile");
        PresetNames { patterns, declared, matcher }
    }
}

const fn build_trigger_name_lengths() -> [u64; 256] {
    let mut table = [0; 256];
    let mut preset = 0;
    while preset < TEST_PRESETS.len() {
        mark_name_lengths(&mut table, TEST_PRESETS[preset].trigger_files);
        if let Some(companion) = TEST_PRESETS[preset].companion_file {
            mark_name_lengths(&mut table, &[companion]);
        }
        mark_name_lengths(&mut table, TEST_PRESETS[preset].files_declaring_tests);
        preset += 1;
    }
    table
}

const fn build_build_name_lengths() -> [u64; 256] {
    let mut table = [0; 256];
    let mut tool = 0;
    while tool < BUILD_TOOLS.len() {
        mark_name_lengths(&mut table, BUILD_TOOLS[tool].files);
        tool += 1;
    }
    table
}

const fn mark_name_lengths(table: &mut [u64; 256], names: &[&str]) {
    let mut at = 0;
    while at < names.len() {
        let (first, length) = (names[at].as_bytes()[0], names[at].len());
        assert!(length < 64);
        table[first.to_ascii_lowercase() as usize] |= 1 << length;
        table[first.to_ascii_uppercase() as usize] |= 1 << length;
        at += 1;
    }
}

fn has_a_listed_shape(name: &[u8], lengths: &[u64; 256]) -> bool {
    name.len() < 64 && name.first().is_some_and(|first| lengths[*first as usize] & 1 << name.len() != 0)
}

fn collect_noted_files() -> impl Iterator<Item = &'static str> {
    TEST_PRESETS.iter().flat_map(|preset| preset.trigger_files.iter().copied().chain(preset.companion_file)
            .chain(preset.files_declaring_tests.iter().copied()))
}

// Compiled once, since a monorepo turns one preset on in many directories
fn get_preset_names(at: usize) -> Arc<PresetNames> {
    static NAMES : OnceLock<Vec<Arc<PresetNames>>> = OnceLock::new();
    NAMES.get_or_init(|| TEST_PRESETS.iter().map(|preset| {
        let patterns = preset.patterns.iter().map(|pattern| (*pattern).to_owned()).collect();
        Arc::new(PresetNames::compile(patterns, 0))
    }).collect())[at].clone()
}

fn move_into_checkout(scope: &str, checkout: &RepositoryCheckout) -> String {
    let repository = checkout.repository.trim_end_matches('/');
    match is_inside_scope(scope, repository) {
        true => format!("{}{}", checkout.root.trim_end_matches('/'), &scope[repository.len()..]),
        false => scope.to_owned()
    }
}

fn is_a_python_file(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "py")
}

// The walk may join with a backslash
fn is_inside_scope(absolute: &str, scope: &str) -> bool {
    let is_separator = |byte: u8| byte == b'/' || byte == b'\\';
    let folds_case = cfg!(any(windows, target_os = "macos"));
    let (path, scope) = (absolute.as_bytes(), scope.as_bytes());
    path.len() >= scope.len()
            && path.iter().zip(scope).all(|(&a, &b)| a == b || (is_separator(a) && is_separator(b)) || (folds_case && a.eq_ignore_ascii_case(&b)))
            && path.get(scope.len()).is_none_or(|&byte| is_separator(byte))
}

// A name read from under the directory never sees the directory's own name
fn find_names_root_below(directory: &Path) -> usize {
    let spelled = directory.to_string_lossy();
    spelled.len() + usize::from(!spelled.ends_with(['/', '\\']))
}

fn read_trigger_files_of(directory: &Path) -> TriggerFilesSeen {
    let mut seen = TriggerFilesSeen::default();
    let Ok(entries) = fs::read_dir(directory) else { return seen };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let name = file_name.as_encoded_bytes();
        if TriggerFilesSeen::is_a_trigger_file(name) && entry.path().is_file() {
            seen.note(name);
        }
    }
    seen
}

fn find_build_of_test_directory<'a>(name: &[u8], parent: &'a Path, checkout: Option<&'a RepositoryCheckout>)
-> Option<(&'a Path, &'static [&'static str])>
{
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
                    .and_then(|_| find_parent_of(parent, checkout))
        };
        holder.filter(|holder| tool.files.iter().all(|file| holder.join(file).is_file())).map(|holder| (holder, tool.files))
    }))
}

fn collect_folders_up_from<'a>(path: &'a Path, checkout: Option<&'a RepositoryCheckout>) -> Vec<&'a Path> {
    std::iter::successors(Some(path), |directory| find_parent_of(directory, checkout)).collect()
}

// A checkout sits in a temporary folder that belongs to nobody, so above its root the folders are
// the repository's
fn find_parent_of<'a>(directory: &'a Path, checkout: Option<&'a RepositoryCheckout>) -> Option<&'a Path> {
    match checkout {
        Some(checkout) if directory == Path::new(&checkout.root) => Path::new(&checkout.repository).parent(),
        _ => directory.parent()
    }
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

    fn seen(files: &[&str]) -> BuildFilesSeen {
        files.iter().fold(BuildFilesSeen::default(), |mut seen, file| {
            seen.note(file.as_bytes());
            seen
        })
    }

    fn triggers(files: &[&str]) -> TriggerFilesSeen {
        files.iter().fold(TriggerFilesSeen::default(), |mut seen, file| {
            seen.note(file.as_bytes());
            seen
        })
    }

    #[test]
    fn a_build_file_makes_the_test_directory_of_its_tool_and_no_directory_counts_by_its_name_alone() {
        use TestScope::{JvmSources, Ordinary, Tests};
        let nothing = BuildFilesSeen::default();
        let outside = RootsAbove::default();
        let in_jvm = RootsAbove { jvm_build: true, go_module: false };
        for tool in &BUILD_TOOLS {
            let beside = seen(tool.files);
            for directory in tool.test_directories {
                match directory.split_once('/') {
                    None => {
                        assert_eq!(Ordinary.of_child(directory, beside, outside), Tests, "{directory} beside {:?}", tool.files);
                        assert_eq!(Ordinary.of_child(directory, nothing, outside), Ordinary, "{directory} beside nothing");
                    },
                    Some((sources, test)) => {
                        assert_eq!(Ordinary.of_child(sources, beside, outside), JvmSources, "{sources} beside {:?}", tool.files);
                        assert_eq!(Ordinary.of_child(sources, nothing, outside), Ordinary);
                        assert_eq!(Ordinary.of_child(test, beside, outside), Ordinary, "{test} beside {:?} with no {sources} above", tool.files);
                        assert_eq!(JvmSources.of_child(test, nothing, outside), Tests);
                        assert_eq!(JvmSources.of_child("main", nothing, outside), Ordinary);
                    }
                }
            }
        }
        assert_eq!(Ordinary.of_child("src", seen(&["Cargo.toml"]), outside), Ordinary);
        assert_eq!(Ordinary.of_child("__tests__", seen(&["Cargo.toml"]), outside), Ordinary);
        assert_eq!(Ordinary.of_child("tests", seen(&["DESCRIPTION"]), outside), Ordinary, "an Octave package is not an R one");
        assert_eq!(Ordinary.of_child("tests", seen(&["package.json"]), outside), Ordinary);
        assert_eq!(Tests.of_child("examples", nothing, outside), Tests);
        assert_eq!(Tests.of_child("src", seen(&["pom.xml"]), outside), Tests);
        assert_eq!(JvmSources.of_child("tests", seen(&["Cargo.toml"]), outside), Tests);

        assert_eq!(Ordinary.of_child("src", nothing, in_jvm), JvmSources, "a 'src' anywhere under a declared build");
        assert_eq!(Ordinary.of_child("core", nothing, in_jvm), Ordinary);
        assert_eq!(Ordinary.of_child("test", nothing, in_jvm), Ordinary, "a 'test' with no 'src' above it");
        assert_eq!(Ordinary.of_child("tests", seen(&["Cargo.toml"]), in_jvm), Tests);
        assert_eq!(JvmSources.of_child("main", nothing, in_jvm), Ordinary);
        for root in JVM_BUILD_ROOTS {
            assert!(seen(&[root]).opens_a_jvm_build(), "{root}");
            assert!(BUILD_TOOLS.iter().any(|tool| tool.files == [root]), "{root} is not a row of the table");
        }
        assert!(!seen(&["pom.xml", "build.gradle"]).opens_a_jvm_build());
        assert!(!nothing.opens_a_jvm_build());

        let cased = if cfg!(any(windows, target_os = "macos")) { Tests } else { Ordinary };
        assert_eq!(Ordinary.of_child("Tests", seen(&["Cargo.toml"]), outside), cased);
        assert_eq!(Ordinary.of_child("tests", seen(&["cargo.toml"]), outside), cased);
        assert!(BuildFilesSeen::is_a_build_file(b"Cargo.toml"));
        assert!(!BuildFilesSeen::is_a_build_file(b"Cargo.lock"));

        let names = [TestFileName::parse("*_test.go").unwrap()];
        assert_eq!(TestsByPath::WholeFile, Tests.judge_file("main.go", &names));
        assert_eq!(TestsByPath::WholeFile, Ordinary.judge_file("parser_test.go", &names));
        assert_eq!(TestsByPath::WholeFile, JvmSources.judge_file("parser_test.go", &names));
        assert_eq!(TestsByPath::Nothing, Ordinary.judge_file("parser.go", &names));
        assert_eq!(TestsByPath::Nothing, Ordinary.judge_file("parser_test.go", &[]));
        assert_eq!(TestsByPath::TakenBack, TestScope::DeclaredNotTests.judge_file("parser_test.go", &names),
                "a '!' left the toolchain's name on");
        assert_eq!(TestScope::DeclaredNotTests.of_child("tests", seen(&["Cargo.toml"]), outside), TestScope::DeclaredNotTests,
                "a Cargo.toml below a '!' turned it back");
        assert_eq!(TestScope::DeclaredNotTests.of_child("src", nothing, in_jvm), TestScope::DeclaredNotTests);
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
        let climb = PresetClimb::default();
        let of = |path: &str| DirectoryScope::of_target(&root.join(path), names_root, &patterns, &climb, OwnListing::ReadByTheWalk, None);
        let child = |holder: &DirectoryScope, path: &str, build_files: &[&str]| {
            let name = Path::new(path).file_name().unwrap().to_str().unwrap().to_owned();
            holder.of_child(&name, seen(build_files), &root.join(path), &patterns, &mut Vec::new())
        };
        let file = |holder: &DirectoryScope, path: &str| holder.of_file(&root.join(path), &patterns, &mut Vec::new()).0;

        let proj = of("proj");
        assert_eq!((Ordinary, None, names_root), (proj.scope, proj.decided_by, proj.names_root));
        assert_eq!((Tests, Some(0)), { let x = of("proj/spec"); (x.scope, x.decided_by) });
        assert_eq!((DeclaredNotTests, Some(1)), { let x = of("proj/spec/helpers"); (x.scope, x.decided_by) },
                "the folders above the target were not read in order");
        assert_eq!((DeclaredNotTests, Some(2)), { let x = of("proj/tests/fixtures"); (x.scope, x.decided_by) });
        assert_eq!(Tests, of("proj/tests").scope);
        assert_eq!((DeclaredNotTests, Some(3)), { let x = of("proj/vendor/tests"); (x.scope, x.decided_by) },
                "a Cargo.toml below a '!' turned the folder back");

        let tests = child(&proj, "proj/tests", &["Cargo.toml"]);
        assert_eq!((Tests, None), (tests.scope, tests.decided_by));
        assert_eq!(Tests, file(&tests, "proj/tests/a.rs"));
        assert_eq!(DeclaredNotTests, child(&tests, "proj/tests/fixtures", &[]).scope);
        let spec = child(&proj, "proj/spec", &["Cargo.toml"]);
        assert_eq!((Tests, Some(0)), (spec.scope, spec.decided_by));
        assert_eq!(Tests, child(&spec, "proj/spec/unit", &[]).scope);
        assert_eq!(DeclaredNotTests, child(&spec, "proj/spec/helpers", &[]).scope);
        let vendor = child(&proj, "proj/vendor", &["Cargo.toml"]);
        assert_eq!(DeclaredNotTests, child(&vendor, "proj/vendor/tests", &["Cargo.toml"]).scope);
        assert_eq!(DeclaredNotTests, file(&proj, "proj/parser_test.go"), "a '!' on a file name left the toolchain's name on");
        assert_eq!(Ordinary, file(&proj, "proj/parser.go"));
        assert_eq!(DeclaredNotTests, file(&spec, "proj/spec/parser_test.go"), "the later pattern on the file lost to the folder above");

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_trigger_file_is_known_by_its_whole_name_and_a_companion_counts_only_beside_its_trigger() {
        assert!(TriggerFilesSeen::is_a_trigger_file(b"jest.config.ts"));
        assert!(TriggerFilesSeen::is_a_trigger_file(b"Gemfile"), "a companion is noted too");
        assert!(!TriggerFilesSeen::is_a_trigger_file(b"package.json"));
        assert!(!TriggerFilesSeen::is_a_trigger_file(b"jest.config.ts.bak"));
        assert!(!TriggerFilesSeen::is_a_trigger_file(b"conftest.pyc"));
        let none_taken_back = PresetsTakenBack::default();
        let turned_on = triggers(&[".rspec", "Gemfile", "package.json"]).turn_on(Path::new("D:/proj/rails"), none_taken_back);
        assert_eq!(vec![("rspec", ".rspec", "D:/proj/rails".to_owned())],
                turned_on.iter().map(|x| (x.preset.name, x.trigger_file, x.directory.clone())).collect::<Vec<_>>());
        assert!(triggers(&[".rspec"]).turn_on(Path::new("D:/proj/bare"), none_taken_back).is_empty(),
                "a '.rspec' without its Gemfile turned the preset on");
        assert!(triggers(&["Gemfile"]).turn_on(Path::new("D:/proj/gem"), none_taken_back).is_empty(),
                "a companion alone turned the preset on");
    }

    #[test]
    fn every_shipped_preset_compiles_and_its_trigger_files_fit_the_table() {
        for preset in &TEST_PRESETS {
            assert!(!preset.name.is_empty() && !preset.patterns.is_empty(), "{}", preset.name);
            if let Err(error) = PathPatternMatcher::compile(preset.patterns, TakingBack::Refused) {
                panic!("{}: {error:?}", preset.name);
            }
            assert!(preset.companion_file.is_none() || !preset.trigger_files.is_empty(), "{}: a companion of nothing", preset.name);
            assert!(preset.files_declaring_tests.is_empty() || preset.name == "pytest",
                    "{}: its configuration files would be read as pytest's", preset.name);
        }
        let names = collect_noted_files().count();
        assert!(names <= u64::BITS as usize, "{names} trigger, companion and configuration files take more bits than the mask has");
    }

    #[test]
    fn a_pytest_configuration_naming_its_test_folders_turns_the_preset_on_under_them_alone() {
        for name in ["tox.ini", "setup.cfg", "pyproject.toml", "pytest.ini", ".pytest.toml"] {
            assert!(TriggerFilesSeen::is_a_trigger_file(name.as_bytes()), "{name}");
        }
        assert!(!TriggerFilesSeen::is_a_trigger_file(b"tox.ini.bak"));
        let root = std::env::temp_dir().join("mezura_pytest_configuration_presets");
        let _ = std::fs::remove_dir_all(&root);
        for (file, contents) in [("alone/pytest.ini", "[pytest]\ntestpaths = tests\n"), ("beside/pytest.ini", "[pytest]\ntestpaths = tests\n"),
                ("beside/conftest.py", "")] {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, contents).unwrap();
        }
        let none_taken_back = PresetsTakenBack::default();
        let turned_on = |directory: &str, files: &[&str]| triggers(files).turn_on(&root.join(directory), none_taken_back).iter()
                .map(|preset| (preset.preset.name, preset.trigger_file, preset.scopes.clone())).collect::<Vec<_>>();
        let tests_of = |directory: &str| vec![format!("{}/tests", spell_out(&root.join(directory)))];

        assert_eq!(vec![("pytest", "pytest.ini", tests_of("alone"))], turned_on("alone", &["pytest.ini"]));
        let beside = turned_on("beside", &["pytest.ini", "conftest.py"]);
        assert_eq!(vec!["pytest.ini", "conftest.py"], beside.iter().map(|(_, trigger_file, _)| *trigger_file).collect::<Vec<_>>(),
                "the configuration's preset was not pushed first");
        assert_eq!(vec![tests_of("beside"), Vec::new()], beside.into_iter().map(|(_, _, scopes)| scopes).collect::<Vec<_>>(),
                "the conftest.py beside a configuration was narrowed by its testpaths");
        let mut taken_back = PresetsTakenBack::default();
        assert!(taken_back.take_back("pytest"));
        assert!(triggers(&["pytest.ini"]).turn_on(&root.join("alone"), taken_back).is_empty(), "'!@pytest' left the configuration's preset on");

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_scope_holds_the_paths_below_it_whichever_separator_joined_them() {
        assert!(is_inside_scope("D:/proj/tests/a.py", "D:/proj/tests"));
        assert!(is_inside_scope("D:/proj/tests", "D:/proj/tests"));
        assert!(is_inside_scope(r"D:/proj/tests\unit\a.py", "D:/proj/tests"), "a backslash the walk joined with broke the scope");
        assert!(!is_inside_scope("D:/proj/tests2/a.py", "D:/proj/tests"));
        assert!(!is_inside_scope("D:/proj", "D:/proj/tests"));
        assert_eq!(cfg!(any(windows, target_os = "macos")), is_inside_scope("D:/proj/Tests/a.py", "D:/proj/tests"));
    }

    #[test]
    fn a_target_asks_the_disk_only_where_a_component_is_named_like_a_test_directory() {
        use TestScope::{JvmSources, Ordinary, Tests};
        let root = std::env::temp_dir().join("mezura_test_scope_of_target");
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["cargo/tests/common", "cargo/src/tests", "bare/tests", "jvm/src/test/java", "jvm/src/main", "jvm/test",
                "octave/tests", "r/tests", "outer/tests/app", "cased/Tests", "sbt/core/src/test/scala", "sbt/core/src/main",
                "sbt/core/jvm/src/test", "sbt/api/test", "gradle/app/src/test", "gradle/app/src/main", "go/testdata",
                "go/pkg/testdata/deep", "go/pkg/data", "loose/testdata", "rooted/src/test"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in ["cargo/Cargo.toml", "jvm/pom.xml", "octave/DESCRIPTION", "r/DESCRIPTION", "r/NAMESPACE", "cased/Cargo.toml",
                "sbt/build.sbt", "gradle/settings.gradle.kts", "go/go.mod", "rooted/src/settings.gradle"] {
            std::fs::write(root.join(file), "").unwrap();
        }
        let of = |path: &str| TestScope::of_target(&root.join(path), None).0.scope;
        let inside = |path: &str| TestScope::of_target(&root.join(path), None).0.roots.jvm_build;

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
        assert_eq!(of("rooted/src/test"), Ordinary, "a build rooted in 'src' itself took its 'test' for the source set");

        assert_eq!(of("go/testdata"), Tests);
        assert_eq!(of("go/pkg/testdata/deep"), Tests, "a 'testdata' deeper down the module");
        assert_eq!((of("go/pkg/data"), TestScope::of_target(&root.join("go/pkg/data"), None).0.roots.go_module), (Ordinary, true));
        assert_eq!((of("go"), TestScope::of_target(&root.join("go"), None).0.roots.go_module), (Ordinary, false));
        assert_eq!(of("loose/testdata"), Ordinary, "a 'testdata' with no go.mod above it");
        let deep = root.join("go/pkg/testdata/deep");
        let (found, _) = find_test_directory_along(&deep, None);
        let found = found.unwrap();
        assert_eq!((root.join("go/pkg/testdata").as_path(), vec![root.join("go/go.mod")]), (found.directory, found.build_files));

        std::fs::remove_dir_all(&root).unwrap();
    }

    // The checkout sits under a folder named 'test' and the repository's parent has a shorter name,
    // so a preset reading names from where its own folder ends in the repository's spelling would
    // take the checkout whole through mocha's 'test/'
    #[test]
    fn a_checkout_reads_the_folders_above_the_repository_and_never_the_ones_it_was_written_into() {
        let root = std::env::temp_dir().join("mezura_checkout_climb");
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["w/web", "a/test/holder/web/src", "a/test/holder/web/testdata"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in ["w/jest.config.js", "w/.mocharc.json", "w/go.mod", "a/test/holder/web/src/a.test.ts"] {
            std::fs::write(root.join(file), "").unwrap();
        }
        let at = |path: &str| PathBuf::from(spell_out(&root.join(path)));
        let checkout = RepositoryCheckout { root: spell_out(&root.join("a/test/holder/web")), repository: spell_out(&root.join("w/web")) };
        let patterns = PathPatternMatcher::compile(&[] as &[String], TakingBack::Allowed).unwrap();
        let names_root = spell_out(&root.join("a/test/holder")).len() + 1;

        let climb = PresetClimb::default();
        let presets = climb.find_presets_above(&at("a/test/holder/web"), OwnListing::ReadByTheWalk, Some(&checkout)).unwrap();
        assert_eq!(vec!["jest", "mocha"], presets.iter().map(|preset| preset.preset.name).collect::<Vec<_>>());
        assert!(PresetClimb::default().find_presets_above(&at("a/test/holder/web"), OwnListing::ReadByTheWalk, None).is_none());

        let target = DirectoryScope::of_target(&at("a/test/holder/web"), names_root, &patterns, &climb, OwnListing::ReadByTheWalk,
                Some(&checkout));
        assert_eq!(TestScope::Ordinary, target.scope, "mocha's 'test/' took the folder the checkout was written into");
        let sources = DirectoryScope::of_target(&at("a/test/holder/web/src"), names_root, &patterns, &climb,
                OwnListing::ReadByTheWalk, Some(&checkout));
        assert_eq!(TestScope::Tests, sources.of_file(&at("a/test/holder/web/src/a.test.ts"), &patterns, &mut Vec::new()).0);

        assert_eq!(TestScope::Tests, TestScope::of_target(&at("a/test/holder/web/testdata"), Some(&checkout)).0.scope,
                "a 'go.mod' above the repository was not read for its checkout");
        assert_eq!(TestScope::Ordinary, TestScope::of_target(&at("a/test/holder/web/testdata"), None).0.scope);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_configuration_above_a_checkout_names_its_folders_where_the_checkout_holds_them() {
        let root = std::env::temp_dir().join("mezura_checkout_testpaths");
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["w/web/tests", "c/web/tests"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        std::fs::write(root.join("w/pytest.ini"), "[pytest]\ntestpaths = web/tests\n").unwrap();
        let at = |path: &str| PathBuf::from(spell_out(&root.join(path)));
        let checkout = RepositoryCheckout { root: spell_out(&root.join("c/web")), repository: spell_out(&root.join("w/web")) };
        let patterns = PathPatternMatcher::compile(&[] as &[String], TakingBack::Allowed).unwrap();
        let names_root = spell_out(&root.join("c")).len() + 1;

        let climb = PresetClimb::default();
        let tests = DirectoryScope::of_target(&at("c/web/tests"), names_root, &patterns, &climb, OwnListing::ReadByTheWalk, Some(&checkout));
        assert_eq!(TestScope::Tests, tests.of_file(&at("c/web/tests/test_a.py"), &patterns, &mut Vec::new()).0,
                "the folder the configuration names was looked for outside the checkout");
        let elsewhere = DirectoryScope::of_target(&at("c/web"), names_root, &patterns, &climb, OwnListing::ReadByTheWalk, Some(&checkout));
        assert_eq!(TestScope::Ordinary, elsewhere.of_file(&at("c/web/test_b.py"), &patterns, &mut Vec::new()).0);

        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn the_presets_table_of_the_document_is_the_shipped_table() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("TEST_DETECTION.md");
        let document = fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
        let rows = document.lines().filter(|line| line.starts_with("| `@")).map(|line| {
            let cells = line.trim_matches('|').split('|').map(str::trim).collect::<Vec<_>>();
            assert_eq!(4, cells.len(), "a row of the presets table has {} cells: {line}", cells.len());
            (cells[0].trim_matches('`').trim_start_matches('@').to_owned(), collect_backticked(cells[1]), collect_backticked(cells[2]))
        }).collect::<Vec<_>>();
        assert_eq!(TEST_PRESETS.len(), rows.len(), "the table has {} rows for {} presets", rows.len(), TEST_PRESETS.len());

        for preset in &TEST_PRESETS {
            let (_, turned_on_by, patterns) = rows.iter().find(|(name, _, _)| name == preset.name)
                    .unwrap_or_else(|| panic!("'@{}' has no row in the table", preset.name));
            assert_eq!(preset.patterns, patterns.iter().map(String::as_str).collect::<Vec<_>>().as_slice(),
                    "the patterns of '@{}' differ from the shipped ones", preset.name);
            let named = |file: &str| turned_on_by.iter().any(|written| written == file
                    || written.strip_suffix('*').is_some_and(|stem| file.starts_with(stem)));
            for &file in preset.trigger_files.iter().chain(preset.companion_file.iter()).chain(preset.files_declaring_tests) {
                assert!(named(file), "'{file}' turns '@{}' on and its row does not name it", preset.name);
            }
            assert_eq!(preset.trigger_files.is_empty(), turned_on_by.is_empty(),
                    "the row of '@{}' names a file where the preset has none, or the other way round", preset.name);
        }
    }

    fn collect_backticked(cell: &str) -> Vec<String> {
        cell.split('`').skip(1).step_by(2).map(str::to_owned).collect()
    }
}
