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


// A directory or file that was asked for, and the name it was asked for under. 'None' is a target
// that was given no name, and every one of them shares the '(unnamed)' row of the report.
// The name lives inside the target and not in a list of its own, so that everything which already
// carries the targets carries the modules with them: the saved configuration, the log entry that
// decides whether two runs are comparable, and the echo of the settings in the JSON document.
#[derive(Debug,PartialEq,Eq,Clone)]
pub struct Target {
    pub module: Option<String>,
    // Whatever state its holder put it in: as declared it is what was written, prepared it is
    // absolute with its pattern not yet expanded, and the list the run walks, which is also what
    // 'RunResult.targets' reports, holds only resolved absolute paths.
    pub path: String
}

impl Target {
    pub fn of(path: String) -> Self {
        Target { module: None, path }
    }

    pub fn named(module: &str, path: String) -> Self {
        Target { module: Some(module.to_owned()), path }
    }
}

// By path first, case-insensitively where the file system is, so that two spellings of one place sort
// together. The raw path and the name then break the tie, and they are not decoration: 'Ord' has to
// agree with 'Eq', and 'Eq' is derived over both fields. Without them, two targets that differ only
// in case, or only in name, compare Equal while '==' says false, and the first 'dedup' or
// 'binary_search' written over a sorted list of them would quietly drop one.
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

// The counts are read and never written from outside, which is what lets them be private: a number
// the run cannot work with then has no way in. Zero producers left every directory in the queue with
// nothing to drain it and the scan of a real tree answered "nothing found"; zero consumers returned a
// result claiming relevant files and zero of everything at once. Both were silent wrong answers, and
// only the command line was stopping them, which a library caller never goes through.
#[derive(Debug,PartialEq,Clone)]
pub struct Threads {
    producers: usize,
    consumers: usize
}

impl Threads {
    // Clamped rather than refused. What was asked for is readable back through the two methods
    // below, so nothing is hidden, and a count outside the range is a preference the machine cannot
    // honour rather than a mistake worth ending a run over.
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
        // Consumers are oversubscribed hard, because what they wait on is a blocking file open and
        // the number that matters is how many reads are in flight, not how many cores exist. On one
        // machine, going from 22 consumers to 96 cost nothing measurable on a fast disk with a warm
        // cache, won 1.20x on a slow disk, and won 1.97x from cold. The asymmetry is the whole
        // argument: it is free where it does not help.
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

// Everything the counting needs, and nothing else. This is what a library caller builds, and it is
// plain data: nothing in it has to be computed, in any order, before anything else.
#[derive(Debug,PartialEq,Clone)]
pub struct EngineConfig {
    // The places to count, as declared: paths or glob patterns, relative or absolute, exactly as
    // whoever built the configuration wrote them down. 'run' resolves them at its entry with the
    // settings of this same configuration, so the flags that filter a pattern's matches and the
    // flags the walk obeys can never disagree. A relative path is joined to the process working
    // directory at that moment, and 'RunResult.targets' reports what the list resolved to.
    pub dirs: Vec<Target>,
    pub exclude_dirs: Vec<String>,
    pub languages_of_interest: Vec<String>,
    pub excluded_languages: Vec<String>,
    pub forced_languages: HashMap<String,String>,
    pub threads: Threads,
    pub braces_as_code: bool,
    pub should_search_in_dotted: bool,
    pub no_gitignore: bool,
    // '--hide keywords' stops the counting as well as the printing, and nothing else reads the
    // counts, so the work would be thrown away. To the engine that is a question about work.
    pub count_keywords: bool
}

// Written out rather than derived: the derived 'count_keywords' would be false, which is the opposite
// of what the program does by default, and a caller writing 'EngineConfig { dirs, ..Default::default() }'
// would silently get no keywords counted.
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
    // What a caller with nothing to say but where to look writes. Everything else has a default
    // that matches what the command line would have produced.
    pub fn new(dirs: Vec<String>) -> Self {
        EngineConfig {
            dirs: dirs.into_iter().map(Target::of).collect(),
            ..Default::default()
        }
    }

    //Setters used mainly in tests, for the ability to chain many config changes

    pub fn set_exclude_dirs(&mut self, exclude_dirs: Vec<String>) -> &mut Self {
        self.exclude_dirs = exclude_dirs;
        self
    }

    pub fn set_languages_of_interest(&mut self, languages_of_interest: Vec<String>) -> &mut Self {
        self.languages_of_interest = languages_of_interest;
        self
    }

    pub fn set_threads(&mut self, producers: usize, consumers: usize) -> &mut Self {
        self.threads = Threads::new(producers, consumers);
        self
    }

    pub fn set_braces_as_code(&mut self, braces_as_code: bool) -> &mut Self {
        self.braces_as_code = braces_as_code;
        self
    }

    pub fn set_should_search_in_dotted(&mut self, should_search_in_dotted: bool) -> &mut Self {
        self.should_search_in_dotted = should_search_in_dotted;
        self
    }

    pub fn set_no_gitignore(&mut self, no_gitignore: bool) -> &mut Self {
        self.no_gitignore = no_gitignore;
        self
    }

    pub fn set_count_keywords(&mut self, count_keywords: bool) -> &mut Self {
        self.count_keywords = count_keywords;
        self
    }
}
