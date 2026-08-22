use std::io::{IsTerminal, Write};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use mezura_core::ScanProgress;
use unicode_width::UnicodeWidthChar;

use crate::config_manager::Configuration;
use crate::theme::{Style, measure_columns};

const WALK_HEADING : &str = "Analyzing targets";
const MOTION_APPEARS_AFTER_MS : u128 = 150;
// The step constants should be kept as multiples of tick, or a 251 behaves as a 300
const TICK : Duration = Duration::from_millis(50);
const THREE_DOTS_STEP_MS : u128 = 300;
const NUMBER_REFRESH_MS : u128 = 300;
// An interval of microseconds turns one finished file into hundreds of millions of lines a second,
// so a sample taken sooner than this is thrown away.
const MIN_RATE_INTERVAL_SECS : f64 = 0.05;
const RAINBOW_CYCLE_MS : u128 = 3_000;
// The count is right-aligned in this field, so a new digit fills reserved space to its left instead
// of pushing the text beside it.
const COUNT_FIELD_WIDTH : usize = "9,999,999".len();
// The bar only exists when what remains is estimated to outlast this. A queue size would say
// something different on every machine, half a second means half a second everywhere.
const BAR_APPEARS_OVER_ESTIMATED_MS : u128 = 500;
// The bar's width clamp; what else fits beside it is decided by 'fit_parsing_frame'
const MAX_BAR_CELLS : usize = 49;
const MIN_BAR_CELLS : usize = 18;
const FALLBACK_WIDTH : usize = 80;
// A revision name is the user's own text and can be a whole hash, so past this many terminal
// columns it is cut with '..'
const REVISION_NAME_MAX_COLUMNS : usize = 18;
const PERCENT_FIELD_WIDTH : usize = "100".len();
// Right-aligned in reserved fields, for the reason the walk count is
const FILES_RATE_FIELD_WIDTH : usize = "999,999".len();
const LINES_RATE_FIELD_WIDTH : usize = "99,999,999".len();
// Read by both the format strings and the width arithmetic of 'fit_parsing_frame', so the two
// cannot drift apart by a column
const FILES_RATE_SUFFIX : &str = " files/s";
const LINES_RATE_SUFFIX : &str = " lines/s";
// Erases to the end of the screen and not just the line: a terminal that narrowed mid-run rewraps
// the row by itself and welds what spilled onto the row below. Safe because nothing permanent is
// ever printed below a live line.
const ERASE_BELOW : &str = "\r\x1b[0J";
// Erase, then up onto the blank line the first frame opened
const ERASE_BELOW_AND_RETREAT : &str = "\r\x1b[0J\x1b[1A";
// A static and not an argument: the guard that waits for the checkout removals is built before
// there is a configuration to ask, since it has to be the last thing this process drops.
static ANIMATIONS_HIDDEN : AtomicBool = AtomicBool::new(false);

const _ : () = {
    assert!(TICK.as_millis() > 0);
    assert!(THREE_DOTS_STEP_MS > 0 && NUMBER_REFRESH_MS > 0);
    assert!(MIN_BAR_CELLS > 0 && MIN_BAR_CELLS <= MAX_BAR_CELLS);
    assert!(REVISION_NAME_MAX_COLUMNS > 2);
};

// Everything transient goes to stderr and is erased before anything permanent prints, and none of
// it exists unless the output is a terminal: a piped run stays byte-identical with a build that had
// none of this. The gate is 'is_terminal' and never CLICOLOR_FORCE, which forces color into pipes
// and must not force motion into them.
pub struct AnimatedDisplay {
    should_stop: Arc<AtomicBool>,
    animator: Mutex<Option<JoinHandle<()>>>,
    parting: Parting,
    opened_own_line: Arc<AtomicBool>
}

// What the erased line is replaced with when a display finishes. 'Retreat' gives back the line the
// display opened, cursor and all, so the permanent output is byte for byte what a run without the
// display prints.
enum Parting {
    Settle(String),
    Erase,
    Retreat
}

impl AnimatedDisplay {
    // Idempotent, and called on every path out of the run.
    pub fn finish(&self) {
        let Some(animator) = self.animator.lock().unwrap().take() else { return };
        self.should_stop.store(true, Ordering::Relaxed);
        // The animator waits out its tick in 'park_timeout', so it is woken rather than waited for.
        animator.thread().unpark();
        let _ = animator.join();
        if matches!(self.parting, Parting::Retreat) && !self.opened_own_line.load(Ordering::Relaxed) {
            return;
        }
        write_parting(&self.parting);
    }
}

impl Default for AnimatedDisplay {
    fn default() -> Self {
        AnimatedDisplay {
            should_stop: Arc::new(AtomicBool::new(false)),
            animator: Mutex::new(None),
            parting: Parting::Erase,
            opened_own_line: Arc::new(AtomicBool::new(false))
        }
    }
}

// A display that goes out of scope erases itself, so a function full of early returns cannot leave
// a line animating over whatever its caller prints next.
impl Drop for AnimatedDisplay {
    fn drop(&mut self) {
        self.finish();
    }
}

