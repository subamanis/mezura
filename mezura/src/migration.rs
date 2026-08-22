// Keeping the data directory in step with the binary: writing what is missing, correcting what we
// wrote ourselves, and never destroying anything of the user's without keeping a copy of it.
use std::collections::{HashMap, HashSet};

use colored::Colorize;
use include_dir::{File, include_dir};
use mezura_core::EXTENSION_PRIORITY_FILE_NAME;

use crate::config_manager::VERSION_ID;
use crate::message_printer::wrap_message;
use crate::paths::{CONFIG_DIR_NAME, DEFAULT_CONFIG_NAME, LANGUAGES_DIR_NAME, LOGS_DIR_NAME,
        THEMES_DIR_NAME};

const MANIFEST_FILE_NAME : &str = "installed.txt";
const REPLACED_DIR_NAME : &str = "replaced";
// Enough to see what kind of thing went wrong without the list becoming the message
const FAILURES_NAMED : usize = 6;

#[derive(Default)]
pub struct MigrationOutcome {
    // Written before and gone, against brought for the first time. A language file that never
    // existed here was not lost, and telling somebody it was sends them looking for what took it.
    pub restored: Vec<String>,
    pub added: Vec<String>,
    pub replaced: Vec<String>,
    // Ours, unchanged since we wrote it, and corrected by this version: nothing of theirs is at
    // stake, and their counts still move.
    pub updated: Vec<String>,
    pub withdrawn: Vec<String>,
    pub merged: Vec<String>,
    // Under 'replaced/<version>/', named after the moment the pass ran. One folder per pass, or
    // nothing says which run a copy came from.
    pub archived_under: String,
    // A list rather than one error: a file locked by something else costs that file and not the
    // sixty after it
    pub failed: Vec<String>,
    // The only part of the directory a run cannot do without
    pub languages_failed: usize
}

impl MigrationOutcome {
    // Asked by '--restore', which has to say something even when there was nothing to do
    pub fn did_nothing(&self) -> bool {
        self.restored.is_empty() && self.added.is_empty() && self.replaced.is_empty()
                && self.updated.is_empty() && self.withdrawn.is_empty() && self.merged.is_empty()
                && self.failed.is_empty()
    }

    // Only the language files decide this: a theme or the priority file that could not be written
    // leaves the counting exactly as it was.
    pub fn every_language_file_is_in_place(&self) -> bool {
        self.languages_failed == 0
    }

    // A first installation lost nothing, so it says nothing: everything it wrote is new
    pub fn format_restored(&self) -> Option<String> {
        if self.restored.is_empty() {
            return None;
        }

        Some(format!("\n{}\n", wrap_message(&format!(
                "Part of your data directory was missing and has been written again:\n  {}",
                self.restored.join(", "))).yellow()))
    }

    pub fn format_replaced(&self) -> Option<String> {
        if self.replaced.is_empty() {
            return None;
        }

        // It never says "you changed this": all that is known is that the contents are not the ones
        // mezura wrote, and a copy from somewhere and a hand edit arrive here alike
        let (count, plural) = (self.replaced.len(), if self.replaced.len() == 1 {"file"} else {"files"});
        Some(format!("\n{}\n", wrap_message(&format!(
                "Updated the data files for {VERSION_ID}.\n{count} {plural} on disk {} not {} mezura had written, \
so {} kept in '{}{REPLACED_DIR_NAME}/{VERSION_ID}/{}/' in case you want anything out of {}:\n  {}",
                if count == 1 {"was"} else {"were"}, if count == 1 {"the one"} else {"the ones"},
                if count == 1 {"it was"} else {"they were"},
                crate::paths::PERSISTENT_APP_PATHS.data_dir, self.archived_under,
                if count == 1 {"it"} else {"them"},
                self.replaced.join(", ")))).yellow().to_string())
    }

    // Counted rather than listed: there is nothing for them to do about it, and a release that
    // improves twenty language files would open with twenty names.
    pub fn format_updated(&self) -> Option<String> {
        if self.updated.is_empty() {
            return None;
        }

        let (count, plural) = (self.updated.len(), if self.updated.len() == 1 {"file"} else {"files"});
        Some(format!("\n{}\n", wrap_message(&format!(
                "Brought {count} data {plural} up to date for {VERSION_ID}, so counts that depend on {} may \
change.", if count == 1 {"it"} else {"them"})).yellow()))
    }

    // Deleted from their directory, which is more than a replaced file loses
    pub fn format_withdrawn(&self) -> Option<String> {
        if self.withdrawn.is_empty() {
            return None;
        }

        Some(format!("\n{}\n", wrap_message(&format!(
                "No longer part of {VERSION_ID}, and moved to '{}{REPLACED_DIR_NAME}/{VERSION_ID}/{}/':\n  {}",
                crate::paths::PERSISTENT_APP_PATHS.data_dir, self.archived_under,
                self.withdrawn.join(", "))).yellow()))
    }

    // Its own line and not a place among the corrected files: this is the one they are meant to
    // edit, so they need to know their answers are still in it.
    pub fn format_merged(&self) -> Option<String> {
        if self.merged.is_empty() {
            return None;
        }

        Some(format!("\n{}\n", wrap_message(&format!(
                "Added the rules {VERSION_ID} brings to '{}', and kept every rule you had written. Your \
copy as it was is in '{}{REPLACED_DIR_NAME}/{VERSION_ID}/{}/'.",
                self.merged.join("', '"), crate::paths::PERSISTENT_APP_PATHS.data_dir,
                self.archived_under)).yellow()))
    }

    pub fn format_failures(&self) -> Option<String> {
        if self.failed.is_empty() {
            return None;
        }

        // A directory that cannot be written at all fails on every file in it, and eighty-five
        // lines saying so are worth less than the first of them beside the count
        let named = self.failed.iter().take(FAILURES_NAMED).cloned().collect::<Vec<_>>().join("\n  ");
        let rest = match self.failed.len().saturating_sub(FAILURES_NAMED) {
            0 => String::new(),
            more => format!("\n  and {more} more")
        };
        // The priority file decides which language wins an extension two of them claim, so losing it
        // moves counts while every language file is present.
        let counting_with = if self.languages_failed > 0 {
            "Counting with the copies inside the program, so a language file of your own is not in use."
        } else if self.failed.iter().any(|x| x.starts_with(EXTENSION_PRIORITY_FILE_NAME)) {
            "Every language file is in place. Until this one is readable, an extension more than one \
language claims is settled alphabetically, and each such extension says so on its own line."
        } else {
            "Every language file is in place, so the counting is unaffected."
        };
        Some(format!("\n{}\n", wrap_message(&format!(
                "Could not update {} of your data files:\n  {named}{rest}\n{counting_with}",
                self.failed.len())).yellow()))
    }

    // Deliberately not counted against the languages: the run counts perfectly well with one extra
    // file in that folder, while 'languages_failed' abandons the whole directory for the copies
    // inside the program.
    fn could_not_withdraw(&mut self, relative: &str, error: &std::io::Error) {
        self.failed.push(format!("{relative}: {error}"));
    }

    // Every filesystem step goes through here, so one that fails is named and the pass carries on
    fn attempt(&mut self, relative: &str, result: Result<(), std::io::Error>) -> bool {
        match result {
            Ok(()) => true,
            Err(error) => {
                self.failed.push(format!("{relative}: {error}"));
                if relative.starts_with(LANGUAGES_DIR_NAME) {
                    self.languages_failed += 1;
                }
                false
            }
        }
    }
}

