use std::io::{IsTerminal, Write};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use mezura_core::ScanProgress;

use crate::config_manager::Configuration;

const WALK_HEADING : &str = "Analyzing directories";
const MOTION_APPEARS_AFTER_MS : u128 = 150;
// The step constants should be kept as multiples of tick, or a 251 behaves as a 300
const TICK : Duration = Duration::from_millis(50);
const THREE_DOTS_STEP_MS : u128 = 300;
const NUMBER_REFRESH_MS : u128 = 300;
// The count sits right-aligned in this field, so a new digit fills reserved space to its left
// instead of pushing the text beside it. Wide enough that only ten million files overflow it.
const COUNT_FIELD_WIDTH : usize = "9,999,999".len();
// The parsing bar only exists when this many files are still queued when the walk ends: a queue
// the consumers drain in a blink would only flash it
const BAR_APPEARS_OVER_QUEUED : usize = 5_000;
const BAR_CELLS : usize = 30;
const BAR_CHARS : &str = "▏▎▍▌▋▊▉█";   // eighth blocks, 8 quanta per cell
// const BAR_CHARS : &str = "░▒▓█";     // shade steps, 4 quanta
// const BAR_CHARS : &str = "▌█";       // half blocks, 2 quanta
// const BAR_CHARS : &str = ".:#";      // pure ASCII fallback
// Both rates sit right-aligned in reserved fields for the same reason as the walk count: a rate
// crossing a digit boundary between two samples must not push the text beside it
const FILES_RATE_FIELD_WIDTH : usize = "999,999".len();
const LINES_RATE_FIELD_WIDTH : usize = "999,999,999".len();
const ERASE_LINE : &str = "\r\x1b[2K";

const _ : () = {
    assert!(TICK.as_millis() > 0);
    assert!(THREE_DOTS_STEP_MS > 0 && NUMBER_REFRESH_MS > 0);
    assert!(!BAR_CHARS.is_empty());
};

// The moving parts of a run on the terminal, drawn by a differnet thread of this crate.
// Everything transient goes to stderr and is erased before anything permanent prints, and none of it exists
// unless the output is a terminal: a piped run stays byte-identical with a build that had none of
// this. The gate is 'is_terminal' and never CLICOLOR_FORCE, which forces color into pipes and must
// not force motion into them.
pub struct LiveDisplay {
    should_stop: Arc<AtomicBool>,
    animator: Mutex<Option<JoinHandle<()>>>,
    // What the erased line is replaced with: the walk heading settles into its printed form, while
    // 'None' leaves clean ground, which is all the parsing bar leaves behind
    settled_line: Option<String>
}

// Prints the walk heading, animated when both output streams are a terminal and static otherwise.
// Owning both forms is the point: the text exists once, and the piped form is untouched by the
// live one existing.
pub fn start_walk_display(config: &Configuration, progress: Arc<ScanProgress>) -> LiveDisplay {
    if config.view.hidden.directory_info || !config.view.prints_text() {
        return LiveDisplay::default();
    }
    let heading = crate::theme::get_active().heading.paint(WALK_HEADING).to_string();
    if !std::io::stdout().is_terminal() || !std::io::stderr().is_terminal() {
        println!("\n{heading}...");
        return LiveDisplay::default();
    }

    // The line begins on stdout, dotless and unfinished; the animator redraws it in place from
    // stderr, and 'finish' settles it into exactly the line the static branch above prints
    print!("\n{heading}");
    let _ = std::io::stdout().flush();
    let stop = Arc::new(AtomicBool::new(false));
    let animator = {
        let (progress, stop, heading) = (progress, stop.clone(), heading.clone());
        thread::Builder::new().name("live-progress".to_owned())
                .spawn(move || animate_walk_line(&progress, &stop, &heading)).ok()
    };
    if animator.is_none() {
        println!("...");
    }

    LiveDisplay {
        should_stop: stop,
        animator: Mutex::new(animator),
        settled_line: Some(format!("{heading}..."))
    }
}

