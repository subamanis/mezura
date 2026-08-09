#![forbid(unsafe_code)]
#![allow(non_snake_case)]

macro_rules! hashmap {
    ($( $key: expr => $val: expr ),*) => {{
        #[allow(unused_mut)]
        let mut map = ::std::collections::HashMap::new();
        $( map.insert($key, $val); )*
        map
    }}
}

mod animated_display;
mod args;
mod config_files;
mod config_manager;
mod diff;
mod git;
mod json_printer;
mod json_reader;
mod log;
mod message_printer;
mod number_formatter;
mod paths;
mod present;
mod result_printer;
mod sources;
mod suggestions;
mod theme;
mod theme_files;
mod warning_collector;

use std::{collections::HashMap, process::ExitCode, sync::Arc, time::Instant};

use colored::*;
use include_dir::{File, include_dir};
use mezura_core::{EXTENSION_PRIORITY_FILE_NAME, FilesPresent, Language};

use crate::config_manager::{Configuration, OutputFormat};
use crate::config_manager::{CHANGELOG, HELP, LAYOUT, OUTPUT, RESTORE, SHOW_CONFIGS,
        SHOW_LANGUAGES, SHOW_THEMES, THEME_EDITOR, VERSION, VERSION_ID};
use crate::message_printer::Formatted;
use crate::paths::{CONFIG_DIR_NAME, DEFAULT_CONFIG_NAME, LANGUAGES_DIR_NAME, LOGS_DIR_NAME,
        MANIFEST_FILE_NAME, REPLACED_DIR_NAME, THEMES_DIR_NAME};