// Brings the data directory to what this version ships. The shipped copy always wins and the user's
// is kept, and a file we never wrote is never touched, which is what makes a language of their own
// safe. 'force' is '--restore': do it again even though there is nothing to do.
pub fn migrate_data_files(data_dir: &str, force: bool) -> MigrationOutcome {
    let mut outcome = MigrationOutcome::default();
    perform_migration(data_dir, force, &mut outcome);

    outcome
}

fn perform_migration(data_dir: &str, force: bool, outcome: &mut MigrationOutcome) {
    let recorded = read_manifest(data_dir);
    let directories = [LANGUAGES_DIR_NAME, THEMES_DIR_NAME, CONFIG_DIR_NAME, LOGS_DIR_NAME];
    // The priority file belongs here although it is never replaced: this is the record of what was
    // last shipped, and without it a release that adds a rule and no language matches every hash,
    // returns below, and never reaches the merge.
    let carried = shipped_files().into_iter()
            .map(|(relative, contents)| (relative, content_hash(contents)))
            .chain([(EXTENSION_PRIORITY_FILE_NAME.to_owned(),
                    content_hash(read_baked_in_extension_priority_contents().as_bytes()))])
            .collect::<HashMap<_, _>>();
    // Asked of every file rather than of the folder holding it: one language file left behind by a
    // quarantine answers "the folder is not empty" while sixty-six others are missing. And asked of
    // what this version ships rather than of what the record remembers, since a file that could not
    // be written is absent from both and the record would call it present.
    let everything_is_there = carried.keys()
            .all(|relative| holds_something(&(data_dir.to_owned() + relative)))
            // The looser question for the ones written once and left alone, since an empty one of
            // those is somebody's decision and not damage
            && written_once_files().iter()
                    .all(|relative| std::path::Path::new(&(data_dir.to_owned() + relative)).exists())
            // 'is_dir', or a plain file where the folder belongs answers yes forever. The four are
            // named because 'logs' holds nothing that ships and no file above stands for it.
            && directories.iter().all(|name| std::path::Path::new(&(data_dir.to_owned() + name)).is_dir());
    // Whether the record describes the files this binary carries, and not whether the version string
    // moved: the two differ for every build made between releases, where the files change and
    // 'VERSION_ID' does not.
    if !force && recorded == carried && everything_is_there {
        return;
    }

    // Chosen once, so everything this pass moves aside lands together
    outcome.archived_under = find_free_archive_folder(data_dir);
    let archived_under = outcome.archived_under.clone();

    for name in directories {
        // The logs directory holds nothing that ships, but without it a run with '--log' has nowhere to write
        let path = data_dir.to_owned() + name;
        let was_there = std::path::Path::new(&path).exists();
        if outcome.attempt(name, std::fs::create_dir_all(&path)) && !was_there {
            note_written_file(outcome, name.to_owned() + "/", !recorded.is_empty());
        }
    }

    // A file enters the manifest only once it is really on disk with the contents this version
    // ships, so one that could not be written is retried by the next run
    let mut manifest = HashMap::new();
    for (relative, contents) in shipped_files() {
        let target = data_dir.to_owned() + &relative;
        let shipped_hash = content_hash(contents);
        let was_recorded = recorded.contains_key(&relative);

        let on_disk = std::fs::read(&target).ok();
        // There and unreadable is not missing: writing over it would destroy what it holds without
        // keeping a copy, which is the one thing this pass exists not to do.
        if on_disk.is_none() && std::path::Path::new(&target).exists() {
            outcome.attempt(&relative, Err(std::io::Error::other("it is there and could not be read")));
            continue;
        }
        let Some(on_disk) = on_disk else {
            if outcome.attempt(&relative, std::fs::write(&target, contents)) {
                manifest.insert(relative.clone(), shipped_hash);
                note_written_file(outcome, relative, was_recorded);
            }
            continue;
        };

        let on_disk_hash = content_hash(&on_disk);
        if on_disk_hash == shipped_hash {
            manifest.insert(relative, shipped_hash);
            continue;
        }
        if recorded.get(&relative) == Some(&on_disk_hash) || means_the_same(&on_disk, contents) {
            if outcome.attempt(&relative, std::fs::write(&target, contents)) {
                manifest.insert(relative.clone(), shipped_hash);
                outcome.updated.push(relative);
            }
            continue;
        }

        // Copied first and named at once, so a copy sitting in 'replaced' is never one the messages
        // left out
        let copied = match archive(data_dir, &archived_under, &relative, &on_disk) {
            Ok(copied) => copied,
            Err(error) => {
                outcome.attempt(&relative, Err(error));
                continue;
            }
        };
        outcome.replaced.push(relative.clone());
        if !outcome.attempt(&relative, std::fs::write(&target, contents)) {
            // Nothing was replaced after all, their file is still theirs on disk, and saying
            // otherwise contradicts the failure printed beside it
            outcome.replaced.pop();
            if copied {
                let _ = std::fs::remove_file(find_archived_path(data_dir, &archived_under, &relative));
            }
            continue;
        }
        manifest.insert(relative, shipped_hash);
    }

    // Weighed against everything we ship rather than against what this pass manages: a file that
    // moved from the one set to the other, as the themes did, is still shipped
    let still_shipped = carried.keys().cloned()
            .chain(written_once_files())
            .collect::<HashSet<_>>();
    // A file shipped as 'go.txt' and now as 'Go.txt' is one file on Windows and macOS, so the new
    // name was written over the old while the record still names the old, and deleting by name
    // would take away what was written a moment ago. Where the two names really are two files, the
    // old one holds its old bytes and is withdrawn.
    let ours_now = manifest.values().copied().collect::<HashSet<_>>();
    for (relative, hash) in recorded.iter().filter(|(relative, _)| !still_shipped.contains(*relative)) {
        let target = data_dir.to_owned() + relative;
        // Only "already gone" ends it. Anything else keeps the record for the next run to try
        // again: the manifest is the only thing that remembers a file was ever ours, and dropping
        // it over one failed delete leaves the file installed for good, past '--restore' too.
        let on_disk = match std::fs::read(&target) {
            Ok(on_disk) => on_disk,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                manifest.insert(relative.clone(), *hash);
                outcome.could_not_withdraw(relative, &error);
                continue;
            }
        };
        if ours_now.contains(&content_hash(&on_disk)) {
            continue;
        }
        match archive(data_dir, &archived_under, relative, &on_disk)
                .and_then(|_| std::fs::remove_file(&target)) {
            Ok(()) => outcome.withdrawn.push(relative.clone()),
            Err(error) => {
                manifest.insert(relative.clone(), *hash);
                outcome.could_not_withdraw(relative, &error);
            }
        }
    }

    // Written when absent and never touched again, and left out of the manifest so nothing can
    // reach them later either. A theme that has fallen behind breaks nothing, since a token it does
    // not name falls back to a default.
    for (relative, contents) in include_dir!("data/themes").files.iter().map(|file| named(THEMES_DIR_NAME, file)) {
        let target = data_dir.to_owned() + &relative;
        if !std::path::Path::new(&target).exists()
                && outcome.attempt(&relative, std::fs::write(&target, contents)) {
            note_written_file(outcome, relative, !recorded.is_empty());
        }
    }

    let default_config = format!("{data_dir}{CONFIG_DIR_NAME}/{DEFAULT_CONFIG_NAME}");
    let default_relative = format!("{CONFIG_DIR_NAME}/{DEFAULT_CONFIG_NAME}");
    if !std::path::Path::new(&default_config).exists()
            && outcome.attempt(&default_relative,
                    std::fs::write(&default_config, read_baked_in_default_config_contents())) {
        note_written_file(outcome, default_relative, !recorded.is_empty());
    }
    merge_the_priority_file(data_dir, &archived_under, &recorded, &mut manifest, outcome);

    // Last, and holding only what reached the disk, so whatever failed is absent from the record,
    // missing from the completeness check, and tried again by the next run
    outcome.attempt(MANIFEST_FILE_NAME, write_manifest(data_dir, &manifest));
}