// The transient line of a parse whose queue is worth watching: the bar, the files done against the
// files found, and the parsing speed. It erases itself completely; nothing permanent is printed.
pub fn start_parsing_display(config: &Configuration, progress: Arc<ScanProgress>) -> LiveDisplay {
    if config.view.hidden.parsing_info || !std::io::stderr().is_terminal() {
        return LiveDisplay::default();
    }
    if progress.get_files_found().saturating_sub(progress.get_files_parsed()) <= BAR_APPEARS_OVER_QUEUED {
        return LiveDisplay::default();
    }

    let show_advanced = !config.view.hidden.parsing_bar;
    let stop = Arc::new(AtomicBool::new(false));
    let animator = {
        let (progress, stop) = (progress, stop.clone());
        thread::Builder::new().name("live-progress".to_owned())
                .spawn(move || animate_parsing_line(&progress, &stop, show_advanced)).ok()
    };

    LiveDisplay {
        should_stop: stop,
        animator: Mutex::new(animator),
        settled_line: None
    }
}

impl LiveDisplay {
    // Idempotent, and called on every path out of the run: the report and the errors both print on
    // ground the animator has left for good.
    pub fn finish(&self) {
        let Some(animator) = self.animator.lock().unwrap().take() else { return };
        self.should_stop.store(true, Ordering::Relaxed);
        let _ = animator.join();
        let parting = match &self.settled_line {
            Some(line) => format!("{ERASE_LINE}{line}\n"),
            None => ERASE_LINE.to_owned()
        };
        let mut stderr = std::io::stderr().lock();
        let _ = stderr.write_all(parting.as_bytes());
        let _ = stderr.flush();
    }
}

impl Default for LiveDisplay {
    fn default() -> Self {
        LiveDisplay {
            should_stop: Arc::new(AtomicBool::new(false)),
            animator: Mutex::new(None),
            settled_line: None
        }
    }
}

fn animate_walk_line(progress: &ScanProgress, stop: &AtomicBool, heading: &str) {
    let started = Instant::now();
    let mut last_written = String::new();
    let mut previous_width = 0;
    let mut shown_files = 0;
    let mut last_number_slot = 0;
    while !stop.load(Ordering::Relaxed) {
        thread::sleep(TICK);
        let elapsed = started.elapsed().as_millis();
        if elapsed < MOTION_APPEARS_AFTER_MS {
            continue;
        }
        let number_slot = elapsed / NUMBER_REFRESH_MS;
        if number_slot != last_number_slot {
            last_number_slot = number_slot;
            shown_files = progress.get_files_found();
        }
        let frame = format_walk_frame(heading, elapsed, shown_files);
        if frame != last_written {
            previous_width = overwrite_transient_line(&frame, previous_width);
            last_written = frame;
        }
    }
}

// Never an erase between frames, and one write per frame: an erase that reaches the screen before
// its rewrite is a blank the eye reads as flicker, and an unbuffered stderr sends every piece of a
// format string as its own write. Overwriting changes each cell once, and the padding covers what
// a shorter frame would leave behind. Returns the width it drew, which is the next frame's padding.
fn overwrite_transient_line(text: &str, previous_width: usize) -> usize {
    let width = crate::theme::calculate_visible_len(text);
    let padding = " ".repeat(previous_width.saturating_sub(width));
    let mut stderr = std::io::stderr().lock();
    let _ = stderr.write_all(format!("\r{text}{padding}").as_bytes());
    let _ = stderr.flush();
    width
}