fn main() -> ExitCode {
    // Dropped last of all: a '--diff' removes its temporary checkouts on background threads, and
    // exiting before they finish would leave a half-deleted tree in the temp directory
    let _removals = crate::animated_display::RemovalsGuard;

    // Started here and not at the scan, so the footer answers for the whole command: a '--diff'
    // pays for checkouts and baseline counting long before any scan of this run begins
    let instant = Instant::now();

    // Windows needs a virtual terminal enabled before it shows colors at all
    #[cfg(target_os = "windows")]
    control::set_virtual_terminal(true).unwrap();

    let languages_available: Vec<Language>;

    // Before the languages are read, or the run that performs it would still count with the old
    // files and the change would appear to take two runs to arrive
    match migrate_data_files(&crate::paths::PERSISTENT_APP_PATHS.data_dir, false) {
        Ok(outcome) => if let Some(message) = outcome.format() {eprintln!("{message}")},
        // Whatever was written stays on disk, and the version is recorded only after a pass that
        // finished, so the next execution tries again instead of believing it is done
        Err(x) => eprintln!("{}",format!("\nUnable to update the data files: {x}\n").yellow())
    }

    if !crate::paths::PERSISTENT_APP_PATHS.are_initialized {
        // A first execution reads the baked-in copies, since the paths were resolved and the
        // directory judged before the migration above created anything. Same contents either way.
        languages_available = mezura_core::languages::parse_shipped_languages();
    } else {
        match mezura_core::language_file::parse_languages_in_dir(&crate::paths::PERSISTENT_APP_PATHS.languages_dir) {
            Ok((parsed, faulty_files)) => {
                if !faulty_files.is_empty() {
                    eprintln!("{}", crate::message_printer::format_faulty_language_files_message(&faulty_files).yellow());
                    // One warning per file and not one for the list, since each is a whole language
                    // whose files went uncounted and a reader of the document wants to know which
                    for faulty in &faulty_files {
                        let (file, reason) = (&faulty.file_name, &faulty.error);
                        crate::warning_collector::keep(mezura_core::warnings::Warning::new(mezura_core::warnings::Code::LanguageFileUnreadable, file,
                                format!("'{file}' could not be used as a language file, so the files of that language were not counted: {reason}.")));
                    }
                }

                languages_available = parsed;
            },
            Err(x) => {
                eprintln!("\n{}", x.format());
                return ExitCode::FAILURE;
            }
        }
    }

    let args_str = match read_args_as_str() {
        Some(args) => {
            args
        },
        None => {
            String::from("./")
        }
    };

    if let Some(code) = handle_message_only_command(&args_str, &languages_available) {
        return code;
    }

    let mut config = match config_manager::create_config_from_args(&args_str) {
        Ok(config) => config,
        Err(x) => {
            eprintln!("\n{}\n",x.format());
            return ExitCode::FAILURE;
        }
    };
    crate::theme::set_active(config.view.theme.clone());
    crate::number_formatter::set_number_separator(config.view.number_separator);
    crate::number_formatter::set_decimal_separator(config.view.decimal_separator);
    // A terminal that names itself dumb (an Emacs shell buffer, some CI shells) is a tty with no
    // cursor addressing, so nothing moving can be drawn on it; the 'dumb-emacs-ansi' form does
    // understand colors and keeps them, the bare 'dumb' cannot show those either. The removals
    // guard above was built before any of this existed, so it reads the answer from where the
    // setter leaves it rather than from the configuration.
    let terminal_kind = std::env::var("TERM").unwrap_or_default();
    crate::animated_display::set_animations_hidden(
            config.view.hidden.animations || terminal_kind.starts_with("dumb"));
    if terminal_kind == "dumb" {
        control::set_override(false);
    }

    // A pipe already strips the escape codes, but CLICOLOR_FORCE overrides that and would put them
    // inside the strings of the document, so the machine format turns them off itself
    if !config.view.prints_text() {
        control::set_override(false);
    }

    // Printed here and not at the very start, so that '--hide version' can be declared in a
    // configuration file and not only on the command line
    if !config.view.hidden.version && config.view.prints_text() {
        // The status block opens with a blank line of its own, so the separation below the
        // version is only missing when that block is not printed
        let separator = if config.view.hidden.directory_info {"\n"} else {""};
        println!("\n{}{separator}", crate::theme::get_active().version.paint(VERSION_ID));
    }

    if !config.engine.languages_of_interest.is_empty() &&
     config.engine.languages_of_interest.iter().all(|lang| config.engine.excluded_languages.contains(lang)) {
        eprintln!("\n{}\n",crate::theme::get_active().error.paint("Included and excluded languages are mutually exclusive."));
        return ExitCode::FAILURE;
    }

    if !config.engine.languages_of_interest.is_empty() {
        match report_unknown_languages(&languages_available, &config.engine.languages_of_interest) {
            Ok(x) => {
                if let Some(msg) = x {
                    eprintln!("\n {msg}");
                }
            },
            Err(x) => {
                eprintln!("\n{x}\n");
                return ExitCode::FAILURE;
            }
        }
    }

    let (extension_priority, faulty_priority_lines) = read_extension_priority();
    if !faulty_priority_lines.is_empty() {
        eprintln!("{}", format!("\nLines that could not be read in '{EXTENSION_PRIORITY_FILE_NAME}', and were skipped:\n{}",
                faulty_priority_lines.join("\n")).yellow());
        for line in &faulty_priority_lines {
            crate::warning_collector::keep(mezura_core::warnings::Warning::new(mezura_core::warnings::Code::PriorityLineSkipped, line,
                    format!("'{line}' could not be read in '{EXTENSION_PRIORITY_FILE_NAME}' and was skipped, so any contest it was meant to settle was left to the tiebreak.")));
        }
    }

    // Above the language resolution, whose selection a document's adopted settings can change, and
    // below the extension priority, which a side that is a revision is counted with
    let baseline_only = match crate::diff::DiffRequest::of(&mut config, &languages_available) {
        Ok(Some(crate::diff::DiffRequest::BetweenTwoReadings(both))) => {
            return match both.into_comparison(&config, &extension_priority) {
                Ok(comparison) => {
                    if config.view.prints_text() {
                        println!();
                        crate::result_printer::print_comparison(&comparison, &config);
                        // The exec time alone, without the parsing figures: those describe one
                        // scan, and a comparison had up to two
                        if !config.view.hidden.timing {
                            println!("\n{}", crate::theme::get_active().footer.paint(&format_exec_time(&instant)));
                        }
                    } else {
                        crate::json_printer::print_comparison_as_json(&comparison, &chrono::Local::now(), &config);
                    }
                    ExitCode::SUCCESS
                },
                Err(complaint) => {
                    eprintln!("\n{}\n", crate::theme::get_active().error.paint(&crate::message_printer::wrap_message(&complaint)));
                    ExitCode::FAILURE
                }
            };
        },
        Ok(Some(crate::diff::DiffRequest::AgainstThisRun(baseline_only))) => Some(baseline_only),
        Ok(None) => None,
        Err(complaint) => {
            eprintln!("\n{}\n", crate::theme::get_active().error.paint(&crate::message_printer::wrap_message(&complaint)));
            return ExitCode::FAILURE;
        }
    };

    let (languages, reported) = mezura_core::Languages::resolve(&config.engine, languages_available, &extension_priority);
    crate::warning_collector::report_language_resolution_warnings(reported);

    // Above the run, so that a baseline which turns out not to be one costs no scan of the tree
    let counted_baseline = match baseline_only.map(|x| x.count_baseline(&config, &extension_priority)) {
        Some(Ok(x)) => Some(x),
        Some(Err(complaint)) => {
            eprintln!("\n{}\n", crate::theme::get_active().error.paint(&crate::message_printer::wrap_message(&complaint)));
            return ExitCode::FAILURE;
        },
        None => None
    };

    let progress = Arc::new(mezura_core::ScanProgress::default());
    let live = crate::animated_display::start_walk_display(&config, progress.clone());

    let mut parsing_live = None;
    let outcome = mezura_core::run(&config.engine, languages, Some(progress.clone()), |scan| {
        live.finish();
        announce_traversal(&config, scan);
        parsing_live = Some(crate::animated_display::start_parsing_display(&config, progress));
    });
    if let Some(x) = &parsing_live {
        x.finish();
    }
    live.finish();
    match outcome {
        Ok(result) => {
            let comparison = counted_baseline.map(|baseline| baseline.with_subject(
                    crate::diff::Reading::of_this_run(&result, &chrono::Local::now(), &config.engine), &config));
            crate::present::present(&result, comparison.as_ref(), &config);
            // Already presented above as the failures they are, and the exit code keeps its meaning:
            // 1 is a run that did not happen. Every file unparseable, or every place unopenable.
            if result.all_relevant_files_were_faulty() || result.nothing_could_be_read() {
                return ExitCode::FAILURE;
            }
            // The document has its own 'scan_ms' measured inside the run; this is the only place
            // that knows what the whole command took. A run that found nothing prints no timing.
            if !config.view.hidden.timing && config.view.prints_text() && result.files_present.relevant_files > 0 {
                let perf = format_exec_time(&instant);
                // Worked out here and not carried on the result: these are arithmetic on the
                // duration and the counts, and the one second rule is a decision about the report.
                let millis = result.performance.duration_millis;
                let metrics = if millis > 1000 {
                    let seconds = millis as f32 / 1000f32;
                    format!("(Parsing {} files/s | {} lines/s)",
                            crate::number_formatter::format_with_separators((result.files_present.relevant_files as f32 / seconds) as usize),
                            crate::number_formatter::format_with_separators((result.total.lines as f32 / seconds) as usize))
                } else {
                    String::new()
                };
                println!("\n{}",crate::theme::get_active().footer.paint(&(perf + &metrics)));
            }
            ExitCode::SUCCESS
        },
        // Mistakes in the configuration surface here, because the run is what resolves the
        // declared targets: the wording is this crate's own, and a configuration file that
        // supplied the dirs is named as the culprit the reader cannot see failing
        Err(mezura_core::RunError::InvalidTargets(inner)) => {
            eprintln!("{}", crate::config_manager::attribute_dirs_error(inner, &config.view.dirs_source).format());
            ExitCode::FAILURE
        },
        Err(x) => {
            eprintln!("{}",x.format());
            ExitCode::FAILURE
        }
    }
}