// A file the manifest never recorded is one this version brings and not one that was lost. The
// themes and the default configuration are outside the manifest, so for those the question is only
// whether this installation existed before.
fn note_written_file(outcome: &mut MigrationOutcome, relative: String, was_recorded: bool) {
    if was_recorded {
        outcome.restored.push(relative);
    } else {
        outcome.added.push(relative);
    }
}

// The one shipped file that is neither replaced nor left alone: replacing it throws away the answer
// somebody gave to a contested extension, and leaving it alone hides everything a new version adds.
// What enters the manifest is the shipped copy's hash and never the merged file's, since the record
// says what was last brought while the file on disk is theirs and ours together.
fn merge_the_priority_file(data_dir: &str, archived_under: &str, recorded: &HashMap<String, u64>,
        manifest: &mut HashMap<String, u64>, outcome: &mut MigrationOutcome)
{
    let path = data_dir.to_owned() + EXTENSION_PRIORITY_FILE_NAME;
    let ours = read_baked_in_extension_priority_contents();
    let brought = content_hash(ours.as_bytes());
    let theirs = match std::fs::read_to_string(&path) {
        Ok(theirs) => theirs,
        // Not there at all is a first installation, or one that lost it. Anything else is a file
        // that is there and unreadable, and writing over it would destroy it without a copy.
        Err(_) if !std::path::Path::new(&path).exists() => {
            if outcome.attempt(EXTENSION_PRIORITY_FILE_NAME, std::fs::write(&path, &ours)) {
                manifest.insert(EXTENSION_PRIORITY_FILE_NAME.to_owned(), brought);
                note_written_file(outcome, EXTENSION_PRIORITY_FILE_NAME.to_owned(),
                        recorded.contains_key(EXTENSION_PRIORITY_FILE_NAME));
            }
            return;
        },
        Err(error) => {
            outcome.attempt(EXTENSION_PRIORITY_FILE_NAME, Err(error));
            return;
        }
    };

    let Some(merged) = merge_priority_files(&theirs, &ours) else {
        manifest.insert(EXTENSION_PRIORITY_FILE_NAME.to_owned(), brought);
        return;
    };
    let copied = match archive(data_dir, archived_under, EXTENSION_PRIORITY_FILE_NAME, theirs.as_bytes()) {
        Ok(copied) => copied,
        Err(error) => {
            outcome.attempt(EXTENSION_PRIORITY_FILE_NAME, Err(error));
            return;
        }
    };
    // Their file is untouched when the write fails, so the copy would sit in 'replaced' with
    // nothing that ever moved to point at
    if !outcome.attempt(EXTENSION_PRIORITY_FILE_NAME, std::fs::write(&path, merged)) {
        if copied {
            let _ = std::fs::remove_file(find_archived_path(data_dir, archived_under,
                    EXTENSION_PRIORITY_FILE_NAME));
        }
        return;
    }
    manifest.insert(EXTENSION_PRIORITY_FILE_NAME.to_owned(), brought);
    outcome.merged.push(EXTENSION_PRIORITY_FILE_NAME.to_owned());
}

// Their copy brought up to what this version ships, or None when it already holds all of it. Every
// rule they wrote is kept, as written and in the order they left it, and what arrives is the
// explanation, a section their copy does not have, and a contest it never mentions. A rule they
// deleted comes back for that last reason: an extension is handed to another language by reordering
// the names on its line, which is what the file says to do.
fn merge_priority_files(theirs: &str, ours: &str) -> Option<String> {
    let (_, their_blocks) = read_priority_blocks(theirs);
    let (preamble, our_blocks) = read_priority_blocks(ours);
    let nothing : Vec<&str> = Vec::new();

    let mut merged = preamble;
    for ours_here in &our_blocks {
        let theirs_here = their_blocks.iter()
                .find(|x| x.opens.is_some() && x.opens == ours_here.opens)
                .map(|x| &x.rules).unwrap_or(&nothing);
        let already_settled = theirs_here.iter().map(|rule| find_key_of_rule(ours_here.marker, rule))
                .collect::<HashSet<_>>();

        merged.push(ours_here.marker);
        merged.extend(theirs_here.iter().copied());
        merged.extend(ours_here.rules.iter().copied()
                .filter(|rule| !already_settled.contains(&find_key_of_rule(ours_here.marker, rule))));
        merged.push("");
    }
    // A block of their own, under a marker this version knows nothing about, is theirs to keep
    for theirs_here in their_blocks.iter().filter(|x| x.opens.is_none()
            || !our_blocks.iter().any(|ours| ours.opens == x.opens)) {
        merged.push(theirs_here.marker);
        merged.extend(theirs_here.rules.iter().copied());
        merged.push("");
    }

    let ending = if theirs.contains("\r\n") {"\r\n"} else {"\n"};
    let merged = merged.join(ending) + ending;
    if reads_the_same(&merged, theirs) || !every_answer_of_theirs_survived(theirs, &merged) {
        return None;
    }

    Some(merged)
}

// The merge matches sections on 'opens' and prints 'marker', which is why a section carries both
struct PriorityBlock<'a> {
    opens: Option<Holds>,
    marker: &'a str,
    rules: Vec<&'a str>
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Holds {
    Extensions,
    Filenames
}

// The explanation above the first marker, then each section with its rules. Blank lines inside a
// section are dropped, since the merge writes its own between the sections.
fn read_priority_blocks(contents: &str) -> (Vec<&str>, Vec<PriorityBlock<'_>>) {
    let (mut preamble, mut blocks) = (Vec::new(), Vec::<PriorityBlock>::new());
    let mut current = None;

    // A file re-saved by PowerShell or an older Notepad carries a byte order mark, which is not
    // whitespace and sits in front of the first marker, so every rule under it would read as
    // explanation
    for line in crate::config_files::strip_byte_order_mark(contents).lines() {
        if line.trim_start().starts_with("===>") {
            let marker = line.trim();
            let opens = find_what_the_marker_opens(marker);
            // Two sections under one name are one section to the parser, which simply reopens the
            // block, and a file grows a second one by the ordinary act of appending to it
            current = Some(match opens.and_then(|opens| blocks.iter().position(|x| x.opens == Some(opens))) {
                Some(already) => already,
                None => {
                    blocks.push(PriorityBlock { opens, marker, rules: Vec::new() });
                    blocks.len() - 1
                }
            });
        } else if let Some(at) = current {
            if !line.trim().is_empty() {
                blocks[at].rules.push(line.trim_end());
            }
        } else {
            preamble.push(line.trim_end());
        }
    }

    (preamble, blocks)
}

// Asked of the parser that reads the file rather than answered here, so the two cannot disagree
// about what a marker is: '===>contested-extensions' and a marker with a word after its name both
// open the extensions section for the program that counts.
fn find_what_the_marker_opens(marker: &str) -> Option<Holds> {
    let (rules, _) = mezura_core::language_file::parse_priority(&format!("{marker}\nprobe Probe\n"));

    if !rules.by_extension.is_empty() {
        Some(Holds::Extensions)
    } else if !rules.by_filename.is_empty() {
        Some(Holds::Filenames)
    } else {
        None
    }
}

