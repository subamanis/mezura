// What one run produced, as opposed to the vocabulary it produced it in, which is 'domain.rs'. A
// 'Language' exists before anything has been counted; a 'FinalStats' does not.
use std::collections::HashMap;

use crate::engine::modules::{ModuleId, Modules};

#[derive(Debug)]
pub struct Metrics {
    pub files_per_sec: usize,
    pub lines_per_sec: usize
}

// What one run produces, and the only thing 'run' returns. Presentation is a separate call, so the
// same result can be printed, written as JSON, compared with another one, or read by a caller that
// wants none of those.
#[derive(Debug)]
pub struct RunResult {
    // The totals across every module. A run that named none has exactly one module holding the same
    // numbers, and reading these is what every question about the whole run goes through.
    pub content_info_map: HashMap<String, LanguageContentInfo>,
    pub languages_metadata_map: HashMap<String, LanguageMetadata>,
    pub modules: Vec<ModuleResult>,
    pub final_stats: FinalStats,
    pub faulty_files: Vec<FaultyFileDetails>,
    pub files_present: FilesPresent,
    pub scan_duration_millis: u128,
    pub metrics: Option<Metrics>,
    // Directories the walk found and could not open, so everything under them is missing from every
    // number above. Empty on an ordinary run, and the one thing that says the counts are short.
    pub unreadable_dirs: Vec<String>
}

// One part of the run, counted on its own. 'name' is None for the leftovers of the named ones, which
// is also the single unnamed one of a run that declared no modules at all.
#[derive(Debug)]
pub struct ModuleResult {
    pub name: Option<String>,
    pub content_info_map: HashMap<String, LanguageContentInfo>,
    pub languages_metadata_map: HashMap<String, LanguageMetadata>,
    pub final_stats: FinalStats
}

impl RunResult {
    // Nothing of interest was found, which is an answer and not a failure: the counts are zero and
    // the file numbers still say how many were looked at and how many were excluded.
    //
    // The modules are built here for the same reason the ordinary path builds them: one that found
    // nothing was still asked for by name, and its absence reads as a mistake in the report rather
    // than as an empty part. Leaving them out also made the two answers disagree, since 'has_modules'
    // then said no and the whole block vanished from the document exactly when the scan was empty.
    pub(crate) fn of_nothing(files_present: FilesPresent, scan_duration_millis: u128, modules: &Modules,
            unreadable_dirs: Vec<String>) -> Self {
        RunResult {
            content_info_map: HashMap::new(),
            languages_metadata_map: HashMap::new(),
            modules: (0..modules.count()).map(|id| ModuleResult {
                name: modules.name_of(id as ModuleId).map(str::to_owned),
                content_info_map: HashMap::new(),
                languages_metadata_map: HashMap::new(),
                final_stats: FinalStats::new_extended(0, 0, 0, 0, 0, 0, 0)
            }).collect(),
            final_stats: FinalStats::new_extended(0, 0, 0, 0, 0, 0, 0),
            faulty_files: Vec::new(),
            files_present,
            scan_duration_millis,
            metrics: None,
            unreadable_dirs
        }
    }

    // Whether files were found and every one of them failed to parse, which is a different answer
    // from the empty scan: there nothing failed, because there was nothing. Offered here because the
    // obvious comparison is wrong exactly there, the two counts being equal when both are zero.
    pub fn all_relevant_files_were_faulty(&self) -> bool {
        !self.faulty_files.is_empty() && self.faulty_files.len() == self.files_present.relevant_files
    }

    // Whether the report has a second axis at all. One name is enough for the column to appear, and
    // without one there is nothing to group by and the output is what it always was.
    pub fn has_modules(&self) -> bool {
        self.modules.iter().any(|x| x.name.is_some())
    }
}

// 'extra_lines' is what is left after the code and the comments: blank lines, and lines that the
// language required but that say nothing, like a closing brace. The three add up to 'lines'.
// '#[non_exhaustive]' because the fields are not independent: 'size' is 'bytes_size' in another unit,
// and 'extra_lines' is what the other two leave over. Every number is readable from outside and only
// this crate can put one together, so no caller can build a set that disagrees with itself. The types
// above are plain bags and are constructible by anyone who wants to test their own rendering.
#[derive(Debug, PartialEq)]
#[non_exhaustive]
pub struct FinalStats {
    pub files: usize,
    pub lines: usize,
    pub code_lines: usize,
    pub comment_lines: usize,
    pub extra_lines: usize,
    pub bytes_size: usize,
    pub bytes_average_size: usize,
    pub size: f64,
    pub size_measurement: String,
    pub average_size: f64,
    pub average_size_measurement: String
}