fn format_exec_time(instant: &Instant) -> String {
    format!("Exec time: {} secs ", crate::number_formatter::format_with_decimal_separator(format!("{:.2}", instant.elapsed().as_secs_f32())))
}

// The two lines between the phases. They cannot be printed around the call, because the scanning and
// the counting overlap and these figures are known part way through.
fn announce_traversal(config: &Configuration, scan: FilesPresent) {
    if scan.relevant_files == 0 {
        return;
    }
    if !config.view.hidden.directory_info && config.view.prints_text() {
        println!("{}\n",crate::theme::get_active().summary.paint(&format!("{} files found. {} of interest. {} excluded.",
                crate::number_formatter::format_with_separators(scan.total_files), crate::number_formatter::format_with_separators(scan.relevant_files),
                crate::number_formatter::format_with_separators(scan.excluded_files))));
    }
    if !config.view.hidden.parsing_info && config.view.prints_text() {
        println!("{}...",crate::theme::get_active().heading.paint("Parsing files"));
    }
}

// One at a time, each with the names it is closest to: one line for the whole list leaves nothing to
// attach a suggestion to. The filtering itself belongs to the run, so a caller that is not this
// binary gets the same selection; what is left here is the color and the correction.
fn report_unknown_languages(languages_available: &[Language], languages_of_interest: &[String])
        -> Result<Option<String>, String>
{
    let unknown = mezura_core::languages::find_unknown_language_names(languages_available, languages_of_interest);

    // Deduplicated for the same reason the list of supported languages is: an installation holding
    // two files that declare one name is one language however many files describe it, and offering
    // the same correction twice reads as two different things to try.
    let mut all_names = languages_available.iter().map(|x| x.name.clone()).collect::<Vec<_>>();
    all_names.sort_by_key(|x| x.to_lowercase());
    all_names.dedup();
    let candidates = all_names.iter().map(String::as_str).collect::<Vec<_>>();

    // Only the mistake is colored. What to do about it is not an error, it is the way out.
    let mut report = String::with_capacity(100);
    for name in &unknown {
        report.push_str(&format!("\n{}", crate::theme::get_active().warning.paint(&format!("'{name}' does not exist as a language file."))));
        if let Some(x) = crate::suggestions::formatted_suggestion(name, &candidates) {
            report.push_str(&format!("\n{x}\n"));
        }
    }

    if unknown.len() == languages_of_interest.len() {
        let headline = crate::theme::get_active().error.paint("None of the provided language names map to valid supported languages.");
        return Err(format!("{headline}\n{report}"));
    }

    Ok(if report.is_empty() {None} else {Some(report)})
}

// Three bytes that mean "this is UTF-8" and carry no text. 'trim' leaves them where they are, since
// they are not whitespace, so a header written on the first line of a file stops matching the moment
// somebody re-saves that file with PowerShell's 'Set-Content' or an older Notepad, which is the
// ordinary way of editing one of these on Windows. Every parser of a text format here strips it on
// the way in, and the library does the same for the files it owns: leaving it to each parser is how
// two of the four came to be missing it, and one of those failed by quietly reading no rules at all.
fn strip_byte_order_mark(contents: &str) -> &str {
    contents.trim_start_matches('\u{feff}')
}

fn read_baked_in_default_config_contents() -> String {
    String::from_utf8_lossy(include_bytes!("../data/config/default.txt")).to_string()
}

fn read_baked_in_extension_priority_contents() -> String {
    String::from_utf8_lossy(mezura_core::languages::get_shipped_extension_priority_raw()).to_string()
}

// An installation made by an earlier version has no such file, and the baked-in copy is not used as
// a substitute: the user is meant to edit the one on disk, and reading a different one would make
// their edits look like they had no effect. It is written by the same restore that writes everything
// else, and until it is there every contested extension simply announces its tiebreak.
fn read_extension_priority() -> (HashMap<String,Vec<String>>, Vec<String>) {
    mezura_core::language_file::parse_priority_file(&(crate::paths::PERSISTENT_APP_PATHS.data_dir.clone() + EXTENSION_PRIORITY_FILE_NAME))
}

fn open_in_browser(path: &str) {
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("cmd").args(["/C", "start", "", path]).spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(path).spawn();

    if result.is_err() {
        println!("(the page could not be opened in a browser automatically)");
    }
}

