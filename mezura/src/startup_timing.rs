// Where the time before and after the count went, for the same question 'MEZURA_PHASE_TIMING'
// answers about the counting threads. Off unless that variable says otherwise, and the report goes
// to stderr.
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use mezura_core::prints_phase_timing;

const STEP_COUNT : usize = 11;
const NANOS_PER_MILLISECOND : f64 = 1_000_000.0;

static NANOS : [AtomicU64; STEP_COUNT] = [const { AtomicU64::new(0) }; STEP_COUNT];
static LANGUAGE_FILES : AtomicU64 = AtomicU64::new(0);

/// One serial stretch of `main`. `Migration` holds the two below it and `Args` through `Resolve`
/// together hold everything but the remainder the report calls "other".
#[derive(Clone, Copy)]
pub enum Step {
    Args,
    Paths,
    Migration,
    MigrationHash,
    MigrationCheck,
    Languages,
    Conflicts,
    Config,
    Resolve,
    Run,
    Printing
}

/// Kept alive for the whole of `main`, since dropping it is what prints the second line, whichever
/// way the run ends.
pub struct Report {
    start: Option<Instant>,
    startup_was_printed: bool
}

impl Report {
    pub fn of_this_run() -> Report {
        Report {start: stamp(), startup_was_printed: false}
    }

    /// Prints what happened before the count. Called just above it and not left to the drop below,
    /// or a scan of a large tree holds these figures back for as long as it takes to count.
    pub fn print_startup(&mut self) {
        let Some(start) = self.start else {return};
        self.startup_was_printed = true;

        let before_run = nanos_since(start);
        let measured = [Step::Args, Step::Paths, Step::Migration, Step::Languages, Step::Conflicts,
                Step::Config, Step::Resolve].into_iter().map(get).sum::<u64>();
        let files = LANGUAGE_FILES.load(Ordering::Relaxed) as usize;
        let file_word = if files == 1 {"file"} else {"files"};
        let (args, paths) = (format_millis(get(Step::Args)), format_millis(get(Step::Paths)));
        let (migration, hash, check) = (format_millis(get(Step::Migration)),
                format_millis(get(Step::MigrationHash)), format_millis(get(Step::MigrationCheck)));
        let (languages, conflicts) = (format_millis(get(Step::Languages)), format_millis(get(Step::Conflicts)));
        let (config, resolve) = (format_millis(get(Step::Config)), format_millis(get(Step::Resolve)));
        let other = format_millis(before_run.saturating_sub(measured));
        let before = format_millis(before_run);

        eprintln!("[startup] args {args} ms | paths {paths} ms | migration {migration} ms (hash {hash}, \
check {check}) | languages {languages} ms ({files} {file_word}) | \
conflicts {conflicts} ms | config {config} ms | resolve {resolve} ms | other {other} ms | \
before run {before} ms");
    }
}

impl Drop for Report {
    fn drop(&mut self) {
        let Some(start) = self.start else {return};
        // A run that answered '--version' or refused a command never reached the line above, and
        // where its few milliseconds went is the whole of what there is to say about it
        if !self.startup_was_printed {
            self.print_startup();
        }

        eprintln!("[finish] run {} ms | printing {} ms | whole of main {} ms",
                format_millis(get(Step::Run)), format_millis(get(Step::Printing)),
                format_millis(nanos_since(start)));
    }
}

pub fn measure<T>(step: Step, work: impl FnOnce() -> T) -> T {
    let at = stamp();
    let done = work();
    record(step, at);

    done
}

/// Opens a step that spans several statements, where a closure would have to be wrapped around code
/// that returns from `main`. `None` when the report is off, and a run with it off never reads the
/// clock here at all.
pub fn stamp() -> Option<Instant> {
    prints_phase_timing().then(Instant::now)
}

pub fn record(step: Step, at: Option<Instant>) {
    if let Some(from) = at && let Some(slot) = NANOS.get(step as usize) {
        slot.fetch_add(nanos_since(from), Ordering::Relaxed);
    }
}

pub fn record_language_files(files: usize) {
    if prints_phase_timing() {
        LANGUAGE_FILES.store(files as u64, Ordering::Relaxed);
    }
}

fn get(step: Step) -> u64 {
    NANOS.get(step as usize).map_or(0, |slot| slot.load(Ordering::Relaxed))
}

fn nanos_since(start: Instant) -> u64 {
    start.elapsed().as_nanos() as u64
}

fn format_millis(nanos: u64) -> String {
    format!("{:.1}", nanos as f64 / NANOS_PER_MILLISECOND)
}