pub fn set_animations_hidden(hidden: bool) {
    ANIMATIONS_HIDDEN.store(hidden, Ordering::Relaxed);
}

// Phase timing prints its own report over anything moving, so it counts as hidden too.
fn animations_are_hidden() -> bool {
    ANIMATIONS_HIDDEN.load(Ordering::Relaxed) || !std::io::stderr().is_terminal()
            || mezura_core::prints_phase_timing()
}

// Prints the walk heading, animated when both output streams are a terminal and static otherwise.
pub fn start_walk_display(config: &Configuration, progress: Arc<ScanProgress>) -> AnimatedDisplay {
    if config.view.hidden.directory_info || !config.view.prints_text() {
        return AnimatedDisplay::default();
    }
    let heading = crate::theme::get_active().heading.paint(WALK_HEADING).to_string();
    if !std::io::stdout().is_terminal() || animations_are_hidden() {
        println!("\n{heading}...");
        return AnimatedDisplay::default();
    }

    // The line begins on stdout, dotless and unfinished; the animator redraws it in place from
    // stderr, and 'finish' settles it into exactly the line the static branch above prints
    print!("\n{heading}");
    let _ = std::io::stdout().flush();
    let stop = Arc::new(AtomicBool::new(false));
    let animator = {
        let (progress, stop, heading) = (progress, stop.clone(), heading.clone());
        thread::Builder::new().name("animated-display".to_owned())
                .spawn(move || animate_walk_line(&progress, &stop, &heading)).ok()
    };
    if animator.is_none() {
        println!("...");
    }

    AnimatedDisplay {
        should_stop: stop,
        animator: Mutex::new(animator),
        parting: Parting::Settle(format!("{heading}...")),
        opened_own_line: Arc::new(AtomicBool::new(false))
    }
}

// The transient line of a '--diff' side that is a git revision: its name with the files discovered
// while the checkout is scanned, then the parsing frame once the scan ends. It erases itself
// completely, so the printed comparison carries no trace of it.
pub fn start_revision_display(config: &Configuration, git_revision: &str, progress: Arc<ScanProgress>,
        already_written: bool) -> AnimatedDisplay {
    if animations_are_hidden() {
        return AnimatedDisplay::default();
    }

    // A write that finished before this display opened gets no writing label at all.
    let shown_name = cap_revision_name(git_revision);
    let writing = (!already_written).then(||
            format!("{} '{shown_name}'", crate::theme::get_active().heading.paint("Writing out")));
    let counting = format!("{} '{shown_name}'", crate::theme::get_active().heading.paint("Counting"));
    let show_bar_and_rates = !config.view.hidden.progress_bar;
    let charset = config.view.progress_bar.get_charset();
    let stop = Arc::new(AtomicBool::new(false));
    let opened = Arc::new(AtomicBool::new(false));
    let animator = {
        let (progress, stop, opened) = (progress, stop.clone(), opened.clone());
        thread::Builder::new().name("animated-display".to_owned())
                .spawn(move || animate_revision_line(&progress, &stop, &opened, writing.as_deref(),
                        &counting, show_bar_and_rates, charset)).ok()
    };

    AnimatedDisplay {
        should_stop: stop,
        animator: Mutex::new(animator),
        parting: Parting::Retreat,
        opened_own_line: opened
    }
}

// The transient line of a parse that outlives the walk. It erases itself completely; nothing
// permanent is printed.
pub fn start_parsing_display(config: &Configuration, progress: Arc<ScanProgress>) -> AnimatedDisplay {
    if config.view.hidden.parsing_info || animations_are_hidden() {
        return AnimatedDisplay::default();
    }
    if progress.get_files_found() == progress.get_files_parsed() {
        return AnimatedDisplay::default();
    }

    let show_bar_and_rates = !config.view.hidden.progress_bar;
    let charset = config.view.progress_bar.get_charset();
    let stop = Arc::new(AtomicBool::new(false));
    let animator = {
        let (progress, stop) = (progress, stop.clone());
        thread::Builder::new().name("animated-display".to_owned())
                .spawn(move || animate_parsing_line(&progress, &stop, show_bar_and_rates, charset)).ok()
    };

    AnimatedDisplay {
        should_stop: stop,
        animator: Mutex::new(animator),
        parting: Parting::Erase,
        opened_own_line: Arc::new(AtomicBool::new(false))
    }
}

// Lives at the top of main, so its drop is the last thing before the process exits on every path:
// the checkout removals a '--diff' left running in the background must not be outlived.
pub struct RemovalsGuard;

impl Drop for RemovalsGuard {
    fn drop(&mut self) {
        if !animations_are_hidden() {
            animate_removals_line();
        }
        crate::git::await_checkout_removals();
    }
}

fn write_parting(parting: &Parting) {
    let text = match parting {
        Parting::Settle(line) => format!("{ERASE_BELOW}{line}\n"),
        Parting::Erase => ERASE_BELOW.to_owned(),
        Parting::Retreat => ERASE_BELOW_AND_RETREAT.to_owned()
    };
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(text.as_bytes());
    let _ = stderr.flush();
}