#[derive(Default)]
struct MigrationOutcome {
    installed: Vec<String>,
    replaced: Vec<String>,
    withdrawn: Vec<String>
}

impl MigrationOutcome {
    // Silence is the ordinary outcome. Only a file of the user's that was moved aside is worth a
    // line, because it is the only part of this that asks something of them.
    fn format(&self) -> Option<String> {
        if self.replaced.is_empty() {
            return None;
        }

        let (count, plural) = (self.replaced.len(), if self.replaced.len() == 1 {"file"} else {"files"});
        Some(format!("\nUpdated the data files for {VERSION_ID}.\n{count} {plural} you had changed {} replaced. \
Yours can be found in '{}{REPLACED_DIR_NAME}/{VERSION_ID}/', if you want to carry your changes over:\n  {}\n",
                if count == 1 {"was"} else {"were"}, crate::paths::PERSISTENT_APP_PATHS.data_dir, self.replaced.join(", ")).yellow().to_string())
    }
}

// FNV-1a over the content with every '\r' dropped. Detecting a change is all it has to do, and the
// carriage returns have to go: the shipped files are written with them, and an editor that saves a
// file back with unix endings would otherwise make every one of them look edited, which would fill
// the archive with copies of what we shipped and put a message in front of the user at every single
// release.
fn content_hash(bytes: &[u8]) -> u64 {
    let mut hash : u64 = 0xcbf29ce484222325;
    for byte in bytes.iter().filter(|x| **x != b'\r') {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }

    hash
}

// The version that wrote it, then one 'path hash' line per file. A missing or unreadable manifest is
// a fresh installation as far as this is concerned, which is what makes it self-healing.
fn read_manifest(data_dir: &str) -> (String, HashMap<String, u64>) {
    let mut entries = HashMap::new();
    let Ok(contents) = std::fs::read_to_string(data_dir.to_owned() + MANIFEST_FILE_NAME) else {
        return (String::new(), entries);
    };

    let mut lines = contents.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#'));
    let version = lines.next().unwrap_or_default().to_owned();
    for line in lines {
        // From the right, because a file name is allowed to hold spaces and a hash is not
        if let Some((path, hash)) = line.rsplit_once(' ') && let Ok(hash) = hash.trim().parse::<u64>() {
            entries.insert(path.trim().to_owned(), hash);
        }
    }

    (version, entries)
}

fn write_manifest(data_dir: &str, entries: &HashMap<String, u64>) -> Result<(), std::io::Error> {
    let mut sorted = entries.iter().collect::<Vec<_>>();
    sorted.sort();
    let body = sorted.into_iter().map(|(path, hash)| format!("{path} {hash}")).collect::<Vec<_>>().join("\n");

    std::fs::write(data_dir.to_owned() + MANIFEST_FILE_NAME,
            format!("# Written by mezura. It records which files it installed and what they looked like,\n\
# so that an update can tell a file you edited from one it wrote itself. Deleting it is\n\
# harmless: the next run treats the installation as a new one.\n{VERSION_ID}\n{body}\n"))
}

fn named(dir_name: &str, file: &File<'static>) -> (String, &'static [u8]) {
    let name = std::path::Path::new(file.path).file_name().and_then(|x| x.to_str()).unwrap_or(file.path);
    (dir_name.to_owned() + "/" + name, file.contents)
}

fn shipped_files() -> Vec<(String, &'static [u8])> {
    mezura_core::languages::get_shipped_language_files_raw().into_iter()
            .map(|(name, contents)| (LANGUAGES_DIR_NAME.to_owned() + "/" + name, contents)).collect()
}

// Whether the two say the same thing, which is not the same question as whether they read the same.
// A different indentation, a blank line or a re-saved line ending changes every byte and no meaning,
// and treating that as an edit would move a file aside for nothing and say so out loud.
// This is also what keeps the replacing honest: the only differences that survive it are differences
// that change a count, which is the only reason to take somebody's file away from them.
fn means_the_same(on_disk: &[u8], shipped: &[u8]) -> bool {
    let (theirs, ours) = (String::from_utf8_lossy(on_disk), String::from_utf8_lossy(shipped));
    match (mezura_core::language_file::parse_language(&theirs), mezura_core::language_file::parse_language(&ours)) {
        (Some(theirs), Some(ours)) => theirs == ours,
        // Ours always parses, so this is a file edited into something that no longer does, and
        // replacing it is a repair
        _ => false
    }
}

fn archive(data_dir: &str, relative: &str, contents: &[u8]) -> Result<(), std::io::Error> {
    let target = format!("{data_dir}{REPLACED_DIR_NAME}/{VERSION_ID}/{relative}");
    if let Some(parent) = std::path::Path::new(&target).parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Never over one that is already there: two runs can start at the same moment, and a build under
    // development is run many times under the one version
    if !std::path::Path::new(&target).exists() {
        std::fs::write(&target, contents)?;
    }

    Ok(())
}