fn animate_parsing_line(progress: &ScanProgress, stop: &AtomicBool, show_advanced: bool) {
    let started = Instant::now();
    let total = progress.get_files_found();
    let count_width = crate::number_formatter::format_with_separators(total).chars().count();
    let mut last_written = String::new();
    let mut previous_width = 0;
    let mut shown_parsed = progress.get_files_parsed();
    let mut rates = None;
    let mut last_sample = (started, shown_parsed, progress.get_lines_counted());
    let mut last_number_slot = 0;
    while !stop.load(Ordering::Relaxed) {
        thread::sleep(TICK);
        let elapsed = started.elapsed().as_millis();
        if elapsed < MOTION_APPEARS_AFTER_MS {
            continue;
        }
        let number_slot = elapsed / NUMBER_REFRESH_MS;
        if number_slot != last_number_slot {
            last_number_slot = number_slot;
            let (now, parsed, lines) = (Instant::now(), progress.get_files_parsed(), progress.get_lines_counted());
            let seconds = now.duration_since(last_sample.0).as_secs_f64();
            if seconds > 0.0 {
                // The delta between two samples and never the average since the start, so the
                // figure follows what the disk is doing right now
                rates = Some((((parsed - last_sample.1) as f64 / seconds) as usize,
                        ((lines - last_sample.2) as f64 / seconds) as usize));
            }
            last_sample = (now, parsed, lines);
            shown_parsed = parsed;
        }
        // The bar reads the live figure and moves whenever a quantum is crossed, while the count
        // beside it holds still between number refreshes
        let frame = format_parsing_frame(progress.get_files_parsed(), shown_parsed, total, count_width,
                rates, show_advanced);
        if frame != last_written {
            previous_width = overwrite_transient_line(&frame, previous_width);
            last_written = frame;
        }
    }
}

fn format_parsing_frame(bar_parsed: usize, counted_parsed: usize, total: usize, count_width: usize,
        rates: Option<(usize, usize)>, show_advanced: bool) -> String {
    let count = format!("[{:>count_width$}/{}] files",
            crate::number_formatter::format_with_separators(counted_parsed),
            crate::number_formatter::format_with_separators(total));
    if !show_advanced {
        return count;
    }
    let speed = match rates {
        Some((files, lines)) => crate::theme::Style::plain().dim()
                .paint(&format!("  {:>FILES_RATE_FIELD_WIDTH$} files/s | {:>LINES_RATE_FIELD_WIDTH$} lines/s",
                        crate::number_formatter::format_with_separators(files),
                        crate::number_formatter::format_with_separators(lines))).to_string(),
        None => String::new()
    };
    format!("[{}] {count}{speed}", build_bar(bar_parsed, total))
}

// 'BAR_CELLS' cells of 'BAR_CHARS.len()' quanta each: the last character of the set is a full cell,
// the ones before it are its sub-steps, so the tip advances through them before a new cell begins
fn build_bar(parsed: usize, total: usize) -> String {
    let levels = BAR_CHARS.chars().collect::<Vec<_>>();
    let filled_quanta = (parsed.min(total) * BAR_CELLS * levels.len()).checked_div(total).unwrap_or(0);
    let full_cells = filled_quanta / levels.len();
    let tip = filled_quanta % levels.len();

    let mut bar = String::with_capacity(BAR_CELLS * 3);
    let mut cells = 0;
    for _ in 0..full_cells {
        bar.push(*levels.last().unwrap());
        cells += 1;
    }
    if tip > 0 {
        bar.push(levels[tip - 1]);
        cells += 1;
    }
    for _ in cells..BAR_CELLS {
        bar.push(' ');
    }
    bar
}