// A net under the merge: if an answer they gave is missing from the result, their file is left
// exactly as it stands. Bringing them the new rules is worth less than keeping the ones they gave.
fn every_answer_of_theirs_survived(theirs: &str, merged: &str) -> bool {
    let (theirs, merged) = (mezura_core::language_file::parse_priority(theirs).0,
            mezura_core::language_file::parse_priority(merged).0);

    theirs.by_extension.iter().all(|(key, names)| merged.by_extension.get(key) == Some(names))
            && theirs.by_filename.iter().all(|(key, names)| merged.by_filename.get(key) == Some(names))
}

// Keyed by the parser as well, so a rule written '.m' and one written 'M' stay the one contest they
// are to it. Keyed apart, the merge writes a second line settling what the first already settled.
fn find_key_of_rule(marker: &str, rule: &str) -> String {
    let (rules, _) = mezura_core::language_file::parse_priority(&format!("{marker}\n{rule}\n"));

    rules.by_extension.into_keys().chain(rules.by_filename.into_keys()).next()
            // A rule the parser makes nothing of, keyed on its own text so that two copies of it do
            // not both survive
            .unwrap_or_else(|| rule.trim().to_ascii_lowercase())
}

// Trailing whitespace, the line endings and the blank lines at the end of a file are not what the
// file says, and treating any of them as a difference rewrites it and announces that on every run.
fn reads_the_same(one: &str, other: &str) -> bool {
    let significant = |text: &str| text.lines().map(str::trim_end).collect::<Vec<_>>()
            .join("\n").trim_end().to_owned();

    significant(one) == significant(other)
}

// FNV-1a with every '\r' dropped. The shipped files are written with them, so an editor that saves
// one back with unix endings would make it look edited at every release.
fn content_hash(bytes: &[u8]) -> u64 {
    let mut hash : u64 = 0xcbf29ce484222325;
    for byte in bytes.iter().filter(|x| **x != b'\r') {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }

    hash
}

// One 'path hash' line per file. The version on its own line is not read back: it is there for
// whoever opens the file. A missing or unreadable manifest is a fresh installation, which is what
// makes this self-healing.
fn read_manifest(data_dir: &str) -> HashMap<String, u64> {
    let mut entries = HashMap::new();
    let Ok(contents) = std::fs::read_to_string(data_dir.to_owned() + MANIFEST_FILE_NAME) else {
        return entries;
    };

    for line in contents.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')) {
        // From the right, because a file name is allowed to hold spaces and a hash is not
        if let Some((path, hash)) = line.rsplit_once(' ') && let Ok(hash) = hash.trim().parse::<u64>() {
            entries.insert(path.trim().to_owned(), hash);
        }
    }

    entries
}

fn write_manifest(data_dir: &str, entries: &HashMap<String, u64>) -> Result<(), std::io::Error> {
    let mut sorted = entries.iter().collect::<Vec<_>>();
    sorted.sort();
    let body = sorted.into_iter().map(|(path, hash)| format!("{path} {hash}")).collect::<Vec<_>>().join("\n");

    std::fs::write(data_dir.to_owned() + MANIFEST_FILE_NAME,
            format!("# Written by mezura. It records which files it installed and what they looked like,\n\
# so that an update can tell a file you edited from one it wrote itself. Delete it and the next\n\
# run has no way to tell: every file of ours that you have changed is moved into 'replaced' and\n\
# written again from the copies inside the program.\n{VERSION_ID}\n{body}\n"))
}

fn named(dir_name: &str, file: &File<'static>) -> (String, &'static [u8]) {
    let name = std::path::Path::new(file.path).file_name().and_then(|x| x.to_str()).unwrap_or(file.path);
    (dir_name.to_owned() + "/" + name, file.contents)
}

// Nothing records what these looked like, so the repair check has to name them itself
fn written_once_files() -> Vec<String> {
    include_dir!("data/themes").files.iter().map(|file| named(THEMES_DIR_NAME, file).0)
            .chain([format!("{CONFIG_DIR_NAME}/{DEFAULT_CONFIG_NAME}")])
            .collect()
}

fn shipped_files() -> Vec<(String, &'static [u8])> {
    mezura_core::languages::get_shipped_language_files_raw().into_iter()
            .map(|(name, contents)| (LANGUAGES_DIR_NAME.to_owned() + "/" + name, contents)).collect()
}

// Whether the two say the same thing, which is not whether they read the same. This is what keeps
// the replacing honest: the only differences that survive it are the ones that change a count.
fn means_the_same(on_disk: &[u8], shipped: &[u8]) -> bool {
    let (theirs, ours) = (String::from_utf8_lossy(on_disk), String::from_utf8_lossy(shipped));
    match (mezura_core::language_file::parse_language(&theirs), mezura_core::language_file::parse_language(&ours)) {
        (Some(theirs), Some(ours)) => theirs == ours,
        // Ours always parses, so this is a file edited into something that no longer does, and
        // replacing it is a repair
        _ => false
    }
}

// The copy keeps its own name, under the folder this pass was given. The bool says whether this
// call is what put it there, so that a caller can undo it when the step it was taken for fails.
fn archive(data_dir: &str, archived_under: &str, relative: &str, contents: &[u8]) -> Result<bool, std::io::Error> {
    let target = find_archived_path(data_dir, archived_under, relative);
    if let Some(parent) = std::path::Path::new(&target).parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::path::Path::new(&target).exists() {
        return Ok(false);
    }
    std::fs::write(&target, contents)?;

    Ok(true)
}

fn find_archived_path(data_dir: &str, archived_under: &str, relative: &str) -> String {
    format!("{data_dir}{REPLACED_DIR_NAME}/{VERSION_ID}/{archived_under}/{relative}")
}

// Not just "is it there": a shipped file emptied by a crash or a sync client satisfies its name and
// is no longer a language definition, and every run then reports it as faulty
fn holds_something(path: &str) -> bool {
    std::fs::metadata(path).map(|x| x.len() > 0).unwrap_or(false)
}

// Named after the moment the pass ran, which sorts as it reads and holds no character a path
// refuses. Two passes inside one second would otherwise share a folder.
fn find_free_archive_folder(data_dir: &str) -> String {
    let taken = |name: &str|
            std::path::Path::new(&format!("{data_dir}{REPLACED_DIR_NAME}/{VERSION_ID}/{name}")).exists();

    let moment = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
    if !taken(&moment) {
        return moment;
    }
    (2..u32::MAX).map(|attempt| format!("{moment}-{attempt}"))
            .find(|name| !taken(name))
            .unwrap_or(moment)
}

fn read_baked_in_default_config_contents() -> String {
    String::from_utf8_lossy(include_bytes!("../data/config/default.txt")).to_string()
}

fn read_baked_in_extension_priority_contents() -> String {
    String::from_utf8_lossy(mezura_core::languages::get_shipped_extension_priority_raw()).to_string()
}

#[cfg(test)]
mod tests {
    use mezura_core::Language;
    use crate::config_manager::VERSION_ID;
    use crate::migration::{MANIFEST_FILE_NAME, content_hash, merge_priority_files,
            migrate_data_files, read_manifest, write_manifest};
    use crate::paths::test_paths::SCRATCH_DIR;