fn cap_revision_name(name: &str) -> String {
    if measure_columns(name) <= REVISION_NAME_MAX_COLUMNS {
        return name.to_owned();
    }
    let mut kept = String::new();
    let mut columns = 0;
    for character in name.chars() {
        let next = columns + character.width().unwrap_or(0);
        if next > REVISION_NAME_MAX_COLUMNS - 2 {
            break;
        }
        columns = next;
        kept.push(character);
    }
    kept + ".."
}

// One less than the terminal says: the last column is where terminals keep their pending-wrap
// quirks.
fn read_budget() -> usize {
    match terminal_size::terminal_size_of(std::io::stderr()) {
        Some((width, _)) => (width.0 as usize).saturating_sub(1),
        None => FALLBACK_WIDTH - 1
    }
}

// What of the parsing frame fits: the bar takes whatever is left within its clamp, lines/s is given
// up first and files/s after, and a plan of nothing at all means the count stands alone.
#[derive(Debug, PartialEq, Eq)]
struct FramePlan {
    bar_cells: usize,
    files_rate: bool,
    lines_rate: bool
}

impl FramePlan {
    const COUNT_ALONE : FramePlan = FramePlan { bar_cells: 0, files_rate: false, lines_rate: false };
}

// Cross-multiplied, so no pace is ever divided out of nothing: files not yet moving are a pace of
// zero, and at a pace of zero any remainder at all outlasts the threshold
fn needs_a_bar(remaining: usize, done: usize, elapsed_ms: u128) -> bool {
    remaining as u128 * elapsed_ms > done as u128 * BAR_APPEARS_OVER_ESTIMATED_MS
}

fn fit_parsing_frame(budget: usize, label_width: usize, count_width: usize) -> FramePlan {
    let count_block = 2 * count_width + "/".len() + " files".len();
    let files_block = "  ".len() + FILES_RATE_FIELD_WIDTH + FILES_RATE_SUFFIX.len();
    let lines_block = " | ".len() + LINES_RATE_FIELD_WIDTH + LINES_RATE_SUFFIX.len();
    let bar_block = "[] ".len() + PERCENT_FIELD_WIDTH + "% ".len();
    let fixed = label_width + count_block;

    let with_both = budget.saturating_sub(fixed + files_block + lines_block + bar_block);
    if with_both >= MIN_BAR_CELLS {
        return FramePlan { bar_cells: with_both.min(MAX_BAR_CELLS), files_rate: true, lines_rate: true };
    }
    let with_files = budget.saturating_sub(fixed + files_block + bar_block);
    if with_files >= MIN_BAR_CELLS {
        return FramePlan { bar_cells: with_files.min(MAX_BAR_CELLS), files_rate: true, lines_rate: false };
    }
    let bar_alone = budget.saturating_sub(fixed + bar_block);
    if bar_alone >= MIN_BAR_CELLS {
        return FramePlan { bar_cells: bar_alone.min(MAX_BAR_CELLS), files_rate: false, lines_rate: false };
    }
    FramePlan::COUNT_ALONE
}

struct RateSampler {
    baseline: (Instant, usize, usize),
    rates: Option<(usize, usize)>,
    shown_parsed: usize
}

impl RateSampler {
    fn start(progress: &ScanProgress) -> RateSampler {
        let parsed = progress.get_files_parsed();
        RateSampler {
            baseline: (Instant::now(), parsed, progress.get_lines_counted()),
            rates: None,
            shown_parsed: parsed
        }
    }

    // The delta between two samples and never the average since the start. A sample under the
    // minimum interval leaves the baseline where it was: moving it would drop that window's work
    // from the next delta.
    fn sample(&mut self, progress: &ScanProgress) {
        let (now, parsed, lines) = (Instant::now(), progress.get_files_parsed(), progress.get_lines_counted());
        let seconds = now.duration_since(self.baseline.0).as_secs_f64();
        if seconds > MIN_RATE_INTERVAL_SECS {
            self.rates = Some((((parsed - self.baseline.1) as f64 / seconds) as usize,
                    ((lines - self.baseline.2) as f64 / seconds) as usize));
            self.baseline = (now, parsed, lines);
        }
        self.shown_parsed = parsed;
    }
}

// The one seam every animator writes frames through: an unchanged frame on an unchanged budget is
// not written, a changed budget forces a redraw over an erase since the terminal may have rewrapped
// the row on its own, and a frame wider than the budget is not drawn at all.
struct TransientRow {
    last_written: String,
    last_budget: Option<usize>,
    drew_first_frame: bool
}

impl TransientRow {
    fn new() -> TransientRow {
        TransientRow {
            last_written: String::new(),
            last_budget: None,
            drew_first_frame: false
        }
    }

    // 'on_first_frame' runs once, right before the first frame reaches the screen, so a display
    // opens its own line only if it ever draws anything.
    fn draw(&mut self, frame: &str, budget: usize, on_first_frame: impl FnOnce()) {
        let resized = self.last_budget.is_some_and(|previous| previous != budget);
        self.last_budget = Some(budget);
        let width = measure_columns(frame);
        if width > budget {
            if !self.last_written.is_empty() {
                erase_row();
                self.last_written.clear();
            }
            return;
        }
        if !resized && frame == self.last_written {
            return;
        }
        if !self.drew_first_frame {
            on_first_frame();
            self.drew_first_frame = true;
        }
        if resized && !self.last_written.is_empty() {
            redraw_resized_line(frame);
        } else {
            overwrite_transient_line(frame, width, measure_columns(&self.last_written), budget);
        }
        self.last_written = frame.to_owned();
    }
}