// Brings the data directory to what this version ships. The shipped copy always wins and the user's
// is kept, so that every installation runs with the language files we last corrected, and nothing of
// theirs is destroyed. A file we never wrote is never touched, which is what makes a language of
// their own safe. 'force' is '--restore': do it again even though the version says there is nothing
// to do.
fn migrate_data_files(data_dir: &str, force: bool) -> Result<MigrationOutcome, std::io::Error> {
    let (recorded_version, recorded) = read_manifest(data_dir);
    let mut outcome = MigrationOutcome::default();
    if !force && recorded_version == VERSION_ID {
        return Ok(outcome);
    }

    for name in [LANGUAGES_DIR_NAME, THEMES_DIR_NAME, CONFIG_DIR_NAME, LOGS_DIR_NAME] {
        // The logs directory holds nothing that ships, but without it a run with '--log' has nowhere to write
        std::fs::create_dir_all(data_dir.to_owned() + name)?;
    }

    let mut manifest = HashMap::new();
    for (relative, contents) in shipped_files() {
        let target = data_dir.to_owned() + &relative;
        let shipped_hash = content_hash(contents);
        manifest.insert(relative.clone(), shipped_hash);

        let Ok(on_disk) = std::fs::read(&target) else {
            std::fs::write(&target, contents)?;
            outcome.installed.push(relative);
            continue;
        };

        let on_disk_hash = content_hash(&on_disk);
        if on_disk_hash == shipped_hash {
            continue;
        }
        if recorded.get(&relative) == Some(&on_disk_hash) || means_the_same(&on_disk, contents) {
            std::fs::write(&target, contents)?;
            continue;
        }

        archive(data_dir, &relative, &on_disk)?;
        std::fs::write(&target, contents)?;
        outcome.replaced.push(relative);
    }

    // What we used to ship and no longer do. Recognised only because the manifest remembers writing
    // it, so a file of the user's own is never mistaken for one of ours that was withdrawn.
    // Weighed against everything we ship and not against what this pass manages, because a file that
    // moved from the one set to the other, as the themes did, is still shipped and deleting it would
    // be the opposite of what that move was for.
    let still_shipped = shipped_files().into_iter().map(|(relative, _)| relative)
            .chain(include_dir!("data/themes").files.iter().map(|file| named(THEMES_DIR_NAME, file).0))
            .chain([format!("{CONFIG_DIR_NAME}/{DEFAULT_CONFIG_NAME}"), EXTENSION_PRIORITY_FILE_NAME.to_owned()])
            .collect::<std::collections::HashSet<_>>();
    for relative in recorded.keys().filter(|relative| !still_shipped.contains(*relative)) {
        let target = data_dir.to_owned() + relative;
        if let Ok(on_disk) = std::fs::read(&target) {
            archive(data_dir, relative, &on_disk)?;
            std::fs::remove_file(&target)?;
            outcome.withdrawn.push(relative.clone());
        }
    }

    // Written when they are absent and never touched again, and deliberately left out of the
    // manifest so that nothing can reach them later either. All three exist in order to be edited:
    // the default settings, the answer given to a contested extension, which replacing would make
    // somebody give again at every release, and the themes, which are taste. A theme that has fallen
    // behind what we ship breaks nothing, since a token it does not name falls back to a default, so
    // there is no correctness to weigh against somebody's own colors.
    for (relative, contents) in include_dir!("data/themes").files.iter().map(|file| named(THEMES_DIR_NAME, file)) {
        let target = data_dir.to_owned() + &relative;
        if !std::path::Path::new(&target).exists() {
            std::fs::write(&target, contents)?;
            outcome.installed.push(relative);
        }
    }

    let default_config = format!("{data_dir}{CONFIG_DIR_NAME}/{DEFAULT_CONFIG_NAME}");
    if !std::path::Path::new(&default_config).exists() {
        std::fs::write(&default_config, read_baked_in_default_config_contents())?;
        outcome.installed.push(DEFAULT_CONFIG_NAME.to_owned());
    }
    let priority_path = data_dir.to_owned() + EXTENSION_PRIORITY_FILE_NAME;
    if !std::path::Path::new(&priority_path).exists() {
        std::fs::write(&priority_path, read_baked_in_extension_priority_contents())?;
        outcome.installed.push(EXTENSION_PRIORITY_FILE_NAME.to_owned());
    }

    // Last, so that a pass that died halfway leaves the old version recorded and the next run tries
    // again, instead of a half-written directory that claims to be current
    write_manifest(data_dir, &manifest)?;
    Ok(outcome)
}

fn read_args_as_str() -> Option<String> {
    let args = std::env::args().skip(1)
            .filter_map(|arg| crate::args::get_trimmed_if_not_empty(&arg))
            .collect::<Vec<String>>();
    if args.is_empty() {
        None
    } else {
        Some(args.join(" ").trim().to_owned())
    }
}

