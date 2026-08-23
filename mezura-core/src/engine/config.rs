use std::collections::HashMap;

pub const MAX_PRODUCERS_VALUE : usize = 32;
pub const MIN_PRODUCERS_VALUE : usize = 1;
pub const MAX_CONSUMERS_VALUE : usize = 128;
pub const MIN_CONSUMERS_VALUE : usize = 1;

pub(crate) const DEF_SEARCH_IN_DOTTED  : bool    = false;
pub(crate) const DEF_NO_GITIGNORE      : bool    = false;
pub(crate) const DEF_NO_IGNORE_FILES   : bool    = false;

#[derive(Debug,PartialEq,Eq,Clone)]
pub struct Target {
    // 'None' shares the '(unnamed)' row of the report with every other unnamed target.
    pub module: Option<String>,
    // What was typed, until the run resolves it: the list 'RunResult.targets' reports holds
    // absolute paths.
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
// together. The tie-breakers keep 'Ord' agreeing with the derived 'Eq': without them two targets
// differing only in case or only in module compare Equal while '==' says otherwise, and a 'dedup'
// over a sorted list drops one of them.
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

// Private so that a number the run cannot work with has no way in: zero scanning threads leaves
// every directory in the queue and answers "nothing found" over a real tree, and zero counting
// threads returns a result claiming files and zero of everything else.
#[derive(Debug,PartialEq,Eq,Clone,Copy)]
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

    pub fn producers(&self) -> usize {
        self.producers
    }

    pub fn consumers(&self) -> usize {
        self.consumers
    }
}

impl From<(usize,usize)> for Threads {
    fn from(threads: (usize,usize)) -> Self {
        Threads::new(threads.0, threads.1)
    }
}

impl Default for Threads {
    fn default() -> Self {
        let threads = num_cpus::get();
        // Four counting threads per core. What they wait on is a blocking file open, so what decides
        // the speed is how many reads are in flight and not how many cores exist.
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

// Every field but 'threads' answers what gets counted. That one answers how fast, and is here only
// because it is saved and reloaded beside the others; the log leaves it out of what makes two runs
// comparable.
#[derive(Debug,PartialEq,Clone)]
pub struct EngineConfig {
    // 'run' resolves these at its entry with the settings beside them, so the flags that filter a
    // pattern's matches and the flags the scan obeys cannot disagree. A relative path is joined to
    // the working directory at that moment.
    pub targets: Vec<Target>,
    pub exclude_dirs: Vec<String>,
    pub languages_of_interest: Vec<String>,
    pub excluded_languages: Vec<String>,
    pub forced_languages: HashMap<String,String>,
    pub threads: Threads,
    pub should_search_in_dotted: bool,
    pub no_gitignore: bool,
    // The '.ignore' and '.rgignore' that ripgrep, the silver searcher and fd read and git does not.
    // A separate answer from the one above, since obeying the repository and obeying the search
    // tools are two decisions: a vendored dependency is usually hidden by one and kept by the other.
    pub no_ignore_files: bool,
    // Hiding the keywords stops the counting too, since nothing else reads them and the work would
    // be thrown away.
    pub count_keywords: bool,
    // Off by default, so a bundle is left out of every figure and reported as skipped
    pub count_minified: bool,
    // The same, for a file whose head says a tool wrote it
    pub count_generated: bool,
    // Off by default: one entry per file, where the run otherwise holds one per language
    pub collect_files: bool
}

// Written out rather than derived, because a derived 'count_keywords' would be false and anyone
// writing 'EngineConfig { targets, ..Default::default() }' would silently get no keywords.
impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            targets: Vec::new(),
            exclude_dirs: Vec::new(),
            languages_of_interest: Vec::new(),
            excluded_languages: Vec::new(),
            forced_languages: HashMap::new(),
            threads: Threads::default(),
            should_search_in_dotted: DEF_SEARCH_IN_DOTTED,
            no_gitignore: DEF_NO_GITIGNORE,
            no_ignore_files: DEF_NO_IGNORE_FILES,
            count_keywords: true,
            count_minified: false,
            count_generated: false,
            collect_files: false
        }
    }
}

impl EngineConfig {
    // Everything but the places to look takes the default the command line would have produced.
    pub fn new(targets: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
        EngineConfig {
            targets: targets.into_iter().map(Target::of).collect(),
            ..Default::default()
        }
    }
}