// Never an erase between frames, and one write per frame: an erase that reaches the screen before
// its rewrite is a blank the eye reads as flicker, and an unbuffered stderr sends every piece of a
// format string as its own write.
fn overwrite_transient_line(text: &str, width: usize, previous_width: usize, budget: usize) {
    let padding = " ".repeat(previous_width.min(budget).saturating_sub(width));
    let mut stderr = std::io::stderr().lock();
    // The trailing return parks the cursor at column 0, one still spot instead of a blink hopping
    // wherever the text's tail happens to end.
    let _ = stderr.write_all(format!("\r{text}{padding}\r").as_bytes());
    let _ = stderr.flush();
}

// The one frame write that begins with an erase: after a resize, flicker beats a welded row.
fn redraw_resized_line(text: &str) {
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(format!("{ERASE_BELOW}{text}\r").as_bytes());
    let _ = stderr.flush();
}

// A frame that stopped fitting leaves the row empty rather than stale.
fn erase_row() {
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(ERASE_BELOW.as_bytes());
    let _ = stderr.flush();
}

// Opened once before the first frame and given back by 'Parting::Retreat'.
fn open_line_below() {
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(b"\n");
    let _ = stderr.flush();
}

// On the calling thread and not on an animator of its own: the caller has nothing left to do but
// wait for the same event.
fn animate_removals_line() {
    let started = Instant::now();
    let mut row = TransientRow::new();
    while let Some(revision) = crate::git::find_running_removal() {
        thread::sleep(TICK);
        let elapsed = started.elapsed().as_millis();
        if elapsed < MOTION_APPEARS_AFTER_MS {
            continue;
        }
        let budget = read_budget();
        let label = format!("{} '{}'", crate::theme::get_active().heading.paint("Cleaning up"),
                cap_revision_name(&revision));
        let frame = format_walk_frame(&label, elapsed, 0, budget);
        row.draw(&frame, budget, open_line_below);
    }
    if row.drew_first_frame {
        write_parting(&Parting::Retreat);
    }
}

fn animate_walk_line(progress: &ScanProgress, stop: &AtomicBool, heading: &str) {
    let started = Instant::now();
    let mut row = TransientRow::new();
    let mut shown_files = 0;
    let mut last_number_slot = 0;
    let mut frozen_elapsed = None;
    while !stop.load(Ordering::Relaxed) {
        // Woken early by 'finish'. A spurious wake costs nothing: every figure below is worked out
        // from this thread's own clock, and an unchanged frame is not written.
        thread::park_timeout(TICK);
        let elapsed = started.elapsed().as_millis();
        if elapsed < MOTION_APPEARS_AFTER_MS {
            continue;
        }
        // When a scanning thread has died nothing calls 'finish' until the run returns its error,
        // so the dots stop moving rather than claim a scan that is still going.
        if frozen_elapsed.is_none() && progress.is_walk_done() {
            frozen_elapsed = Some(elapsed);
        }
        let budget = read_budget();
        let number_slot = elapsed / NUMBER_REFRESH_MS;
        if number_slot != last_number_slot {
            last_number_slot = number_slot;
            shown_files = progress.get_files_found();
        }
        let frame = format_walk_frame(heading, frozen_elapsed.unwrap_or(elapsed), shown_files, budget);
        row.draw(&frame, budget, || {});
    }
}