// These commands take no configuration, so there is nothing that could hide the version line
// from them, and they are the only place where the version of an installed binary can be read.
// 'None' means the args are not one of them, and not that nothing went wrong: three of the branches
// below report a mistake, and a bool could not tell the caller to stop with a failure instead of
// handing arguments it has already rejected to the configuration parser.
fn handle_message_only_command(args_str: &str, languages_available: &[Language]) -> Option<ExitCode> {
    let is_present = |command: &str| crate::args::find_command(args_str, command).is_some();
    let message_command = [HELP, VERSION, CHANGELOG, SHOW_LANGUAGES, SHOW_CONFIGS, SHOW_THEMES,
            THEME_EDITOR, RESTORE].into_iter().find(|x| is_present(x))?;

    // Refused rather than answered, and before the banner below, which would otherwise be the first
    // thing written. The redirection that makes '--output json' worth asking for is what makes this
    // dangerous: 'mezura --output json --help > stats.json' leaves a file named for a document that
    // holds a help text, and nothing says it is not one until something tries to parse it. The two
    // can only have been typed together, since a configuration file may not declare '--output'.
    if asks_for_a_json_document(args_str) {
        eprintln!("\n{}\n", crate::theme::get_active().error.paint(&format!(
                "'--{message_command}' prints a message to read and '--output json' writes a document for a \
program to read, and both of them go to the output, so only one of the two can be asked for at a time.")));
        return Some(ExitCode::FAILURE);
    }

    // '--version' prints the line itself, with the release date next to it, so it is answered before
    // the plain banner that every other message-only command opens with. With '--help' next to it,
    // the question is about the command and belongs to the help.
    if is_present(VERSION) && !is_present(HELP) {
        crate::message_printer::print_version();
        return Some(ExitCode::SUCCESS);
    }
    println!("\n{}", crate::theme::get_active().version.paint(VERSION_ID));

    if is_present(HELP) {
        crate::message_printer::print_help_message_for_given_args(args_str);
        return Some(ExitCode::SUCCESS);
    } else if let Some(pos) = crate::args::find_command(args_str, CHANGELOG) {
        return match args_str[pos + CHANGELOG.len() + 2..].split_whitespace().next() {
            Some("full") => {
                crate::message_printer::print_changelog(true);
                Some(ExitCode::SUCCESS)
            },
            Some(arg) if !arg.starts_with("--") => {
                println!("\n{}", config_manager::ArgParsingError::IncorrectCommandArgs(CHANGELOG.to_owned()).format());
                crate::message_printer::print_help_message_for_command(CHANGELOG);
                Some(ExitCode::FAILURE)
            },
            _ => {
                crate::message_printer::print_changelog(false);
                Some(ExitCode::SUCCESS)
            },
        };
    } else if is_present(SHOW_LANGUAGES) {
        crate::message_printer::print_supported_languages(languages_available);
        return Some(ExitCode::SUCCESS);
    } else if is_present(SHOW_CONFIGS) {
        crate::message_printer::print_existing_configs();
        return Some(ExitCode::SUCCESS);
    } else if is_present(RESTORE) {
        // The same pass a version change performs, asked for by hand: useful when something was
        // damaged inside one version, where nothing would otherwise trigger it
        return match migrate_data_files(&crate::paths::PERSISTENT_APP_PATHS.data_dir, true) {
            Ok(outcome) => {
                if outcome.installed.is_empty() && outcome.replaced.is_empty() && outcome.withdrawn.is_empty() {
                    println!("\nEverything that ships with mezura is in place.");
                }
                if !outcome.installed.is_empty() {
                    let plural = if outcome.installed.len() == 1 {"file"} else {"files"};
                    println!("\nRestored {} {plural}:\n{}", outcome.installed.len(), outcome.installed.join(", "));
                }
                if let Some(message) = outcome.format() {
                    println!("{message}");
                }
                if !outcome.withdrawn.is_empty() {
                    println!("\nNo longer part of mezura, and moved to '{}{REPLACED_DIR_NAME}/{VERSION_ID}/':\n{}",
                            crate::paths::PERSISTENT_APP_PATHS.data_dir, outcome.withdrawn.join(", "));
                }
                Some(ExitCode::SUCCESS)
            },
            Err(x) => {
                println!("\n{}", format!("Unable to restore the files: {x}").red());
                Some(ExitCode::FAILURE)
            }
        };
    } else if is_present(THEME_EDITOR) {
        return match crate::theme_files::generate_theme_editor_page() {
            Ok(path) => {
                println!("\nTheme editor page generated at:\n{path}");
                open_in_browser(&path);
                Some(ExitCode::SUCCESS)
            },
            Err(x) => {
                println!("\n{}", format!("Unable to generate the theme editor page: {x}").red());
                Some(ExitCode::FAILURE)
            }
        };
    } else if let Some(pos) = crate::args::find_command(args_str, SHOW_THEMES) {
        // The preview follows '--layout', so that what it shows is what a run would print. Read here
        // by hand, because a message-only command runs before there is a configuration to ask.
        let layout = crate::args::find_command(args_str, LAYOUT)
                .and_then(|at| args_str[at + LAYOUT.len() + 2..].split_whitespace().next())
                .and_then(config_manager::Layout::parse)
                .unwrap_or_default();

        return match args_str[pos + SHOW_THEMES.len() + 2..].split_whitespace().next() {
            Some(arg) if !arg.starts_with("--") => match config_manager::BarThickness::parse(arg) {
                Some(thickness) => {
                    crate::message_printer::print_existing_themes(thickness, layout);
                    Some(ExitCode::SUCCESS)
                },
                None => {
                    println!("\n{}", config_manager::ArgParsingError::IncorrectCommandArgs(SHOW_THEMES.to_owned()).format());
                    crate::message_printer::print_help_message_for_command(SHOW_THEMES);
                    Some(ExitCode::FAILURE)
                }
            },
            _ => {
                crate::message_printer::print_existing_themes(config_manager::BarThickness::default(), layout);
                Some(ExitCode::SUCCESS)
            }
        };
    }

    None
}

// Read off the arguments by hand, the way '--show-themes' reads '--layout' above: a message-only
// command runs before there is a configuration to ask.
fn asks_for_a_json_document(args_str: &str) -> bool {
    crate::args::find_command(args_str, OUTPUT)
            .and_then(|at| args_str[at + OUTPUT.len() + 2..].split_whitespace().next())
            .and_then(OutputFormat::parse) == Some(OutputFormat::Json)
}

#[cfg(test)]
mod tests {
    use mezura_core::Language;
    use crate::paths::test_paths::SCRATCH_DIR;

