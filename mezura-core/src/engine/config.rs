use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

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

// The languages a run counts, the ones it leaves out, and the extensions it hands to a language of
// your choosing. Each of the three can be answered once for the run and again for a single module.
pub type LanguageNames = ScopedByModule<Vec<String>>;
pub type ForcedLanguages = ScopedByModule<HashMap<String,String>>;

// A setting a module answers differently from the rest of the run, so that one repository can hold
// MATLAB under one directory and Objective-C under another and be counted once.
//
// The fields are private because the two halves are not interchangeable: the run's own answer is the
// one every module that says nothing falls back to, and a map built by hand out of both would lose
// that.
#[derive(Debug,PartialEq,Eq,Clone,Default)]
pub struct ScopedByModule<T> {
    whole_run: T,
    per_module: BTreeMap<String,T>
}

impl<T> ScopedByModule<T> {
    pub fn of_the_whole_run(value: T) -> Self {
        ScopedByModule { whole_run: value, per_module: BTreeMap::new() }
    }

    pub fn of(whole_run: T, per_module: impl IntoIterator<Item = (String,T)>) -> Self {
        ScopedByModule { whole_run, per_module: per_module.into_iter().collect() }
    }

    pub fn get_of_the_whole_run(&self) -> &T {
        &self.whole_run
    }

    // The modules this setting singles out, which is what a run checks its target names against.
    pub fn get_module_names(&self) -> impl Iterator<Item = &str> {
        self.per_module.keys().map(String::as_str)
    }

    pub fn is_scoped(&self) -> bool {
        !self.per_module.is_empty()
    }

    fn get_declared_by(&self, module: Option<&str>) -> Option<&T> {
        module.and_then(|name| self.per_module.get(name))
    }
}

impl ScopedByModule<Vec<String>> {
    // A module that names languages of its own answers instead of the run and not on top of it,
    // which is what lets '--languages rust,ios/swift' mean Swift alone inside 'ios'.
    pub(crate) fn get_names_of_module(&self, module: Option<&str>) -> &[String] {
        self.get_declared_by(module).unwrap_or(&self.whole_run)
    }

    // Every name written anywhere in it, whichever module it was written for: whether a language
    // exists is not a question one module answers differently.
    pub fn get_all_names(&self) -> Vec<String> {
        let mut names = self.whole_run.clone();
        for own in self.per_module.values() {
            let fresh = own.iter().filter(|name| !names.contains(name)).cloned().collect::<Vec<_>>();
            names.extend(fresh);
        }

        names
    }

    pub fn is_empty(&self) -> bool {
        self.whole_run.is_empty() && self.per_module.values().all(Vec::is_empty)
    }

    pub fn of_written_form(names: &[String]) -> Self {
        let mut scoped = LanguageNames::default();
        for name in names {
            let (module, name) = split_off_module_scope(name);
            match module {
                Some(module) => scoped.per_module.entry(module.to_owned()).or_default().push(name.to_owned()),
                None => scoped.whole_run.push(name.to_owned())
            }
        }

        scoped
    }

    pub fn to_written_form(&self) -> Vec<String> {
        self.whole_run.iter().cloned()
                .chain(self.per_module.iter().flat_map(|(module, names)| names.iter()
                        .map(|name| format_module_scope(Some(module), name))))
                .collect()
    }
}

impl ScopedByModule<HashMap<String,String>> {
    // A module keeps every rule of the run it does not answer itself, so naming '.m' inside one
    // module does not quietly take that module's '.pl' rule away with it.
    pub(crate) fn get_rules_of_module(&self, module: Option<&str>) -> Cow<'_, HashMap<String,String>> {
        match self.get_declared_by(module) {
            None => Cow::Borrowed(&self.whole_run),
            Some(own) => {
                let mut merged = self.whole_run.clone();
                merged.extend(own.iter().map(|(claimed, language)| (claimed.clone(), language.clone())));
                Cow::Owned(merged)
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.whole_run.is_empty() && self.per_module.values().all(HashMap::is_empty)
    }

    pub fn of_written_form(pairs: &HashMap<String,String>) -> Self {
        let mut scoped = ForcedLanguages::default();
        for (claimed, language) in pairs {
            let (module, claimed) = split_off_module_scope(claimed);
            match module {
                Some(module) => { scoped.per_module.entry(module.to_owned()).or_default()
                        .insert(claimed.to_owned(), language.clone()); },
                None => { scoped.whole_run.insert(claimed.to_owned(), language.clone()); }
            }
        }

        scoped
    }

    pub fn to_written_form(&self) -> HashMap<String,String> {
        self.whole_run.iter().map(|(claimed, language)| (claimed.clone(), language.clone()))
                .chain(self.per_module.iter().flat_map(|(module, rules)| rules.iter()
                        .map(|(claimed, language)| (format_module_scope(Some(module), claimed), language.clone()))))
                .collect()
    }
}

// Through the written form and not straight into the run's own half: a caller handing over a plain
// list is handing over what somebody wrote, and 'ios/m' in it means the module. Taken literally it
// would be an extension no file has, matching nothing and saying nothing, while every place that
// writes the setting back out spells it as a module scope and reads it back as one.
impl From<Vec<String>> for ScopedByModule<Vec<String>> {
    fn from(names: Vec<String>) -> Self {
        LanguageNames::of_written_form(&names)
    }
}

impl From<HashMap<String,String>> for ScopedByModule<HashMap<String,String>> {
    fn from(pairs: HashMap<String,String>) -> Self {
        ForcedLanguages::of_written_form(&pairs)
    }
}

// 'ios/m' is the extension 'm' inside the module named 'ios'. A module name can hold no slash, so
// the first one always separates the two. Text with nothing on one side of that slash is not a
// scope at all and is left whole, which is what makes '/rust' a language name nobody answers to and
// reports as such, rather than a rule that quietly holds everywhere.
pub fn split_off_module_scope(text: &str) -> (Option<&str>, &str) {
    match text.split_once('/') {
        Some((module, value)) if !module.is_empty() && !value.is_empty() => (Some(module), value),
        _ => (None, text)
    }
}

pub fn format_module_scope(module: Option<&str>, value: &str) -> String {
    match module {
        Some(module) => format!("{module}/{value}"),
        None => value.to_owned()
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
    pub languages_of_interest: LanguageNames,
    pub excluded_languages: LanguageNames,
    pub forced_languages: ForcedLanguages,
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
            languages_of_interest: LanguageNames::default(),
            excluded_languages: LanguageNames::default(),
            forced_languages: ForcedLanguages::default(),
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
