// What the counting needs and nothing else, plus the two types that only it names: a target is a
// place to walk, and the thread counts decide how many walk it. How the answer is shown is decided
// in the binary, which is a crate of its own and never seen from here.
use std::collections::HashMap;


pub const MAX_PRODUCERS_VALUE : usize = 32;
pub const MIN_PRODUCERS_VALUE : usize = 1;
pub const MAX_CONSUMERS_VALUE : usize = 128;
pub const MIN_CONSUMERS_VALUE : usize = 1;
pub const DEF_BRACES_AS_CODE    : bool    = false;
pub const DEF_SEARCH_IN_DOTTED  : bool    = false;
pub const DEF_NO_GITIGNORE      : bool    = false;


// A directory or file that was asked for, and the name it was asked for under. 'None' is a target
// that was given no name, and every one of them shares the '(unnamed)' row of the report.
// The name lives inside the target and not in a list of its own, so that everything which already
// carries the targets carries the modules with them: the saved configuration, the log entry that
// decides whether two runs are comparable, and the echo of the settings in the JSON document.
#[derive(Debug,PartialEq,Eq,Clone)]
pub struct Target {
    pub module: Option<String>,
    // Absolute and resolved, never what was typed
    pub path: String
}

impl Target {
    pub fn of(path: String) -> Self {
        Target { module: None, path }
    }

    pub fn named(module: &str, path: String) -> Self {
        Target { module: Some(module.to_owned()), path }
    }

    // The form that reads back as this exact target. The quotes go around the path and not around
    // the whole thing, because the name is taken from before the first '=' and a leading quote
    // would end up inside it.
    pub fn declared_form(&self) -> String {
        let path = if self.path.contains(char::is_whitespace) {format!("\"{}\"", self.path)} else {self.path.clone()};
        match &self.module {
            Some(name) => format!("{name}={path}"),
            None => path
        }
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

#[derive(Debug,PartialEq,Clone)]
pub struct Threads {
    pub producers: usize,
    pub consumers: usize
}

impl Threads {
    pub fn new(producers: usize, consumers: usize) -> Self {
        Threads {
            producers,
            consumers
        }
    }

    pub fn from(threads: (usize,usize)) -> Self {
        Threads {
            producers: threads.0,
            consumers: threads.1
        }
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

// Everything the counting needs, and nothing else. This is what a library caller builds.
#[derive(Debug,PartialEq,Clone)]
pub struct EngineConfig {
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
    // What a caller with nothing to say but where to look writes. Everything else has a default that
    // matches what the command line would have produced.
    pub fn new(dirs: Vec<String>) -> Self {
        EngineConfig {
            dirs: dirs.into_iter().map(Target::of).collect(),
            braces_as_code: DEF_BRACES_AS_CODE,
            should_search_in_dotted: DEF_SEARCH_IN_DOTTED,
            no_gitignore: DEF_NO_GITIGNORE,
            count_keywords: true,
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

// Targets on one line, for the log entry that decides whether two runs are comparable.
//
// The separator is a comma while nothing is named, which is the only thing this ever wrote and what
// keeps a run after an upgrade from reporting 'modified: dirs' over a difference in punctuation. The
// moment a module exists it has to be whitespace: inside a comma list a name carries on to the paths
// after it, so 'frontend=./web,./ui' is one module of two directories, and an unnamed target written
// after a named one with a comma between them would be read back as part of it.
pub fn targets_to_string(targets: &[Target]) -> String {
    if targets.iter().all(|x| x.module.is_none()) {
        targets.iter().map(|x| x.path.clone()).collect::<Vec<_>>().join(",")
    } else {
        targets.iter().map(Target::declared_form).collect::<Vec<_>>().join(" ")
    }
}