    use crate::config_manager::VERSION_ID;

    use crate::{content_hash, migrate_data_files, report_unknown_languages};

    // The shipped copy always wins and the user's is kept, which is the whole of the policy. What
    // this pins is the three ways a file can differ from what we ship, because only one of them is
    // supposed to reach the user as a message.
    #[test]
    fn a_migration_replaces_what_was_changed_and_keeps_it_and_is_silent_about_the_rest() {
        let dir = SCRATCH_DIR.to_owned() + "migration-test/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let first = migrate_data_files(&dir, false).unwrap();
        assert!(!first.installed.is_empty() && first.replaced.is_empty() && first.format().is_none());
        assert!(std::path::Path::new(&(dir.clone() + "installed.txt")).exists());

        // Nothing changed since, so the version alone stops it
        assert!(migrate_data_files(&dir, false).unwrap().installed.is_empty());

        let lua = dir.clone() + "languages/Lua.txt";
        let shipped = std::fs::read_to_string(&lua).unwrap();

        // Said differently and meaning the same: corrected without a word, since telling somebody
        // their file was replaced when nothing about it counted differently is noise
        std::fs::write(&lua, shipped.replace("\" '", "\"     '")).unwrap();
        let cosmetic = migrate_data_files(&dir, true).unwrap();
        assert!(cosmetic.replaced.is_empty(), "a difference that changes no count was reported");
        assert_eq!(shipped, std::fs::read_to_string(&lua).unwrap());

        // A symbol removed is a different language, so their copy is kept and named
        std::fs::write(&lua, shipped.replace("\" '", "\"")).unwrap();
        let edited = migrate_data_files(&dir, true).unwrap();
        assert_eq!(vec!["languages/Lua.txt".to_owned()], edited.replaced);
        assert_eq!(shipped, std::fs::read_to_string(&lua).unwrap());
        assert!(std::fs::read_to_string(format!("{dir}replaced/{}/languages/Lua.txt", crate::config_manager::VERSION_ID))
                .unwrap().contains("\""));

        // A theme is taste, and one that has fallen behind what we ship breaks nothing, so somebody
        // who expanded the one we ship keeps what they wrote. This is the case that decided the
        // split: a language file that has fallen behind gives wrong numbers, a theme gives colors.
        let theme = dir.clone() + "themes/Dracula.txt";
        let mine = std::fs::read_to_string(&theme).unwrap() + "\nheading = #ff0000";
        std::fs::write(&theme, &mine).unwrap();
        assert!(migrate_data_files(&dir, true).unwrap().replaced.is_empty());
        assert_eq!(mine, std::fs::read_to_string(&theme).unwrap());

        // A file of their own is never ours to touch, whatever happens around it
        let theirs = dir.clone() + "languages/Mine.txt";
        std::fs::write(&theirs, "not a language file at all").unwrap();
        // and one that was deleted comes back
        std::fs::remove_file(dir.clone() + "languages/Zig.txt").unwrap();

        let third = migrate_data_files(&dir, true).unwrap();
        assert_eq!(vec!["languages/Zig.txt".to_owned()], third.installed);
        assert!(third.replaced.is_empty());
        assert_eq!("not a language file at all", std::fs::read_to_string(&theirs).unwrap());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // The worst thing this mechanism could do is reset somebody's settings, and these are the two
    // files it must never touch after creating them: one holds their preferences and the other the
    // answer they gave to a contested extension, which they would have to give again every release.
    #[test]
    fn the_two_files_that_exist_in_order_to_be_edited_are_never_replaced() {
        let dir = SCRATCH_DIR.to_owned() + "migration-carve-out/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        migrate_data_files(&dir, false).unwrap();

        let config = dir.clone() + "config/default.txt";
        let priority = dir.clone() + "extension_priority.txt";
        std::fs::write(&config, "settings of my own").unwrap();
        std::fs::write(&priority, "m  MATLAB, Objective-C").unwrap();

        let outcome = migrate_data_files(&dir, true).unwrap();
        assert!(outcome.replaced.is_empty() && outcome.installed.is_empty());
        assert_eq!("settings of my own", std::fs::read_to_string(&config).unwrap());
        assert_eq!("m  MATLAB, Objective-C", std::fs::read_to_string(&priority).unwrap());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // The one branch that removes a file, which is why it has to be told apart from a file of their
    // own by more than the fact that we do not ship it: only the manifest remembers writing it.
    #[test]
    fn a_file_we_no_longer_ship_is_moved_out_and_one_of_their_own_is_left_alone() {
        let dir = SCRATCH_DIR.to_owned() + "migration-withdrawn/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        migrate_data_files(&dir, false).unwrap();

        let withdrawn = dir.clone() + "languages/Gone.txt";
        let theirs = dir.clone() + "languages/Mine.txt";
        std::fs::write(&withdrawn, "a language of an earlier version").unwrap();
        std::fs::write(&theirs, "a language of my own").unwrap();
        let manifest = dir.clone() + "installed.txt";
        let recorded = std::fs::read_to_string(&manifest).unwrap();
        std::fs::write(&manifest, recorded + "languages/Gone.txt 1
").unwrap();

        let outcome = migrate_data_files(&dir, true).unwrap();
        assert_eq!(vec!["languages/Gone.txt".to_owned()], outcome.withdrawn);
        assert!(!std::path::Path::new(&withdrawn).exists());
        assert_eq!("a language of an earlier version",
                std::fs::read_to_string(format!("{dir}replaced/{VERSION_ID}/languages/Gone.txt")).unwrap());
        assert_eq!("a language of my own", std::fs::read_to_string(&theirs).unwrap());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // Found by running the thing and not by any test, which is the shape of what the tests were
    // missing: every one of them starts from an empty directory, so the manifest is always in step
    // with the code that reads it, and an installation whose state predates the binary is the one
    // case this exists for. The themes moved from the managed set to the one that is written and
    // never touched, their entries stayed behind in the manifest, and the branch for what we no
    // longer ship deleted every one of them.
    #[test]
    fn a_file_that_stopped_being_managed_is_not_a_file_that_stopped_being_shipped() {
        let dir = SCRATCH_DIR.to_owned() + "migration-recategorised/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        migrate_data_files(&dir, false).unwrap();

        // as an earlier version of the code recorded them, before they were left alone
        let manifest = dir.clone() + "installed.txt";
        let recorded = std::fs::read_to_string(&manifest).unwrap();
        std::fs::write(&manifest, recorded + "themes/Dracula.txt 1\nconfig/default.txt 2\n").unwrap();

        let outcome = migrate_data_files(&dir, true).unwrap();
        assert!(outcome.withdrawn.is_empty(), "still shipped, and taken away: {:?}", outcome.withdrawn);
        assert!(std::path::Path::new(&(dir.clone() + "themes/Dracula.txt")).exists());
        assert!(std::path::Path::new(&(dir.clone() + "config/default.txt")).exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // The trigger is that the recorded version differs, not that it is older, so that an older
    // binary installs the data that matches it. A manifest that cannot be read is a new installation,
    // which is what keeps the mechanism self-healing.
    #[test]
    fn any_version_but_this_one_makes_the_pass_run_and_so_does_an_unreadable_manifest() {
        let dir = SCRATCH_DIR.to_owned() + "migration-manifest/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        migrate_data_files(&dir, false).unwrap();
        assert!(migrate_data_files(&dir, false).unwrap().installed.is_empty());

        for recorded in ["v99.0.0", "v0.0.1", "", "not a version at all"] {
            std::fs::write(dir.clone() + "installed.txt", recorded).unwrap();
            std::fs::remove_file(dir.clone() + "languages/Zig.txt").unwrap();
            assert_eq!(vec!["languages/Zig.txt".to_owned()], migrate_data_files(&dir, false).unwrap().installed,
                    "a manifest recording '{recorded}' did not make the pass run");
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // Load bearing: the shipped files are written with carriage returns and an editor that saves one
    // back without them changes every line and no meaning. Converting the whole of 'data' to CRLF
    // moved 41 files and the forced pass that followed archived none of them, which is this.
    #[test]
    fn the_hash_does_not_see_line_endings() {
        assert_eq!(content_hash(b"first
second
"), content_hash(b"first
second
"));
        assert_ne!(content_hash(b"first
second
"), content_hash(b"first
third
"));
    }

    // What decides the refusal above. A message and a document both want the output, and the whole
    // point of asking for a document is to redirect it, so the message would land in the file.
    #[test]
    fn a_message_and_a_json_document_are_never_asked_for_together() {
        assert!(crate::asks_for_a_json_document("./src --output json"));
        // the value is read the same way the configuration reads it, so the spelling is the same one
        assert!(crate::asks_for_a_json_document("--output JSON --help"));
        assert!(!crate::asks_for_a_json_document("./src --output text --help"));
        assert!(!crate::asks_for_a_json_document("./src --help"));
        // a value that is not one of the two is the configuration's mistake to report, not this one
        assert!(!crate::asks_for_a_json_document("./src --output jsonn --help"));
        // and '--output' written last of all is read without falling off the end of the line
        assert!(!crate::asks_for_a_json_document("./src --output"));
    }

    fn java_and_csharp() -> Vec<Language> {
        vec![Language::new("Java", [""; 0], ["\""], [""; 0], None, []),
             Language::new("C#", [""; 0], ["\""], [""; 0], None, [])]
    }

    // The counterpart of this, that the list really is narrowed, is asserted next to the run, which
    // is where the narrowing happens now. What is left here is the part a person reads.
    #[test]
    fn test_report_unknown_languages() {
        let available = java_and_csharp();

        // every name exists, so there is nothing to say
        assert!(report_unknown_languages(&available, &["java".to_owned()]).unwrap().is_none());

        // one of the three is real, so the other two are reported and the run goes on
        let some_unknown = ["java".to_owned(), "c++".to_owned(), "Rust".to_owned()];
        assert!(report_unknown_languages(&available, &some_unknown).unwrap().is_some());

        // none of them is, and that stops the run
        let all_unknown = ["c++".to_owned(), "Rust".to_owned()];
        assert!(report_unknown_languages(&available, &all_unknown).is_err());
    }

    // An installation that has been given a second file declaring a name it already had is one
    // language described twice, and the correction offered for a misspelling of it has to say so
    // once. The list used to arrive as a map, which deduplicated it without anybody deciding to.
    #[test]
    fn a_language_declared_by_two_files_is_suggested_once() {
        let mut available = java_and_csharp();
        available.push(Language::new("Java", [""; 0], ["\""], [""; 0], None, []));

        let report = report_unknown_languages(&available, &["jaava".to_owned(), "C#".to_owned()])
                .unwrap().expect("a misspelling was not reported at all");
        assert_eq!(1, report.matches("Java").count(), "'Java' was offered more than once:\n{report}");
    }
}