    #[test]
    fn a_migration_replaces_what_was_changed_and_keeps_it_and_is_silent_about_the_rest() {
        let dir = SCRATCH_DIR.to_owned() + "migration-test/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let first = migrate_data_files(&dir, false);
        assert!(first.failed.is_empty(), "a first installation into an empty directory failed: {:?}", first.failed);
        assert!(!first.added.is_empty() && first.replaced.is_empty() && first.updated.is_empty());
        assert!(first.restored.is_empty() && first.format_restored().is_none()
                && first.format_replaced().is_none(),
                "a first installation, which lost nothing, spoke about missing files: {:?}", first.restored);
        assert!(std::path::Path::new(&(dir.clone() + "installed.txt")).exists());

        assert!(migrate_data_files(&dir, false).did_nothing());

        let lua = dir.clone() + "languages/Lua.txt";
        let shipped = std::fs::read_to_string(&lua).unwrap();

        // Spelled differently and meaning the same, so it is corrected without a word
        std::fs::write(&lua, shipped.replace("\" '", "\"     '")).unwrap();
        let cosmetic = migrate_data_files(&dir, true);
        assert!(cosmetic.replaced.is_empty(), "a difference that changes no count was reported");
        assert_eq!(shipped, std::fs::read_to_string(&lua).unwrap());

        // A symbol removed is a different language, so their copy is kept and named
        std::fs::write(&lua, shipped.replace("\" '", "\"")).unwrap();
        let edited = migrate_data_files(&dir, true);
        assert_eq!(vec!["languages/Lua.txt".to_owned()], edited.replaced);
        assert_eq!(shipped, std::fs::read_to_string(&lua).unwrap());
        assert!(std::fs::read_to_string(format!("{dir}replaced/{}/{}/languages/Lua.txt",
                VERSION_ID, edited.archived_under)).unwrap().contains("\""));

        // A theme is taste and a language file is numbers, so an expanded theme keeps what it holds
        let theme = dir.clone() + "themes/Dracula.txt";
        let mine = std::fs::read_to_string(&theme).unwrap() + "\nheading = #ff0000";
        std::fs::write(&theme, &mine).unwrap();
        assert!(migrate_data_files(&dir, true).replaced.is_empty());
        assert_eq!(mine, std::fs::read_to_string(&theme).unwrap());

        // A file of their own is never ours to touch, whatever happens around it
        let theirs = dir.clone() + "languages/Mine.txt";
        std::fs::write(&theirs, "not a language file at all").unwrap();
        std::fs::remove_file(dir.clone() + "languages/Zig.txt").unwrap();

        let third = migrate_data_files(&dir, true);
        assert_eq!(vec!["languages/Zig.txt".to_owned()], third.restored);
        assert!(third.replaced.is_empty() && third.added.is_empty());
        assert_eq!("not a language file at all", std::fs::read_to_string(&theirs).unwrap());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // A first run counts from the copies inside the program and every later one from the directory,
    // so the two have to say the same thing or one tree gives two answers
    #[test]
    fn a_migrated_directory_holds_exactly_the_languages_the_program_carries() {
        let dir = SCRATCH_DIR.to_owned() + "migrated-languages/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        migrate_data_files(&dir, false);

        let (from_disk, faulty) = mezura_core::language_file::parse_languages_in_dir(
                &(dir.clone() + "languages/")).unwrap();
        assert!(faulty.is_empty(), "the migration wrote language files that do not parse: {faulty:?}");

        let by_name = |mut languages: Vec<Language>| {
            languages.sort_by(|one, other| one.name.cmp(&other.name));
            languages
        };
        assert_eq!(by_name(mezura_core::languages::parse_shipped_languages()), by_name(from_disk));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_file_that_exists_in_order_to_be_edited_is_never_replaced() {
        let dir = SCRATCH_DIR.to_owned() + "migration-carve-out/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        migrate_data_files(&dir, false);

        let config = dir.clone() + "config/default.txt";
        std::fs::write(&config, "settings of my own").unwrap();

        // Only the rule under the marker: the same words appear in the explanation as an example,
        // and the explanation is ours and does come back
        let priority = dir.clone() + mezura_core::EXTENSION_PRIORITY_FILE_NAME;
        let reordered = std::fs::read_to_string(&priority).unwrap()
                .replace("\nm       Objective-C, MATLAB", "\nm       MATLAB, Objective-C");
        std::fs::write(&priority, &reordered).unwrap();

        let outcome = migrate_data_files(&dir, true);
        assert!(outcome.replaced.is_empty() && outcome.restored.is_empty() && outcome.added.is_empty());
        assert!(outcome.merged.is_empty(), "a copy already holding every rule was rewritten");
        assert_eq!("settings of my own", std::fs::read_to_string(&config).unwrap());
        assert_eq!(reordered, std::fs::read_to_string(&priority).unwrap());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // An installation made before the 'contested-filenames' block existed has no such block, so
    // nothing in their own copy says the rules it holds can be written at all.
    #[test]
    fn the_priority_file_gains_what_this_version_adds_and_keeps_every_decision() {
        let dir = SCRATCH_DIR.to_owned() + "migration-priority-merge/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        migrate_data_files(&dir, false);

        // A copy written before the second block and the explanation above it existed, holding one
        // answer of their own and one rule about a contest we never had
        let path = dir.clone() + mezura_core::EXTENSION_PRIORITY_FILE_NAME;
        let theirs = "How this file worked back then.\n\n===> contested-extensions\nm       MATLAB, Objective-C\nlpr     Lazarus, Pascal\n";
        std::fs::write(&path, theirs).unwrap();
        // and the record that version left, holding the hash of some other text under this name.
        // That record is the only thing that can say a release changed this file.
        let mut recorded = read_manifest(&dir);
        recorded.insert(mezura_core::EXTENSION_PRIORITY_FILE_NAME.to_owned(), 1);
        write_manifest(&dir, &recorded).unwrap();

        let outcome = migrate_data_files(&dir, false);
        assert_eq!(vec![mezura_core::EXTENSION_PRIORITY_FILE_NAME.to_owned()], outcome.merged);
        assert!(outcome.format_merged().is_some(), "the file was rewritten without a word");
        assert!(outcome.replaced.is_empty(), "the merge reported itself as a replacement too");

        let merged = std::fs::read_to_string(&path).unwrap();
        assert!(merged.contains("m       MATLAB, Objective-C"), "their answer was lost:\n{merged}");
        assert!(merged.contains("lpr     Lazarus, Pascal"), "a rule of their own was lost:\n{merged}");
        assert!(merged.contains("===> contested-filenames"), "the block that could not be discovered \
                is still not there:\n{merged}");
        // and the parser agrees that the contest is settled once, their way
        let (rules, faulty) = mezura_core::language_file::parse_priority(&merged);
        assert!(faulty.is_empty(), "the merged file does not parse cleanly: {faulty:?}");
        assert_eq!(Some(&vec!["MATLAB".to_owned(), "Objective-C".to_owned()]), rules.by_extension.get("m"));
        assert_eq!(Some(&vec!["C Header".to_owned(), "Objective-C".to_owned()]), rules.by_extension.get("h"));

        // Their copy as it was is kept: the merge is the one place this pass rewrites a file that
        // was theirs to edit
        assert_eq!(theirs, std::fs::read_to_string(format!("{dir}replaced/{VERSION_ID}/{}/{}",
                outcome.archived_under, mezura_core::EXTENSION_PRIORITY_FILE_NAME)).unwrap());

        assert!(migrate_data_files(&dir, false).did_nothing(),
                "a merged file was merged again, so every run would say so");
        assert!(migrate_data_files(&dir, true).merged.is_empty(),
                "asked for by hand, it merged a file that already held everything");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // The one branch that removes a file. Only the manifest tells it apart from a file of their own.
    #[test]
    fn a_file_we_no_longer_ship_is_moved_out_and_one_of_their_own_is_left_alone() {
        let dir = SCRATCH_DIR.to_owned() + "migration-withdrawn/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        migrate_data_files(&dir, false);

        let withdrawn = dir.clone() + "languages/Gone.txt";
        let theirs = dir.clone() + "languages/Mine.txt";
        std::fs::write(&withdrawn, "a language of an earlier version").unwrap();
        std::fs::write(&theirs, "a language of my own").unwrap();
        let manifest = dir.clone() + "installed.txt";
        let recorded = std::fs::read_to_string(&manifest).unwrap();
        std::fs::write(&manifest, recorded + "languages/Gone.txt 1\n").unwrap();

        let outcome = migrate_data_files(&dir, true);
        assert_eq!(vec!["languages/Gone.txt".to_owned()], outcome.withdrawn);
        assert!(!std::path::Path::new(&withdrawn).exists());
        assert_eq!("a language of an earlier version", std::fs::read_to_string(
                format!("{dir}replaced/{VERSION_ID}/{}/languages/Gone.txt", outcome.archived_under)).unwrap());
        assert_eq!("a language of my own", std::fs::read_to_string(&theirs).unwrap());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_second_restore_after_a_second_edit_keeps_both_edits_under_their_own_folders() {
        let dir = SCRATCH_DIR.to_owned() + "migration-twice/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        migrate_data_files(&dir, false);
        let read_back = |folder: &str, name: &str|
                std::fs::read_to_string(format!("{dir}replaced/{VERSION_ID}/{folder}/languages/{name}"));

        let mine = dir.clone() + "languages/Rust.txt";
        std::fs::write(&mine, "my first edit").unwrap();
        let first = migrate_data_files(&dir, true);
        assert_eq!(vec!["languages/Rust.txt".to_owned()], first.replaced);

        // Both passes run inside the same second, which the folder name has to survive without
        // waiting for the clock
        std::fs::write(&mine, "my second edit").unwrap();
        let second = migrate_data_files(&dir, true);
        assert_eq!(vec!["languages/Rust.txt".to_owned()], second.replaced, "the copy kept its own name");
        assert_ne!(first.archived_under, second.archived_under, "two passes shared one folder");

        assert_eq!("my first edit", read_back(&first.archived_under, "Rust.txt").unwrap());
        assert_eq!("my second edit", read_back(&second.archived_under, "Rust.txt").unwrap());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // A file shipped under one spelling and now under another is one file on Windows and macOS, so
    // the new name is written over it and the old name then reads that same file.
    #[test]
    fn a_shipped_file_renamed_only_in_its_case_is_not_withdrawn_after_being_written() {
        let dir = SCRATCH_DIR.to_owned() + "migration-recased/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        migrate_data_files(&dir, false);

        // What an earlier version would have recorded had it shipped the name in another case
        let manifest = dir.clone() + "installed.txt";
        let recorded = std::fs::read_to_string(&manifest).unwrap();
        std::fs::write(&manifest, recorded + "languages/RUST.txt 1\n").unwrap();

        let outcome = migrate_data_files(&dir, true);
        assert!(std::path::Path::new(&(dir.clone() + "languages/Rust.txt")).exists(),
                "the language file was written and then deleted through its other spelling: {:?}", outcome.withdrawn);
        assert!(outcome.withdrawn.is_empty(), "{:?}", outcome.withdrawn);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // Nothing else catches this: the run falls back to the copies baked into the binary and counts
    // correctly, so the only symptom is a data directory that can no longer be edited.
    #[test]
    fn an_installation_that_lost_its_files_is_repaired_even_though_the_binary_has_not_moved() {
        let dir = SCRATCH_DIR.to_owned() + "migration-emptied/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        migrate_data_files(&dir, false);
        let a_language = dir.clone() + "languages/Rust.txt";
        assert!(std::path::Path::new(&a_language).exists(), "the first pass wrote nothing");

        for entry in std::fs::read_dir(dir.clone() + "languages/").unwrap().flatten() {
            std::fs::remove_file(entry.path()).unwrap();
        }

        // Same binary, same manifest, and the languages gone
        let outcome = migrate_data_files(&dir, false);
        assert!(std::path::Path::new(&a_language).exists(),
                "an emptied languages folder was left empty, and the run would count from the binary in silence");
        assert!(!outcome.restored.is_empty() && outcome.replaced.is_empty() && outcome.added.is_empty(),
                "the files came back as somebody's changed copies rather than as missing ones: {:?}", outcome.replaced);
        assert!(outcome.format_restored().is_some(), "an installation was repaired without a word");

        assert!(migrate_data_files(&dir, false).did_nothing());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // One file left behind answers "the folder is not empty" for a whole installation that is
    // missing sixty-six others
    #[test]
    fn an_installation_missing_one_file_of_many_is_repaired_too() {
        let dir = SCRATCH_DIR.to_owned() + "migration-partly-emptied/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        migrate_data_files(&dir, false);

        let (kept, lost) = (dir.clone() + "languages/Rust.txt", dir.clone() + "languages/Java.txt");
        std::fs::remove_file(&lost).unwrap();
        let outcome = migrate_data_files(&dir, false);
        assert!(std::path::Path::new(&lost).exists(), "one language file of many was left missing");
        assert!(std::path::Path::new(&kept).exists());
        assert!(outcome.replaced.is_empty(), "a missing file came back as a changed one: {:?}", outcome.replaced);

        // and so is the priority file, whose loss sends every contested extension to the tiebreak
        let priority = dir.clone() + mezura_core::EXTENSION_PRIORITY_FILE_NAME;
        std::fs::remove_file(&priority).unwrap();
        migrate_data_files(&dir, false);
        assert!(std::path::Path::new(&priority).exists(),
                "'{}' was left missing, so every contested extension falls to the tiebreak for good",
                mezura_core::EXTENSION_PRIORITY_FILE_NAME);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // The one directory with nothing shipped inside it, so no file of the completeness check stands
    // for it, and a run with '--log' has nowhere to write while it is gone
    #[test]
    fn a_deleted_logs_folder_is_made_again_and_said_out_loud() {
        let dir = SCRATCH_DIR.to_owned() + "migration-logs/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        migrate_data_files(&dir, false);

        let logs = dir.clone() + "logs";
        std::fs::remove_dir_all(&logs).unwrap();
        let outcome = migrate_data_files(&dir, false);
        assert!(std::path::Path::new(&logs).exists(), "the logs folder was left deleted");
        assert_eq!(vec!["logs/".to_owned()], outcome.restored);
        assert!(outcome.format_restored().is_some(), "the folder came back without a word");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // A language file the manifest has never seen arrives through the same branch as one somebody
    // deleted, and calling it missing sends them looking for whatever took it away
    #[test]
    fn a_language_this_version_brings_is_not_reported_as_one_that_went_missing() {
        let dir = SCRATCH_DIR.to_owned() + "migration-new-language/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        migrate_data_files(&dir, false);

        // as the manifest of a version that did not ship Zig yet
        let manifest = dir.clone() + "installed.txt";
        let recorded = std::fs::read_to_string(&manifest).unwrap();
        std::fs::write(&manifest, recorded.lines().filter(|line| !line.contains("Zig.txt"))
                .collect::<Vec<_>>().join("\n")).unwrap();
        std::fs::remove_file(dir.clone() + "languages/Zig.txt").unwrap();

        let outcome = migrate_data_files(&dir, false);
        assert_eq!(vec!["languages/Zig.txt".to_owned()], outcome.added);
        assert!(outcome.restored.is_empty() && outcome.format_restored().is_none(),
                "a language that never existed here was reported as missing: {:?}", outcome.restored);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // 'themes' is made a file because creating a directory over one fails on every system.
    #[test]
    fn what_cannot_be_written_costs_itself_and_not_the_rest_of_the_directory() {
        let dir = SCRATCH_DIR.to_owned() + "migration-one-bad/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.clone() + "themes", "not a directory").unwrap();

        let outcome = migrate_data_files(&dir, false);

        assert!(!outcome.failed.is_empty(), "creating a directory over a file succeeded");
        assert!(outcome.failed.iter().all(|x| x.starts_with("themes")),
                "something other than the themes failed: {:?}", outcome.failed);
        assert!(outcome.every_language_file_is_in_place(), "a theme that could not be written blamed the languages");
        assert!(outcome.format_failures().is_some(), "the failure was not reported");

        // Everything the pass had left to do after the directory it could not make
        assert!(std::path::Path::new(&(dir.clone() + "config/default.txt")).exists(), "the pass stopped at the themes");
        assert!(std::path::Path::new(&(dir.clone() + MANIFEST_FILE_NAME)).exists(), "the manifest was never written");
        // Against what the binary carries and not against today's number, or adding a language
        // breaks a test about a directory that could not be written
        assert_eq!(mezura_core::languages::get_shipped_language_files_raw().len(),
                std::fs::read_dir(dir.clone() + "languages").unwrap().count());

        std::fs::remove_file(dir.clone() + "themes").unwrap();
        let second = migrate_data_files(&dir, false);
        assert!(second.failed.is_empty(), "{:?}", second.failed);
        assert!(std::path::Path::new(&(dir.clone() + "themes/Dracula.txt")).is_file(),
                "the themes were never written on the second pass");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // The case every upgrade is made of: a file we shipped, that they never touched, whose contents
    // this version corrects. Overwritten without asking, and not without a word.
    #[test]
    fn a_file_of_ours_brought_up_to_date_is_not_brought_up_to_date_in_silence() {
        let dir = SCRATCH_DIR.to_owned() + "migration-updated/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        migrate_data_files(&dir, false);

        // What an older version's copy looks like: the contents differ from what we ship, and the
        // manifest records exactly what is on disk
        let lua = dir.clone() + "languages/Lua.txt";
        let older = std::fs::read_to_string(&lua).unwrap().replace("Lua", "Lua ");
        std::fs::write(&lua, &older).unwrap();
        let mut recorded = read_manifest(&dir);
        recorded.insert("languages/Lua.txt".to_owned(), content_hash(older.as_bytes()));
        write_manifest(&dir, &recorded).unwrap();

        // Not forced, because the case to see is a build whose language files changed while the
        // version string did not
        let outcome = migrate_data_files(&dir, false);

        assert_eq!(vec!["languages/Lua.txt".to_owned()], outcome.updated);
        assert!(outcome.replaced.is_empty(), "a file of ours was treated as one of theirs: {:?}", outcome.replaced);
        assert!(outcome.format_updated().is_some(), "the file was brought up to date in silence");
        assert!(!std::fs::read_to_string(&lua).unwrap().contains("Lua "), "the file was not brought up to date");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // A file moved aside by a pass that then dies has to be announced, or the retry finds it
    // matching what we ship, says nothing either, and the copy sits in 'replaced' with nothing
    // pointing at it. The manifest is made a directory because writing over one fails everywhere,
    // and it is the last step, so everything before it has already been done.
    #[test]
    fn a_pass_that_fails_still_says_what_it_moved_aside() {
        let dir = SCRATCH_DIR.to_owned() + "migration-failed/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        migrate_data_files(&dir, false);

        let lua = dir.clone() + "languages/Lua.txt";
        std::fs::write(&lua, "a language of my own under a name of ours").unwrap();
        std::fs::remove_file(dir.clone() + "installed.txt").unwrap();
        std::fs::create_dir(dir.clone() + "installed.txt").unwrap();

        let outcome = migrate_data_files(&dir, true);
        assert_eq!(1, outcome.failed.len(), "writing the manifest over a directory succeeded");
        assert!(outcome.failed[0].starts_with(MANIFEST_FILE_NAME), "the failure does not name the file: {:?}",
                outcome.failed);
        assert!(outcome.every_language_file_is_in_place(), "a manifest that could not be written blamed the languages");
        assert_eq!(vec!["languages/Lua.txt".to_owned()], outcome.replaced);
        assert!(outcome.format_replaced().is_some(), "the file was moved aside in silence");
        assert_eq!("a language of my own under a name of ours", std::fs::read_to_string(
                format!("{dir}replaced/{VERSION_ID}/{}/languages/Lua.txt", outcome.archived_under)).unwrap());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // An installation whose manifest predates the code reading it, which no other test here can
    // produce, since they all start from an empty directory. A file that moved from the managed set
    // to the one written and left alone keeps its entry, and that is not one we stopped shipping.
    #[test]
    fn a_file_that_stopped_being_managed_is_not_a_file_that_stopped_being_shipped() {
        let dir = SCRATCH_DIR.to_owned() + "migration-recategorised/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        migrate_data_files(&dir, false);

        // as an earlier version of the code recorded them, before they were left alone
        let manifest = dir.clone() + "installed.txt";
        let recorded = std::fs::read_to_string(&manifest).unwrap();
        std::fs::write(&manifest, recorded + "themes/Dracula.txt 1\nconfig/default.txt 2\n").unwrap();

        let outcome = migrate_data_files(&dir, true);
        assert!(outcome.withdrawn.is_empty(), "still shipped, and taken away: {:?}", outcome.withdrawn);
        assert!(std::path::Path::new(&(dir.clone() + "themes/Dracula.txt")).exists());
        assert!(std::path::Path::new(&(dir.clone() + "config/default.txt")).exists());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // The trigger is whether the record describes the files this binary carries, and the version
    // string it also holds decides nothing: between two releases the files change and the string
    // does not.
    #[test]
    fn a_manifest_that_does_not_describe_this_binary_makes_the_pass_run() {
        let dir = SCRATCH_DIR.to_owned() + "migration-manifest/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        migrate_data_files(&dir, false);
        assert!(migrate_data_files(&dir, false).did_nothing());

        let manifest = dir.clone() + "installed.txt";
        let whole = std::fs::read_to_string(&manifest).unwrap();
        let a_language = |text: &str| text.lines().find(|line| line.contains("Zig.txt"))
                .expect("the manifest records no language files").to_owned();

        // A version that is not this one changes nothing by itself: the files it recorded are the
        // ones this binary carries, so there is nothing to bring up to date
        for version in ["v99.0.0", "v0.0.1", "not a version at all"] {
            std::fs::write(&manifest, whole.replace(VERSION_ID, version)).unwrap();
            assert!(migrate_data_files(&dir, false).did_nothing(),
                    "a manifest recording '{version}' made the pass work although every file was in place");
        }

        // A record that does not describe what this binary carries makes it run, whichever way it
        // differs
        for (broken, why) in [(String::new(), "an unreadable manifest"),
                (whole.replace(&a_language(&whole), "languages/Zig.txt 1"), "a file we ship, recorded as another"),
                (whole.lines().filter(|line| !line.contains("Zig.txt")).collect::<Vec<_>>().join("\n"),
                        "a file this binary brings and the record never saw"),
                (whole.clone() + "languages/Gone.txt 1\n", "a file recorded that we no longer ship")] {
            std::fs::write(&manifest, &broken).unwrap();
            std::fs::remove_file(dir.clone() + "languages/Zig.txt").unwrap();
            assert!(!migrate_data_files(&dir, false).did_nothing(), "{why} did not make the pass run");
            assert!(std::path::Path::new(&(dir.clone() + "languages/Zig.txt")).exists());
        }

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // Dropping the record leaves the file installed for good: a language a release took away keeps
    // being loaded, and '--restore' cannot help, since nothing tells it from a language of the
    // user's own any more. A directory in the way is how a removal is made to fail on every system.
    #[test]
    fn a_withdrawal_that_could_not_finish_is_tried_again_by_the_next_run() {
        let dir = SCRATCH_DIR.to_owned() + "migration-withdrawal-failed/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        migrate_data_files(&dir, false);

        let withdrawn = dir.clone() + "languages/Gone.txt";
        std::fs::create_dir(&withdrawn).unwrap();
        std::fs::write(withdrawn.clone() + "/inside.txt", "not going anywhere").unwrap();
        let manifest = dir.clone() + "installed.txt";
        let recorded = std::fs::read_to_string(&manifest).unwrap();
        std::fs::write(&manifest, recorded + "languages/Gone.txt 1\n").unwrap();

        let outcome = migrate_data_files(&dir, true);
        assert!(outcome.withdrawn.is_empty(), "a removal that failed was reported as done");
        assert!(!outcome.failed.is_empty(), "the removal failed and said nothing");
        assert!(outcome.every_language_file_is_in_place(),
                "a file being taken away put the language files in doubt, so the run would count \
                 from the binary over one file it wanted to delete");
        assert_eq!(Some(&1), read_manifest(&dir).get("languages/Gone.txt"),
                "the record was dropped, so nothing will ever try to withdraw it again");

        // and once the way is clear it goes, which is what the record was kept for
        std::fs::remove_dir_all(&withdrawn).unwrap();
        std::fs::write(&withdrawn, "a language of an earlier version").unwrap();
        assert_eq!(vec!["languages/Gone.txt".to_owned()], migrate_data_files(&dir, true).withdrawn);
        assert!(!read_manifest(&dir).contains_key("languages/Gone.txt"));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_replacement_whose_write_fails_is_not_reported_and_leaves_no_copy() {
        let dir = SCRATCH_DIR.to_owned() + "migration-write-failed/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        migrate_data_files(&dir, false);

        // A directory where the language file was: it cannot be written over on any system
        let lua = dir.clone() + "languages/Lua.txt";
        std::fs::remove_file(&lua).unwrap();
        std::fs::create_dir(&lua).unwrap();
        std::fs::write(lua.clone() + "/inside.txt", "in the way").unwrap();

        let outcome = migrate_data_files(&dir, true);
        assert!(outcome.replaced.is_empty(), "a file that was never written was reported as replaced");
        assert!(outcome.format_replaced().is_none());
        assert!(!outcome.failed.is_empty(), "the write failed and said nothing");
        assert!(!std::path::Path::new(&format!("{dir}replaced/{VERSION_ID}/{}/languages/Lua.txt",
                outcome.archived_under)).exists(), "a copy was kept of a file that never moved");

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // A shipped file that is there and empty satisfies its name and is no longer a language
    // definition, so every run reports it as faulty and names nothing that would put it right
    #[test]
    fn a_shipped_file_emptied_where_it_stands_is_written_again() {
        let dir = SCRATCH_DIR.to_owned() + "migration-emptied-file/";
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        migrate_data_files(&dir, false);

        let lua = dir.clone() + "languages/Lua.txt";
        std::fs::write(&lua, "").unwrap();
        assert!(!migrate_data_files(&dir, false).did_nothing(), "an emptied language file was left empty");
        assert!(!std::fs::read_to_string(&lua).unwrap().is_empty());

        // A plain file of the same name answers "it is there" forever, so the folder is never made
        std::fs::remove_dir_all(dir.clone() + "logs").unwrap();
        std::fs::write(dir.clone() + "logs", "not a directory").unwrap();
        let outcome = migrate_data_files(&dir, false);
        assert!(!outcome.failed.is_empty(), "a file sitting where the logs folder belongs was accepted as one");

        std::fs::remove_file(dir.clone() + "logs").unwrap();
        assert!(std::path::Path::new(&(dir.clone() + "logs")).is_dir()
                || migrate_data_files(&dir, false).failed.is_empty());

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // The shipped files are written with carriage returns, and an editor that saves one back
    // without them changes every line and no meaning
    #[test]
    fn the_hash_does_not_see_line_endings() {
        assert_eq!(content_hash(b"first\r\nsecond\r\n"), content_hash(b"first\nsecond\n"));
        assert_ne!(content_hash(b"first\nsecond\n"), content_hash(b"first\nthird\n"));
    }

    #[test]
    fn merging_the_priority_file_settles_each_contest_once() {
        let ours = "How it works now.\n\n===> contested-extensions\nh       C Header, Objective-C\nm       Objective-C, MATLAB\n\n===> contested-filenames\n";

        // A contest they settled is not settled again further down, whichever spelling they wrote
        // it in: a file with two rules for one extension has the second reported as faulty
        let merged = merge_priority_files("===> contested-extensions\n.M   MATLAB, Objective-C\n", ours).unwrap();
        assert_eq!(1, merged.matches("MATLAB").count(), "the contest was settled twice:\n{merged}");
        assert!(merged.contains(".M   MATLAB, Objective-C"));

        // Their copy already says all of it, so there is nothing to write and nothing to report
        assert_eq!(None, merge_priority_files(ours, ours));
        // and a copy saved with the line endings of the other system is not a difference
        assert_eq!(None, merge_priority_files(&ours.replace('\n', "\r\n"), ours));

        // A section of their own has to survive an older binary that knows nothing about it
        let merged = merge_priority_files("===> theirs-alone\nq   Something\n", ours).unwrap();
        assert!(merged.contains("===> theirs-alone") && merged.contains("q   Something"),
                "a block of their own was dropped:\n{merged}");

        // The explanation is ours and comes back as we write it, so a section that arrives is never
        // one that nothing explains
        let merged = merge_priority_files("How it worked back then.\n\n===> contested-extensions\nh       C Header, Objective-C\nm       Objective-C, MATLAB\n", ours).unwrap();
        assert!(merged.starts_with("How it works now."), "the explanation was left behind:\n{merged}");
    }

    // The merge reads the file to rewrite it and the parser reads it to count. Where the two
    // disagree about where a section begins, the merge takes a section for one of their own, writes
    // ours beside it, and the parser lets ours win the contest they had settled.
    #[test]
    fn merging_the_priority_file_finds_a_section_wherever_the_parser_finds_one() {
        let ours = "How it works now.\n\n===> contested-extensions\nh       C Header, Objective-C\nm       Objective-C, MATLAB\n\n===> contested-filenames\n";
        let settled_once = |theirs: &str, why: &str| {
            let merged = merge_priority_files(theirs, ours)
                    .unwrap_or_else(|| panic!("{why}: their copy was called up to date"));
            let (rules, faulty) = mezura_core::language_file::parse_priority(&merged);
            assert!(faulty.is_empty(), "{why}: the merged file has lines the parser rejects: {faulty:?}\n{merged}");
            assert_eq!(Some(&vec!["MATLAB".to_owned(), "Objective-C".to_owned()]), rules.by_extension.get("m"),
                    "{why}: their answer was reversed\n{merged}");
        };

        // A mark in front of the first marker, which is what PowerShell and an older Notepad write
        // when the file is re-saved
        settled_once("\u{feff}===> contested-extensions\nm       MATLAB, Objective-C\n", "a byte order mark");
        // The parser reads the first word after the arrow, and neither of these changes it
        settled_once("===>contested-extensions\nm       MATLAB, Objective-C\n", "a marker with no space");
        settled_once("===> contested-extensions   (mine)\nm       MATLAB, Objective-C\n", "a marker with a word after it");

        // A second section under the same name, which is what appending to the file looks like
        let merged = merge_priority_files(
                "===> contested-extensions\nm       MATLAB, Objective-C\n\n===> contested-extensions\nlpr     Lazarus, Pascal\n", ours).unwrap();
        assert!(merged.contains("lpr     Lazarus, Pascal"), "the second section was dropped:\n{merged}");
        assert_eq!(1, merged.matches("MATLAB").count(), "the contest was settled twice:\n{merged}");
    }
}