fn format_walk_frame(heading: &str, elapsed_ms: u128, files_found: usize) -> String {
    let dots = ".".repeat((elapsed_ms / THREE_DOTS_STEP_MS % 3) as usize + 1);
    if files_found == 0 {
        return format!("{heading}{dots}");
    }
    let count = crate::theme::Style::plain().dim()
            .paint(&format!("{:>width$} files discovered",
                    crate::number_formatter::format_with_separators(files_found), width = COUNT_FIELD_WIDTH));
    // The dots are padded to their widest, so the count does not shift as they cycle
    format!("{heading}{dots:<4}{count}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dots_cycle_through_one_to_three() {
        assert_eq!("h.", format_walk_frame("h", 0, 0));
        assert_eq!("h..", format_walk_frame("h", THREE_DOTS_STEP_MS, 0));
        assert_eq!("h...", format_walk_frame("h", 2 * THREE_DOTS_STEP_MS, 0));
        assert_eq!("h.", format_walk_frame("h", 3 * THREE_DOTS_STEP_MS, 0));
    }

    // Nothing found yet reads better as silence than as '0 files', and once the count is there it
    // sits in one column while the dots move beside it
    #[test]
    fn the_count_appears_once_files_were_found_and_holds_its_column() {
        assert!(!format_walk_frame("h", 0, 0).contains("files"));
        let one_dot = format_walk_frame("h", 0, 42);
        let three_dots = format_walk_frame("h", 2 * THREE_DOTS_STEP_MS, 42);
        assert!(one_dot.contains("42 files discovered") && three_dots.contains("42 files discovered"));
        assert_eq!(crate::theme::calculate_visible_len(&one_dot), crate::theme::calculate_visible_len(&three_dots));

        // and a count that grows a digit fills its reserved field instead of pushing the text
        let five_digits = format_walk_frame("h", 0, 99_999);
        let six_digits = format_walk_frame("h", 0, 100_000);
        assert_eq!(crate::theme::calculate_visible_len(&five_digits), crate::theme::calculate_visible_len(&six_digits));
    }

    // Written against the invariants and not the characters, since 'BAR_CHARS' exists to be swapped
    #[test]
    fn the_bar_holds_its_width_fills_monotonically_and_reaches_both_ends() {
        let quanta = BAR_CELLS * BAR_CHARS.chars().count();
        assert_eq!(" ".repeat(BAR_CELLS), build_bar(0, quanta));
        assert_eq!(BAR_CHARS.chars().last().unwrap().to_string().repeat(BAR_CELLS), build_bar(quanta, quanta));

        let mut previous_filled = 0;
        for parsed in 0..=quanta {
            let bar = build_bar(parsed, quanta);
            assert_eq!(BAR_CELLS, bar.chars().count(), "width moved at {parsed}/{quanta}");
            let filled = bar.chars().filter(|x| *x != ' ').count();
            assert!(filled >= previous_filled, "the bar retreated at {parsed}/{quanta}");
            previous_filled = filled;
        }

        // one quantum in, the tip is the first sub-step of the set
        assert_eq!(BAR_CHARS.chars().next().unwrap(), build_bar(1, quanta).chars().next().unwrap());
    }

    #[test]
    fn the_parsing_frame_keeps_its_width_as_the_count_grows_and_hides_its_advanced_part_on_demand() {
        let width = crate::number_formatter::format_with_separators(80_000).chars().count();

        let early = format_parsing_frame(5, 5, 80_000, width, Some((29_238, 14_406_917)), true);
        let late = format_parsing_frame(79_999, 79_999, 80_000, width, Some((312, 9_154)), true);
        assert!(early.contains("files/s") && early.contains("lines/s"));
        assert_eq!(crate::theme::calculate_visible_len(&early), crate::theme::calculate_visible_len(&late));

        // before the first sample there is no honest rate, so none is shown
        assert!(!format_parsing_frame(5, 5, 80_000, width, None, true).contains("files/s"));

        // the reduced form is the count alone: no bar, no rates, and the count still holds its column
        let reduced = format_parsing_frame(5, 5, 80_000, width, Some((29_238, 14_406_917)), false);
        let reduced_late = format_parsing_frame(79_999, 79_999, 80_000, width, None, false);
        assert!(!reduced.contains("files/s") && !reduced.contains(BAR_CHARS.chars().last().unwrap()));
        assert!(reduced.contains("] files"));
        assert_eq!(crate::theme::calculate_visible_len(&reduced), crate::theme::calculate_visible_len(&reduced_late));
    }
}