// One line for the whole side, and the label follows the phase: 'Writing out' until the first file
// of the inner run is found, which is when git has finished materialising the revision, 'Counting'
// after.
fn animate_revision_line(progress: &ScanProgress, stop: &AtomicBool, opened: &AtomicBool,
        writing_label: Option<&str>, counting_label: &str, show_bar_and_rates: bool, charset: &str) {
    let started = Instant::now();
    let counting_label_width = measure_columns(counting_label) + 1;
    let mut row = TransientRow::new();
    let mut shown_files = 0;
    let mut parsing: Option<(usize, usize)> = None;
    let mut sampler = RateSampler::start(progress);
    let mut last_number_slot = 0;
    let mut drain_started = started;
    let mut parsed_at_walk_end = 0;
    let mut earned = false;
    while !stop.load(Ordering::Relaxed) {
        thread::park_timeout(TICK);
        let elapsed = started.elapsed().as_millis();
        if elapsed < MOTION_APPEARS_AFTER_MS {
            continue;
        }

        // The count of what has been parsed replaces the count of what was discovered the moment
        // the walk ends; the bar and the rates join it only once the drain has earned them.
        if parsing.is_none() && progress.is_walk_done() {
            let total = progress.get_files_found();
            parsing = Some((total, crate::number_formatter::format_with_separators(total).chars().count()));
            sampler = RateSampler::start(progress);
            drain_started = Instant::now();
            parsed_at_walk_end = progress.get_files_parsed();
        }

        let number_slot = elapsed / NUMBER_REFRESH_MS;
        let numbers_due = number_slot != last_number_slot;
        if numbers_due {
            last_number_slot = number_slot;
        }

        let budget = read_budget();
        let frame = match parsing {
            None => {
                if numbers_due {
                    shown_files = progress.get_files_found();
                }
                // 'files found' is the proxy for git having finished writing
                let label = if progress.get_files_found() == 0 {
                    writing_label.unwrap_or(counting_label)
                } else {
                    counting_label
                };
                format_walk_frame(label, elapsed, shown_files, budget)
            },
            Some((total, count_width)) => {
                if numbers_due {
                    sampler.sample(progress);
                }
                if !earned {
                    let parsed = progress.get_files_parsed();
                    earned = needs_a_bar(total.saturating_sub(parsed), parsed - parsed_at_walk_end,
                            drain_started.elapsed().as_millis());
                }
                let plan = if show_bar_and_rates && earned {
                    fit_parsing_frame(budget, counting_label_width, count_width)
                } else {
                    FramePlan::COUNT_ALONE
                };
                let bare = format_parsing_frame(progress.get_files_parsed(), sampler.shown_parsed, total,
                        count_width, sampler.rates, &plan, charset, calculate_rainbow_phase(elapsed));
                let labeled = format!("{counting_label} {bare}");
                // The label is the next thing given up, ahead of the count it introduces.
                if measure_columns(&labeled) <= budget { labeled } else { bare }
            }
        };
        row.draw(&frame, budget, || {
            open_line_below();
            opened.store(true, Ordering::Relaxed);
        });
    }
}


fn animate_parsing_line(progress: &ScanProgress, stop: &AtomicBool, show_bar_and_rates: bool, charset: &str) {
    let started = Instant::now();
    let total = progress.get_files_found();
    let count_width = crate::number_formatter::format_with_separators(total).chars().count();
    let start_parsed = progress.get_files_parsed();
    let mut row = TransientRow::new();
    let mut sampler = RateSampler::start(progress);
    let mut last_number_slot = 0;
    let mut earned = false;
    while !stop.load(Ordering::Relaxed) {
        thread::park_timeout(TICK);
        let elapsed = started.elapsed().as_millis();
        // Asked again every tick until it is earned: a fast drain never earns a bar, a slow start
        // earns it on the spot.
        if !earned {
            let parsed = progress.get_files_parsed();
            if !needs_a_bar(total.saturating_sub(parsed), parsed - start_parsed, elapsed) {
                continue;
            }
            earned = true;
        }
        let budget = read_budget();
        let number_slot = elapsed / NUMBER_REFRESH_MS;
        if number_slot != last_number_slot {
            last_number_slot = number_slot;
            sampler.sample(progress);
        }
        let plan = if show_bar_and_rates {
            fit_parsing_frame(budget, 0, count_width)
        } else {
            FramePlan::COUNT_ALONE
        };
        // The bar reads the live figure and moves whenever a quantum is crossed, while the count
        // beside it holds still between number refreshes.
        let frame = format_parsing_frame(progress.get_files_parsed(), sampler.shown_parsed, total, count_width,
                sampler.rates, &plan, charset, calculate_rainbow_phase(elapsed));
        row.draw(&frame, budget, || {});
    }
}

fn format_parsing_frame(bar_parsed: usize, counted_parsed: usize, total: usize, count_width: usize,
        rates: Option<(usize, usize)>, plan: &FramePlan, charset: &str, phase: f32) -> String {
    let theme = crate::theme::get_active();
    let count = theme.progress_bar_figures.paint(&format!("{:>count_width$}/{} files",
            crate::number_formatter::format_with_separators(counted_parsed),
            crate::number_formatter::format_with_separators(total))).to_string();
    if plan.bar_cells == 0 {
        return count;
    }
    let speed = match rates {
        Some((files, lines)) if plan.files_rate => {
            let mut figures = format!("  {:>FILES_RATE_FIELD_WIDTH$}{FILES_RATE_SUFFIX}",
                    crate::number_formatter::format_with_separators(files));
            if plan.lines_rate {
                figures += &format!(" | {:>LINES_RATE_FIELD_WIDTH$}{LINES_RATE_SUFFIX}",
                        crate::number_formatter::format_with_separators(lines));
            }
            theme.progress_bar_figures.paint(&figures).to_string()
        },
        _ => String::new()
    };
    // The share follows the count and not the bar, so every figure on the line moves at once: the
    // bar is the one part that answers every tick.
    let share = theme.progress_bar_figures.paint(&format!("{:>PERCENT_FIELD_WIDTH$}%",
            calculate_percentage_done(counted_parsed, total)));
    format!("{}{}{} {share} {count}{speed}", theme.bar_frame.paint("["),
            paint_bar_cells(&build_bar(bar_parsed, total, plan.bar_cells, charset), charset,
                    &theme.progress_bar_fill, &theme.progress_bar_empty, phase),
            theme.bar_frame.paint("]"))
}