#[derive(Debug)]
#[non_exhaustive]
pub struct FaultyFileDetails {
    pub path: String,
    pub error_msg: String,
    pub size: u64
}

// A mistake in the configuration itself, which only a library caller can make: the command line
// validates its own input before it ever builds a run. Finding nothing is a result, and every file
// failing to parse is a result with the reasons attached, so neither lives here.
#[derive(Debug)]
#[non_exhaustive]
pub enum RunError {
    // The pattern as the caller wrote it, not the anchored form the matcher builds from it
    InvalidExcludePattern(String)
}

#[derive(Debug,Default,Clone,Copy)]
pub struct FilesPresent {
    pub total_files: usize,
    pub relevant_files: usize,
    pub excluded_files: usize
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidExcludePattern(x) => write!(f, "'{x}' is not a valid exclude pattern, so nothing was counted.")
        }
    }
}

impl std::error::Error for RunError {}

impl FinalStats {
    pub fn new(files: usize, lines: usize, code_lines: usize, comment_lines: usize, bytes_size: usize) -> Self
    {
        // A result with no files is an answer and not a mistake, and this is a public door
        let bytes_average_size = bytes_size.checked_div(files).unwrap_or(0);
        let (size, size_measurement) = FinalStats::get_formatted_size_and_measurement(bytes_size);
        let size = round_1(size);
        let (average_size, average_size_measurement) = Self::get_formatted_size_and_measurement(bytes_average_size);
        let average_size = round_1(average_size);
        FinalStats {
            files,
            lines,
            code_lines,
            comment_lines,
            // Saturating for the same reason the division above is checked: this is a public door,
            // and three counts that do not add up are the caller's arithmetic, not a reason to panic.
            extra_lines: lines.saturating_sub(code_lines).saturating_sub(comment_lines),
            bytes_size,
            bytes_average_size,
            size,
            size_measurement,
            average_size,
            average_size_measurement,
        }
    }

    pub fn new_extended(files: usize, lines: usize, code_lines: usize, comment_lines: usize, extra_lines: usize,
            bytes_size: usize, bytes_average_size: usize) -> Self {
        let (size, size_measurement) = FinalStats::get_formatted_size_and_measurement(bytes_size);
        let size = round_1(size);
        let (average_size, average_size_measurement) = Self::get_formatted_size_and_measurement(bytes_average_size);
        let average_size = round_1(average_size);

        FinalStats {
            files,
            lines,
            code_lines,
            comment_lines,
            extra_lines,
            bytes_size,
            bytes_average_size,
            size,
            size_measurement,
            average_size,
            average_size_measurement,
        }
    }

    pub fn calculate(content_info_map: &HashMap<String,LanguageContentInfo>, languages_metadata_map: &HashMap<String,LanguageMetadata>) -> Self {
        let (mut total_files, mut total_lines, mut total_code_lines, mut total_comment_lines, mut total_bytes) = (0, 0, 0, 0, 0);
        languages_metadata_map.values().for_each(|e| {total_files += e.files; total_bytes += e.bytes});
        content_info_map.values().for_each(|c| {total_lines += c.lines; total_code_lines += c.code_lines;
                total_comment_lines += c.comment_lines});
        let bytes_size = total_bytes;
        let bytes_average_size = total_bytes.checked_div(total_files).unwrap_or(0);
        let (total_size, size_measurement) = Self::get_formatted_size_and_measurement(total_bytes);
        let (average_size, average_size_measurement) = Self::get_formatted_size_and_measurement(bytes_average_size);
        let total_size = round_1(total_size);
        let average_size = round_1(average_size);


        FinalStats {
            files: total_files,
            lines: total_lines,
            code_lines: total_code_lines,
            comment_lines: total_comment_lines,
            extra_lines: total_lines - total_code_lines - total_comment_lines,
            bytes_size,
            bytes_average_size,
            size: total_size,
            size_measurement,
            average_size,
            average_size_measurement
        }
    }

    fn get_formatted_size_and_measurement(value: usize) -> (f64, String) {
        if value >= 1000000000 {(value as f64 / 1000000000f64, "GBs".to_owned())}
        else if value >= 1000000 {(value as f64 / 1000000f64, "MBs".to_owned())}
        else if value >= 1000 {(value as f64 / 1000f64, "KBs".to_owned())}
        else {(value as f64, "Bytes".to_owned())}
    }
}

impl FaultyFileDetails {
    pub fn new(path: String, error_msg: String, size: u64) -> Self {
        FaultyFileDetails {
            path,
            error_msg,
            size
        }
    }
}


pub fn round_1(num: f64) -> f64 {
    (num * 10.0).round() / 10.0
}

use crate::{LanguageContentInfo, LanguageMetadata};


