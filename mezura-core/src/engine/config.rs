// What the counting needs and nothing else, plus the two types that only it names: a target is a
// place to walk, and the thread counts decide how many walk it. How the answer is shown is decided
// in the binary, which is a crate of its own and never seen from here.
use std::collections::HashMap;

pub const MAX_PRODUCERS_VALUE : usize = 32;
pub const MIN_PRODUCERS_VALUE : usize = 1;
pub const MAX_CONSUMERS_VALUE : usize = 128;
pub const MIN_CONSUMERS_VALUE : usize = 1;

pub(crate) const DEF_BRACES_AS_CODE    : bool    = false;
pub(crate) const DEF_SEARCH_IN_DOTTED  : bool    = false;
pub(crate) const DEF_NO_GITIGNORE      : bool    = false;

// The name lives inside the target rather than in a list of its own, so everything that already
// carries the targets carries the names with them: the saved configuration, the log entry that
// decides whether two runs are comparable, and the settings echoed in the JSON document.
#[derive(Debug,PartialEq,Eq,Clone)]
pub struct Target {
    // 'None' shares the '(unnamed)' row of the report with every other unnamed target.
    pub module: Option<String>,
    // In whatever state its holder put it: as declared it is what was typed, and the list the run
    // walks, which is what 'RunResult.targets' reports, holds resolved absolute paths.
    pub path: String
}

impl Target {
    pub fn of(path: impl AsRef<str>) -> Self {
        Target { module: None, path: path.as_ref().to_owned() }
    }

    pub fn named(module: impl AsRef<str>, path: impl AsRef<str>) -> Self {
        Target { module: Some(module.as_ref().to_owned()), path: path.as_ref().to_owned() }
    }
}

// By path first, ignoring case where the filesystem does, so two spellings of one place sort
// together. The two tie-breakers are not decoration: 'Ord' has to agree with 'Eq', which is derived
// over both fields, or targets differing only in case or only in name compare Equal while '==' says
// otherwise, and the first 'dedup' over a sorted list drops one.
impl Ord for Target {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        crate::engine::targets::path_comparison_key(&self.path)
                .cmp(&crate::engine::targets::path_comparison_key(&other.path))
                .then_with(|| self.path.cmp(&other.path))
                .then_with(|| self.module.cmp(&other.module))
    }
}

impl PartialOrd for Target {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.module {
            Some(name) => write!(formatter, "{name}={}", self.path),
            None => write!(formatter, "{}", self.path)
        }
    }
}

// The two counts are private so that a number the run cannot work with has no way in: zero scanning
// threads leaves every directory in the queue and answers "nothing found" over a real tree, and zero
// counting threads returns a result claiming files and zero of everything else.
#[derive(Debug,PartialEq,Clone)]
pub struct Threads {
    producers: usize,
    consumers: usize
}

impl Threads {
    // Clamped rather than refused: a count outside the range is a preference the machine cannot
    // honour, not a mistake worth ending a run over.
    pub fn new(producers: usize, consumers: usize) -> Self {
        Threads {
            producers: producers.clamp(MIN_PRODUCERS_VALUE, MAX_PRODUCERS_VALUE),
            consumers: consumers.clamp(MIN_CONSUMERS_VALUE, MAX_CONSUMERS_VALUE)
        }
    }

    pub fn from(threads: (usize,usize)) -> Self {
        Threads::new(threads.0, threads.1)
    }

    pub fn producers(&self) -> usize {
        self.producers
    }

    pub fn consumers(&self) -> usize {
        self.consumers
    }
}

impl Default for Threads {
    fn default() -> Self {
        let threads = num_cpus::get();
        // Four counting threads per core. What they wait on is a blocking file open, so what decides
        // the speed is how many reads are in flight and not how many cores exist. Measured, going
        // from 22 to 96 of them costs nothing on a fast disk with a warm cache, wins 1.20x on a slow
        // one and 1.97x from cold: free where it does not help, which is the whole argument.
        if threads <= 4 {
            Threads {
                producers: 2,
                consumers: (threads * 4).clamp(8, MAX_CONSUMERS_VALUE)
            }
        } else {
            Threads {
                producers: (threads / 2).clamp(2, MAX_PRODUCERS_VALUE),
                consumers: (threads * 4).clamp(8, MAX_CONSUMERS_VALUE)
            }
        }
    }
}

// Plain data, so nothing has to be computed in any order before anything else, and no setters, so
// one expression says everything and can never fall behind the struct:
//
//     let config = EngineConfig { count_keywords: false, ..EngineConfig::new(["./src"]) };
//
// Every field but 'threads' answers what gets counted. That one answers how fast, and is here only
// because it is saved and reloaded beside the others; the log leaves it out of what makes two runs
// comparable.
#[derive(Debug,PartialEq,Clone)]
pub struct EngineConfig {
    // Paths or glob patterns, relative or absolute, exactly as they were written. 'run' resolves
    // them at its entry with these same settings, so the flags that filter a pattern's matches and
    // the flags the scan obeys cannot disagree. A relative path is joined to the working directory
    // at that moment, and 'RunResult.targets' reports what they resolved to.
    pub dirs: Vec<Target>,
    pub exclude_dirs: Vec<String>,
    pub languages_of_interest: Vec<String>,
    pub excluded_languages: Vec<String>,
    pub forced_languages: HashMap<String,String>,
    pub threads: Threads,
    pub braces_as_code: bool,
    pub should_search_in_dotted: bool,
    pub no_gitignore: bool,
    // Hiding the keywords stops the counting too, since nothing else reads them and the work would
    // be thrown away.
    pub count_keywords: bool
}

// Written out rather than derived, because a derived 'count_keywords' would be false and anyone
// writing 'EngineConfig { dirs, ..Default::default() }' would silently get no keywords.
impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            dirs: Vec::new(),
            exclude_dirs: Vec::new(),
            languages_of_interest: Vec::new(),
            excluded_languages: Vec::new(),
            forced_languages: HashMap::new(),
            threads: Threads::default(),
            braces_as_code: DEF_BRACES_AS_CODE,
            should_search_in_dotted: DEF_SEARCH_IN_DOTTED,
            no_gitignore: DEF_NO_GITIGNORE,
            count_keywords: true
        }
    }
}

impl EngineConfig {
    // Everything but the places to look takes the default the command line would have produced.
    pub fn new(dirs: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        EngineConfig {
            dirs: dirs.into_iter().map(Target::of).collect(),
            ..Default::default()
        }
    }
}