// A cell the bar has not reached is blank until the empty token is styled, and then it is a track
// of full cells in the track's own color. Full and not the lightest character of the set: that one
// is also the first sub-step of the tip, so a cell the bar reached would keep the shape it had and
// change only its color.
fn paint_bar_cells(cells: &str, charset: &str, fill: &Style, empty: &Style, phase: f32) -> String {
    let track = charset.chars().last().unwrap_or(' ');
    let width = cells.chars().count();
    cells.chars().enumerate().map(|(at, cell)| {
        let (style, cell) = if cell == ' ' {
            if *empty == Style::plain() {
                return " ".to_owned();
            }
            (empty, track)
        } else {
            (fill, cell)
        };
        match style.get_color_of_cell(at, width, phase) {
            Some(color) => style.paint_with_color(&cell.to_string(), color).to_string(),
            None => style.paint(&cell.to_string()).to_string()
        }
    }).collect()
}

fn calculate_rainbow_phase(elapsed_ms: u128) -> f32 {
    (elapsed_ms % RAINBOW_CYCLE_MS) as f32 / RAINBOW_CYCLE_MS as f32
}

// Rounded down, so that only the last file makes it a hundred.
fn calculate_percentage_done(parsed: usize, total: usize) -> usize {
    (parsed.min(total) * 100).checked_div(total).unwrap_or(0)
}

// 'cells' cells with one quantum per character of the set: the last character is a full cell, the
// ones before it are its sub-steps, so the tip advances through them before a new cell begins
fn build_bar(parsed: usize, total: usize, cells: usize, charset: &str) -> String {
    let levels = charset.chars().collect::<Vec<_>>();
    let filled_quanta = (parsed.min(total) * cells * levels.len()).checked_div(total).unwrap_or(0);
    let full_cells = filled_quanta / levels.len();
    let tip = filled_quanta % levels.len();

    let mut bar = String::with_capacity(cells * 3);
    let mut drawn = 0;
    for _ in 0..full_cells {
        bar.push(*levels.last().unwrap());
        drawn += 1;
    }
    if tip > 0 {
        bar.push(levels[tip - 1]);
        drawn += 1;
    }
    for _ in drawn..cells {
        bar.push(' ');
    }
    bar
}

fn format_walk_frame(heading: &str, elapsed_ms: u128, files_found: usize, budget: usize) -> String {
    let dots = ".".repeat((elapsed_ms / THREE_DOTS_STEP_MS % 4) as usize);
    if files_found == 0 {
        return format!("{heading}{dots}");
    }
    let count = crate::theme::get_active().progress_bar_figures
            .paint(&format!("{:>width$} files discovered",
                    crate::number_formatter::format_with_separators(files_found), width = COUNT_FIELD_WIDTH));
    // The dots are padded to their widest, so the count does not shift as they cycle
    let full = format!("{heading}{dots:<4}{count}");
    // The count clause is this line's one expendable.
    if measure_columns(&full) > budget {
        return format!("{heading}{dots}");
    }
    full
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOMY : usize = 200;

    #[test]
    fn the_dots_cycle_through_none_to_three() {
        assert_eq!("h", format_walk_frame("h", 0, 0, ROOMY));
        assert_eq!("h.", format_walk_frame("h", THREE_DOTS_STEP_MS, 0, ROOMY));
        assert_eq!("h..", format_walk_frame("h", 2 * THREE_DOTS_STEP_MS, 0, ROOMY));
        assert_eq!("h...", format_walk_frame("h", 3 * THREE_DOTS_STEP_MS, 0, ROOMY));
        assert_eq!("h", format_walk_frame("h", 4 * THREE_DOTS_STEP_MS, 0, ROOMY));
    }

    #[test]
    fn the_count_appears_once_files_were_found_and_holds_its_column() {
        assert!(!format_walk_frame("h", 0, 0, ROOMY).contains("files"));
        let one_dot = format_walk_frame("h", 0, 42, ROOMY);
        let three_dots = format_walk_frame("h", 2 * THREE_DOTS_STEP_MS, 42, ROOMY);
        assert!(one_dot.contains("42 files discovered") && three_dots.contains("42 files discovered"));
        assert_eq!(measure_columns(&one_dot), measure_columns(&three_dots));

        let five_digits = format_walk_frame("h", 0, 99_999, ROOMY);
        let six_digits = format_walk_frame("h", 0, 100_000, ROOMY);
        assert_eq!(measure_columns(&five_digits), measure_columns(&six_digits));

        let narrow = format_walk_frame("h", THREE_DOTS_STEP_MS, 42, 10);
        assert_eq!("h.", narrow);
    }

    // Written against what the fit promises and never against precomputed boundaries, so the width
    // constants can move without this test naming their values.
    #[test]
    fn the_frame_plan_steps_down_as_the_budget_narrows() {
        let charset = crate::config_manager::ProgressBarStyle::default().get_charset();
        let plan_at = |budget: usize| fit_parsing_frame(budget, 0, 6);
        let render = |plan: &FramePlan| format_parsing_frame(3_000, 3_000, 80_000, 6,
                Some((29_238, 14_406_917)), plan, charset, 0.0);

        assert_eq!(FramePlan::COUNT_ALONE, plan_at(0));
        assert_eq!(FramePlan { bar_cells: MAX_BAR_CELLS, files_rate: true, lines_rate: true },
                plan_at(10_000));

        let mut previous = plan_at(0);
        for budget in 1..=400 {
            let plan = plan_at(budget);
            assert!(plan.files_rate || !plan.lines_rate, "lines/s without files/s at {budget}");
            assert!(plan.bar_cells == 0 || (MIN_BAR_CELLS..=MAX_BAR_CELLS).contains(&plan.bar_cells),
                    "a bar of {} cells at {budget}", plan.bar_cells);

            assert!(!previous.files_rate || plan.files_rate, "files/s lost at {budget}");
            assert!(!previous.lines_rate || plan.lines_rate, "lines/s lost at {budget}");
            assert!(previous.bar_cells == 0 || plan.bar_cells > 0, "the bar lost at {budget}");

            if (plan.bar_cells > 0 && previous.bar_cells == 0)
                    || (plan.files_rate && !previous.files_rate)
                    || (plan.lines_rate && !previous.lines_rate) {
                assert_eq!(MIN_BAR_CELLS, plan.bar_cells, "something arrived at {budget} without the bar at its minimum");
            }

            if plan.bar_cells > 0 {
                let width = measure_columns(&render(&plan));
                assert!(width <= budget, "a frame of {width} spilled over a budget of {budget}");
                if plan.bar_cells < MAX_BAR_CELLS {
                    let widened = FramePlan { bar_cells: plan.bar_cells + 1,
                            files_rate: plan.files_rate, lines_rate: plan.lines_rate };
                    assert!(measure_columns(&render(&widened)) > budget,
                            "a column was left unspent at {budget}");
                }
            }
            previous = plan;
        }

        // a label shifts the whole ladder by exactly its own width
        for budget in [40, 60, 90, 120] {
            for label_width in [1, 16, 31] {
                assert_eq!(plan_at(budget), fit_parsing_frame(budget + label_width, label_width, 6));
            }
        }

        // the same sweep through the revision line's join, where the label's own separator column
        // can drift between the fit arithmetic and the frame
        let label = "Counting 'feature/live-rende..'";
        let label_width = measure_columns(label) + 1;
        for budget in 0..180 {
            let plan = fit_parsing_frame(budget, label_width, 6);
            if plan.bar_cells == 0 {
                continue;
            }
            let frame = format!("{label} {}", format_parsing_frame(3_000, 3_000, 80_000, 6,
                    Some((29_238, 14_406_917)), &plan,
                    crate::config_manager::ProgressBarStyle::default().get_charset(), 0.0));
            assert!(measure_columns(&frame) <= budget,
                    "a labeled frame of {} spilled over a budget of {budget}", measure_columns(&frame));
        }
    }

    #[test]
    fn the_share_done_rounds_down_and_reaches_a_hundred_only_at_the_end() {
        assert_eq!(0, calculate_percentage_done(0, 80_000));
        assert_eq!(99, calculate_percentage_done(79_999, 80_000));
        assert_eq!(100, calculate_percentage_done(80_000, 80_000));
        // a count that overshot its total, which a walk that found more files can do
        assert_eq!(100, calculate_percentage_done(80_001, 80_000));
        assert_eq!(0, calculate_percentage_done(0, 0));
    }

    #[test]
    fn the_bar_is_earned_by_the_estimated_remaining_time_and_not_by_the_queue() {
        // 1,000 files in 50ms: 5,000 more are a quarter second, 20,000 are a whole one
        assert!(!needs_a_bar(5_000, 1_000, 50));
        assert!(needs_a_bar(20_000, 1_000, 50));
        // an estimate that lands exactly on the threshold does not earn it
        assert!(!needs_a_bar(10_000, 1_000, 50));
        assert!(needs_a_bar(1, 0, 50));
        assert!(!needs_a_bar(0, 0, 1_000));
        assert!(!needs_a_bar(1_000, 0, 0));
    }

    // Written against the cap and not against its value, so the constant can move freely.
    #[test]
    fn a_long_revision_name_is_cut_with_dots_and_a_short_one_kept_whole() {
        assert_eq!("HEAD", cap_revision_name("HEAD"));

        let long = "feature/a-branch-name-that-cannot-possibly-fit-anywhere";
        let cut = cap_revision_name(long);
        assert!(cut.ends_with(".."), "'{cut}'");
        assert!(long.starts_with(cut.trim_end_matches('.')), "'{cut}' is not how '{long}' begins");
        assert_eq!(REVISION_NAME_MAX_COLUMNS, measure_columns(&cut));
        assert_eq!(REVISION_NAME_MAX_COLUMNS, cap_revision_name("0123456789abcdef0123456789abcdef01234567").chars().count());

        // the cut counts terminal columns and not characters: a fullwidth glyph occupies two
        let at_the_cap = "Ａ".repeat(REVISION_NAME_MAX_COLUMNS / 2);
        assert_eq!(at_the_cap, cap_revision_name(&at_the_cap));
        let wide_cut = cap_revision_name(&"Ａ".repeat(REVISION_NAME_MAX_COLUMNS));
        assert!(wide_cut.ends_with(".."), "'{wide_cut}'");
        assert!(measure_columns(&wide_cut) <= REVISION_NAME_MAX_COLUMNS);
        assert_eq!((REVISION_NAME_MAX_COLUMNS - 2) / 2, wide_cut.chars().count() - 2);
    }

    #[test]
    fn the_bar_holds_its_width_fills_monotonically_and_reaches_both_ends() {
        use crate::config_manager::ProgressBarStyle;
        for style in [ProgressBarStyle::Smooth, ProgressBarStyle::Blocky, ProgressBarStyle::Hash] {
            for cells in [MIN_BAR_CELLS, MAX_BAR_CELLS] {
                let charset = style.get_charset();
                let quanta = cells * charset.chars().count();
                assert_eq!(" ".repeat(cells), build_bar(0, quanta, cells, charset));
                assert_eq!(charset.chars().last().unwrap().to_string().repeat(cells), build_bar(quanta, quanta, cells, charset));

                let mut previous_filled = 0;
                for parsed in 0..=quanta {
                    let bar = build_bar(parsed, quanta, cells, charset);
                    assert_eq!(cells, bar.chars().count(), "width moved at {parsed}/{quanta} ({charset})");
                    let filled = bar.chars().filter(|x| *x != ' ').count();
                    assert!(filled >= previous_filled, "the bar retreated at {parsed}/{quanta} ({charset})");
                    previous_filled = filled;
                }

                // one quantum in, the tip is the first sub-step of the set
                assert_eq!(charset.chars().next().unwrap(), build_bar(1, quanta, cells, charset).chars().next().unwrap());
            }
        }
    }

    #[test]
    fn the_parsing_frame_keeps_its_width_as_the_count_grows_and_drops_its_bar_and_rates_on_demand() {
        let charset = crate::config_manager::ProgressBarStyle::default().get_charset();
        let width = crate::number_formatter::format_with_separators(80_000).chars().count();
        let full = FramePlan { bar_cells: MAX_BAR_CELLS, files_rate: true, lines_rate: true };

        let early = format_parsing_frame(5, 5, 80_000, width, Some((29_238, 14_406_917)), &full, charset, 0.0);
        let late = format_parsing_frame(79_999, 79_999, 80_000, width, Some((312, 9_154)), &full, charset, 0.0);
        assert!(early.contains("files/s") && early.contains("lines/s"));
        assert_eq!(measure_columns(&early), measure_columns(&late));

        assert!(early.contains("  0%") && late.contains(" 99%"));
        assert!(format_parsing_frame(80_000, 80_000, 80_000, width, None, &full, charset, 0.0).contains("100%"));

        // before the first sample there is no honest rate, so none is shown
        assert!(!format_parsing_frame(5, 5, 80_000, width, None, &full, charset, 0.0).contains("files/s"));

        let one_rate = FramePlan { bar_cells: MAX_BAR_CELLS, files_rate: true, lines_rate: false };
        let narrower = format_parsing_frame(5, 5, 80_000, width, Some((29_238, 14_406_917)), &one_rate, charset, 0.0);
        assert!(narrower.contains("files/s") && !narrower.contains("lines/s"));

        let reduced = format_parsing_frame(5, 5, 80_000, width, Some((29_238, 14_406_917)), &FramePlan::COUNT_ALONE, charset, 0.0);
        let reduced_late = format_parsing_frame(79_999, 79_999, 80_000, width, None, &FramePlan::COUNT_ALONE, charset, 0.0);
        assert!(!reduced.contains("files/s") && !reduced.contains(charset.chars().last().unwrap()));
        assert!(reduced.contains("files"));
        assert_eq!(measure_columns(&reduced), measure_columns(&reduced_late));
    }

    // A test binary is not a terminal, so the escapes may not be emitted at all: the colors are
    // asserted in 'theme', and what is left here holds either way.
    #[test]
    fn a_bar_is_painted_cell_by_cell_and_only_where_a_token_asks_for_it() {
        let charset = crate::config_manager::ProgressBarStyle::default().get_charset();
        let plain = Style::plain();
        for parsed in [0, 1, 40, 120] {
            let cells = build_bar(parsed, 120, MIN_BAR_CELLS, charset);
            assert_eq!(cells, paint_bar_cells(&cells, charset, &plain, &plain, 0.0));
        }

        let cells = build_bar(40, 120, MIN_BAR_CELLS, charset);
        let gradient = Style::parse("ff0000..0000ff").unwrap();
        let track = Style::parse("333333").unwrap();
        let painted = paint_bar_cells(&cells, charset, &gradient, &track, 0.0);
        assert_eq!(measure_columns(&cells), measure_columns(&painted), "the paint took columns of its own");
        assert!(!painted.contains(' '), "a styled empty cell is still blank instead of a track");
        // the track is drawn with the full cell character, the same one the fill ends on
        assert!(painted.contains(charset.chars().last().unwrap()));
        assert!(paint_bar_cells(&cells, charset, &gradient, &plain, 0.0).contains(' '));
    }
}
