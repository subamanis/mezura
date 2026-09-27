// The declarations are read beside the extent walk and one row per file is kept, so the graph is
// walked once the counting is over and which thread counted the declaring file and which the
// declared one changes nothing.
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::hash::{BuildHasherDefault, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{FileEntry, LineClasses, Stats, TestCode};
use crate::engine::file_parser::TestReport;
use crate::engine::modules::ModuleId;
use crate::engine::targets::normalise_separators;
use crate::engine::test_detection::{attribute, find_word_end, is_word_byte, is_word_start};

// The files whose declarations resolve beside them. Any other file's resolve under a folder of its
// own name, then beside it for a crate root.
const ROOT_STEMS : [&str; 3] = ["lib", "main", "mod"];
const MODULE_FILE_STEM : &str = "mod";
const PUB : &[u8] = b"pub";
const RAW_IDENTIFIER : &[u8] = b"r#";
// A declaration under it names a file anywhere, so it is not followed at all, since the default
// place may hold a file another declaration names
const PATH_ATTRIBUTE : &[u8] = b"path";
const COMMENT_STARTS : [&[u8]; 3] = [b"//", b"/*", b"*"];
const MOST_HOPS : usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Declaration {
    pub name: Box<str>,
    // Outermost first
    pub inline_modules: Vec<Box<str>>,
    pub seed: bool,
}

pub(crate) struct ModuleDeclarations<'a> {
    keyword: &'a [u8],
    contents: &'a [u8],
    found: Vec<Declaration>,
    open: Vec<(Box<str>, u32)>,
    depth: u32,
}

impl<'a> ModuleDeclarations<'a> {
    pub(crate) fn of(keyword: &'a str, contents: &'a str) -> Self {
        ModuleDeclarations { keyword: keyword.as_bytes(), contents: contents.as_bytes(), found: Vec::new(),
                open: Vec::new(), depth: 0 }
    }

    // The ranges are offsets into the line past its lead, the whitespace before its first byte. A
    // declaration starts with the keyword, a visibility or an attribute, so outside an inline
    // module a line whose first code byte is none of those costs one comparison, inlined into the
    // line loop, and the reading below stays out of it.
    #[inline(always)]
    pub(crate) fn observe_line(&mut self, line_start: usize, raw_line: &str, lead: usize, ranges: &[(usize, usize)],
            is_test: bool)
    {
        let Some(&(first, _)) = ranges.first() else { return };
        let line = &raw_line.as_bytes()[lead..];
        if self.open.is_empty() && !matches!(line[first], b'm' | b'p' | b'#') { return; }
        self.read_line(line_start, line, ranges, is_test);
    }

    pub(crate) fn into_declarations(self) -> Vec<Declaration> {
        self.found
    }

    fn read_line(&mut self, line_start: usize, line: &[u8], ranges: &[(usize, usize)], is_test: bool) {
        let opened = self.read_declaration(line_start, line, ranges, is_test);
        if self.open.is_empty() && !opened { return; }
        for &(from, to) in ranges {
            for at in memchr::memchr2_iter(b'{', b'}', &line[from..to]) {
                if line[from + at] == b'{' {
                    self.depth += 1;
                    continue;
                }
                self.depth = self.depth.saturating_sub(1);
                while self.open.last().is_some_and(|(_, opened_at)| *opened_at >= self.depth) {
                    self.open.pop();
                }
            }
        }
    }

    // The attributes are read over the whole line, since a string inside one is no code range, and
    // what follows them has to sit inside one
    fn read_declaration(&mut self, line_start: usize, line: &[u8], ranges: &[(usize, usize)], is_test: bool) -> bool {
        let Some((mut at, names_a_path)) = skip_attributes(line, ranges[0].0) else { return false };
        let Some(&(_, to)) = ranges.iter().find(|(from, to)| *from <= at && at < *to) else { return false };
        let code = &line[..to];
        if code[at..].starts_with(PUB) && !code.get(at + PUB.len()).is_some_and(|byte| is_word_byte(*byte)) {
            at = skip_whitespace(code, at + PUB.len());
            if code.get(at) == Some(&b'(') {
                let Some(close) = find_closing(code, at, b'(', b')') else { return false };
                at = skip_whitespace(code, close + 1);
            }
        }
        if !code[at..].starts_with(self.keyword) { return false; }
        at += self.keyword.len();
        if !code.get(at).is_some_and(u8::is_ascii_whitespace) { return false; }
        at = skip_whitespace(code, at);
        if code[at..].starts_with(RAW_IDENTIFIER) { at += RAW_IDENTIFIER.len(); }
        if !code.get(at).is_some_and(|byte| is_word_start(*byte)) { return false; }
        let end = find_word_end(code, at);
        let Ok(name) = std::str::from_utf8(&code[at..end]) else { return false };
        match code.get(skip_whitespace(code, end)) {
            Some(b';') => {
                if !names_a_path && !self.is_under_a_path_attribute(line_start) {
                    self.found.push(Declaration { name: name.into(),
                            inline_modules: self.open.iter().map(|(name, _)| name.clone()).collect(), seed: is_test });
                }
                false
            },
            Some(b'{') => {
                self.open.push((name.into(), self.depth));
                true
            },
            _ => false
        }
    }

    // An attribute spanning lines is not walked through, so a path above one of those goes unseen
    fn is_under_a_path_attribute(&self, line_start: usize) -> bool {
        let mut end = line_start;
        while end > 0 {
            let line_end = end - 1;
            let start = memchr::memrchr(b'\n', &self.contents[..line_end]).map_or(0, |at| at + 1);
            let line = self.contents[start..line_end].trim_ascii();
            if line.is_empty() || COMMENT_STARTS.iter().any(|comment| line.starts_with(comment)) {
                end = start;
                continue;
            }
            if line.starts_with(b"#[") || line.starts_with(b"#![") {
                let name_start = skip_whitespace(line, 2 + usize::from(line[1] == b'!'));
                if &line[name_start..find_word_end(line, name_start)] == PATH_ATTRIBUTE { return true; }
                end = start;
                continue;
            }
            return false;
        }
        false
    }
}

pub(crate) struct ModuleRow {
    pub path: PathBuf,
    pub module: ModuleId,
    pub language_name: Arc<str>,
    pub lines: usize,
    pub classes: LineClasses,
    pub bytes: usize,
    pub partial: Option<TestReport>,
    pub declarations: Vec<Declaration>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Promotion {
    pub seeds: usize,
    pub promoted: usize,
}

// The rows are borrowed, so the caller frees them where their ten thousand paths are off the wall
pub(crate) fn promote_declared_test_modules(rows: &[ModuleRow], tests_by_module: &mut [HashMap<String, TestCode>],
        files_by_module: &mut [HashMap<String, Vec<FileEntry>>]) -> Promotion
{
    let seeds = rows.iter().flat_map(|row| &row.declarations).filter(|declaration| declaration.seed).count();
    if seeds == 0 {
        return Promotion::default();
    }
    // Keyed on the bytes of the path, which every path of one run and every candidate for it
    // share, since the same walk joined them
    let by_path = rows.iter().enumerate().map(|(at, row)| (row.path.as_os_str(), at)).collect::<PathIndex<'_>>();
    let mut marked = vec![false; rows.len()];
    let mut pending = Vec::new();
    for row in rows {
        for declaration in row.declarations.iter().filter(|declaration| declaration.seed) {
            pending.extend(find_declared_row(&row.path, declaration, &by_path));
        }
    }
    // A row already marked ends the walk there, which is what ends a cycle
    while let Some(at) = pending.pop() {
        if marked[at] { continue; }
        marked[at] = true;
        for declaration in &rows[at].declarations {
            pending.extend(find_declared_row(&rows[at].path, declaration, &by_path));
        }
    }

    let mut promoted = 0;
    let mut entries_by_path: HashMap<(ModuleId, Arc<str>), HashMap<String, usize>> = HashMap::new();
    for row in rows.iter().zip(&marked).filter_map(|(row, marked)| marked.then_some(row)) {
        let share = tests_by_module[row.module as usize].entry(row.language_name.to_string()).or_default();
        if !share.promote_to_whole_file(row.lines, &row.classes, row.bytes, row.partial.as_ref()) { continue; }
        promoted += 1;
        let Some(entries) = files_by_module[row.module as usize].get_mut(&*row.language_name) else { continue };
        let index = entries_by_path.entry((row.module, row.language_name.clone())).or_insert_with(|| entries.iter()
                .enumerate().map(|(at, entry)| (entry.path.clone(), at)).collect());
        if let Some(&at) = index.get(&spell_out(&row.path)) {
            entries[at].tests = Some(Stats::new(1, row.bytes, row.lines, row.classes.clone(), HashMap::new()));
        }
    }
    Promotion { seeds, promoted }
}

pub(crate) fn find_declaring_chain(file: &Path, read: &mut dyn FnMut(&Path) -> Option<Vec<Declaration>>) -> Option<Vec<PathBuf>> {
    let mut chain = Vec::new();
    let mut visited = HashSet::from([file.to_path_buf()]);
    let mut current = file.to_path_buf();
    for _ in 0..MOST_HOPS {
        let (declarer, seed) = find_declarer_of(&current, read, &visited)?;
        chain.push(declarer.clone());
        if seed { return Some(chain); }
        visited.insert(current);
        current = declarer;
    }
    None
}

fn find_declared_file_candidates(declaring: &Path, declaration: &Declaration) -> Vec<PathBuf> {
    let (Some(dir), Some(stem)) = (declaring.parent(), declaring.file_stem().and_then(|stem| stem.to_str())) else {
        return Vec::new();
    };
    let extension = format_extension_of(declaring);
    let own = if ROOT_STEMS.contains(&stem) { dir.to_path_buf() } else { dir.join(stem) };
    let mut places = vec![own.clone()];
    if own != dir { places.push(dir.to_path_buf()); }
    let mut candidates = Vec::with_capacity(places.len() * 2);
    for mut place in places {
        for inline in &declaration.inline_modules { place.push(&**inline); }
        candidates.push(place.join(format!("{}{extension}", declaration.name)));
        candidates.push(place.join(&*declaration.name).join(format!("{MODULE_FILE_STEM}{extension}")));
    }
    candidates
}

type PathIndex<'a> = HashMap<&'a OsStr, usize, BuildHasherDefault<BytesHasher>>;

// Eight bytes at a time, since the keys are ten thousand paths of one run and the default hasher
// takes a millisecond over them
#[derive(Default)]
struct BytesHasher(u64);

impl Hasher for BytesHasher {
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.0 = (self.0.rotate_left(5) ^ u64::from_le_bytes(word)).wrapping_mul(0x517c_c1b7_2722_0a95);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

fn find_declared_row(declaring: &Path, declaration: &Declaration, by_path: &PathIndex<'_>) -> Option<usize> {
    find_declared_file_candidates(declaring, declaration).iter().find_map(|candidate| by_path.get(candidate.as_os_str()).copied())
}

// The file's own folder answers for a crate root beside it too, so every sibling is read there
fn find_declarer_of(file: &Path, read: &mut dyn FnMut(&Path) -> Option<Vec<Declaration>>, visited: &HashSet<PathBuf>)
        -> Option<(PathBuf, bool)>
{
    let extension = format_extension_of(file);
    let stem = file.file_stem()?.to_str()?;
    let (name, mut holder) = match stem == MODULE_FILE_STEM {
        true => (file.parent()?.file_name()?.to_str()?.to_owned(), file.parent()?.parent()?.to_path_buf()),
        false => (stem.to_owned(), file.parent()?.to_path_buf())
    };
    let mut inline: Vec<Box<str>> = Vec::new();
    loop {
        let mut candidates = ROOT_STEMS.iter().map(|root| holder.join(format!("{root}{extension}"))).collect::<Vec<_>>();
        if let (Some(parent), Some(holder_name)) = (holder.parent(), holder.file_name()) {
            candidates.push(parent.join(format!("{}{extension}", holder_name.to_string_lossy())));
        }
        if inline.is_empty() && let Ok(listing) = std::fs::read_dir(&holder) {
            let siblings = listing.flatten().map(|entry| entry.path())
                    .filter(|path| path.extension() == file.extension() && !candidates.contains(path)).collect::<Vec<_>>();
            candidates.extend(siblings);
        }
        for candidate in candidates {
            if candidate == file || visited.contains(&candidate) { continue; }
            if let Some(declarations) = read(&candidate)
                    && let Some(found) = declarations.iter().find(|declaration| *declaration.name == *name
                            && declaration.inline_modules == inline) {
                return Some((candidate, found.seed));
            }
        }
        let holder_name = holder.file_name()?.to_str()?.to_owned();
        inline.insert(0, holder_name.into());
        holder = holder.parent()?.to_path_buf();
    }
}

fn format_extension_of(file: &Path) -> String {
    file.extension().map(|extension| format!(".{}", extension.to_string_lossy())).unwrap_or_default()
}

fn spell_out(path: &Path) -> String {
    normalise_separators(&path.to_string_lossy()).into_owned()
}

// Read as Rust's attribute, so a ']' inside a string of one closes nothing
fn skip_attributes(line: &[u8], from: usize) -> Option<(usize, bool)> {
    let mut at = skip_whitespace(line, from);
    let mut names_a_path = false;
    while line.get(at) == Some(&b'#') {
        let open = at + 1 + usize::from(line.get(at + 1) == Some(&b'!'));
        if line.get(open) != Some(&b'[') { return None; }
        let name_start = skip_whitespace(line, open + 1);
        names_a_path |= &line[name_start..find_word_end(line, name_start)] == PATH_ATTRIBUTE;
        at = skip_whitespace(line, attribute::find_matching_bracket(line, open, b']')? + 1);
    }
    Some((at, names_a_path))
}

fn find_closing(code: &[u8], open: usize, opener: u8, closer: u8) -> Option<usize> {
    let mut depth = 0usize;
    for (at, byte) in code.iter().enumerate().skip(open) {
        if *byte == opener {
            depth += 1;
        } else if *byte == closer {
            depth -= 1;
            if depth == 0 { return Some(at); }
        }
    }
    None
}

fn skip_whitespace(code: &[u8], from: usize) -> usize {
    let mut at = from;
    while code.get(at).is_some_and(u8::is_ascii_whitespace) { at += 1; }
    at
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::FileStats;

    type TestLine<'a> = (&'a str, Option<&'a [(usize, usize)]>, bool);

    fn read_with_ranges(lines: &[TestLine<'_>]) -> Vec<Declaration> {
        let contents = lines.iter().map(|(line, _, _)| *line).collect::<Vec<_>>().join("\n");
        let mut reader = ModuleDeclarations::of("mod", &contents);
        let mut line_start = 0;
        for (line, ranges, is_test) in lines {
            let lead = line.len() - line.trim_ascii_start().len();
            let whole = [(0, line.trim_ascii().len())];
            if !line.trim_ascii().is_empty() {
                reader.observe_line(line_start, line, lead, ranges.unwrap_or(&whole), *is_test);
            }
            line_start += line.len() + 1;
        }
        reader.into_declarations()
    }

    fn read(lines: &[(&str, bool)]) -> Vec<Declaration> {
        read_with_ranges(&lines.iter().map(|(line, is_test)| (*line, None, *is_test)).collect::<Vec<_>>())
    }

    fn declared(name: &str, inline_modules: &[&str], seed: bool) -> Declaration {
        Declaration { name: name.into(), inline_modules: inline_modules.iter().map(|name| (*name).into()).collect(), seed }
    }

    #[test]
    fn a_declaration_is_read_after_attributes_and_a_visibility_and_a_seed_is_one_on_a_test_line() {
        let found = read(&[("mod plain;", false), ("pub mod shown;", false), ("pub(crate) mod inner;", false),
                ("pub(in crate::a) mod deep;", false), ("#[cfg(unix)] mod unix;", false),
                ("#[cfg(test)] mod checks;", true), ("mod tests;", true), ("mod r#type;", false),
                ("mod  spaced ;", false), ("mod x; // a comment stays outside the range", false)]);
        assert_eq!(vec![declared("plain", &[], false), declared("shown", &[], false), declared("inner", &[], false),
                declared("deep", &[], false), declared("unix", &[], false), declared("checks", &[], true),
                declared("tests", &[], true), declared("type", &[], false), declared("spaced", &[], false),
                declared("x", &[], false)], found);
    }

    #[test]
    fn what_is_no_declaration_is_left_alone() {
        assert!(read(&[("module x;", false), ("mod x", false), ("let mod_x = 1;", false), ("modx;", false),
                ("mod 1;", false), ("use mod_a::b;", false), ("fn mod() {}", false), ("pub mod;", false),
                ("#[cfg(test) mod x;", false), ("mod x {", false), ("}", false), ("mod x { }", false)]).is_empty());
    }

    #[test]
    fn a_declaration_under_a_path_attribute_is_not_followed() {
        let found = read(&[("#[path = \"x.rs\"] mod on_the_line;", true), ("#[path = \"x.rs\"]", false),
                ("mod below_it;", true), ("#[cfg(test)]", true), ("#[path = \"x.rs\"]", true), ("mod under_both;", true),
                ("#[ path = \"x.rs\" ]", false), ("#[cfg(test)]", true), ("mod under_both_reversed;", true),
                ("#[path = \"x.rs\"]", false), ("// a note", false), ("", false), ("/* another */", false),
                ("mod under_comments;", false),
                ("#[path = \"x.rs\"]", false), ("struct Between;", false), ("mod after_an_item;", false),
                ("#[pathological]", false), ("mod under_a_longer_word;", false), ("mod plain;", false)]);
        assert_eq!(vec![declared("after_an_item", &[], false), declared("under_a_longer_word", &[], false),
                declared("plain", &[], false)], found);

        // As the parser hands the lines over, with the string of the attribute outside the code ranges
        let path_line = "#[path = \"elsewhere.rs\"]";
        let doc_line = "#[doc = \"]\"] mod documented;";
        let found = read_with_ranges(&[(path_line, Some(&[(0, 9), (path_line.len() - 1, path_line.len())]), true),
                ("mod aside;", None, true), (doc_line, Some(&[(0, 8), (11, doc_line.len())]), false),
                ("#[cfg(test)] // mod inside_a_comment;", Some(&[(0, 12)]), true), ("mod after_a_comment;", None, true)]);
        assert_eq!(vec![declared("documented", &[], false), declared("after_a_comment", &[], true)], found);
    }

    #[test]
    fn an_indented_declaration_is_read_through_its_lead() {
        let found = read(&[("    mod indented;", false), ("\tpub mod tabbed;", false), ("  #[cfg(unix)] mod attributed;", false)]);
        assert_eq!(vec![declared("indented", &[], false), declared("tabbed", &[], false), declared("attributed", &[], false)], found);
    }

    #[test]
    fn a_declaration_inside_an_inline_module_carries_it_and_a_macro_block_adds_nothing() {
        let found = read(&[("mod outer {", false), ("    fn f() { if a { } }", false), ("    mod a;", false),
                ("    mod inner {", false), ("        mod b;", false), ("    }", false), ("    cfg_if! {", false),
                ("        mod c;", false), ("    }", false), ("}", false), ("mod after;", false),
                ("#[cfg(test)]", true), ("mod tests {", true), ("    mod cases;", true), ("}", true), ("mod last;", false)]);
        assert_eq!(vec![declared("a", &["outer"], false), declared("b", &["outer", "inner"], false),
                declared("c", &["outer"], false), declared("after", &[], false), declared("cases", &["tests"], true),
                declared("last", &[], false)], found);
    }

    #[test]
    fn braces_outside_the_code_ranges_are_not_counted() {
        let with_a_string = "let s = \"}\"; mod a;";
        let found = read_with_ranges(&[("mod outer {", None, false),
                (with_a_string, Some(&[(0, 8), (11, with_a_string.len())]), false), ("mod b;", None, false),
                ("} // }", Some(&[(0, 1)]), false), ("mod c;", None, false)]);
        assert_eq!(vec![declared("b", &["outer"], false), declared("c", &[], false)], found);
    }

    #[test]
    fn a_declaration_resolves_beside_a_root_file_and_under_the_name_of_any_other() {
        let of = |declaring: &str, name: &str, inline: &[&str]| find_declared_file_candidates(Path::new(declaring),
                &declared(name, inline, false)).iter().map(|path| spell_out(path)).collect::<Vec<_>>();
        assert_eq!(vec!["src/tests.rs", "src/tests/mod.rs"], of("src/lib.rs", "tests", &[]));
        assert_eq!(vec!["src/tests.rs", "src/tests/mod.rs"], of("src/main.rs", "tests", &[]));
        assert_eq!(vec!["src/a/b.rs", "src/a/b/mod.rs"], of("src/a/mod.rs", "b", &[]));
        assert_eq!(vec!["src/a/b.rs", "src/a/b/mod.rs", "src/b.rs", "src/b/mod.rs"], of("src/a.rs", "b", &[]));
        assert_eq!(vec!["src/bin/tool/x.rs", "src/bin/tool/x/mod.rs", "src/bin/x.rs", "src/bin/x/mod.rs"],
                of("src/bin/tool.rs", "x", &[]));
        assert_eq!(vec!["src/outer/inner/x.rs", "src/outer/inner/x/mod.rs"], of("src/lib.rs", "x", &["outer", "inner"]));
        assert_eq!(vec!["src/a/outer/x.rs", "src/a/outer/x/mod.rs", "src/outer/x.rs", "src/outer/x/mod.rs"],
                of("src/a.rs", "x", &["outer"]));
        assert!(of("lib.rs", "x", &[]).is_empty() || of("lib.rs", "x", &[]) == vec!["x.rs", "x/mod.rs"]);
    }

    // Joined the way the walk joins, so the bytes of a path and of a candidate for it agree
    fn joined(path: &str) -> PathBuf {
        path.split('/').fold(PathBuf::new(), |joined, component| joined.join(component))
    }

    fn row(path: &str, module: ModuleId, lines: usize, partial_lines: Option<usize>, declarations: Vec<Declaration>) -> ModuleRow {
        let classes_of = |lines| LineClasses { words_in_code: lines, ..LineClasses::default() };
        ModuleRow { path: joined(path), module, language_name: "Rust".into(), lines, classes: classes_of(lines),
                bytes: lines * 10, declarations,
                partial: partial_lines.map(|partial| TestReport { bytes: partial * 10,
                        stats: FileStats { lines: partial, classes: classes_of(partial), keyword_occurences: Vec::new() } }) }
    }

    #[test]
    fn the_files_a_seed_reaches_are_booked_whole_and_the_walk_follows_every_declaration() {
        let rows = vec![
            row("p/src/lib.rs", 0, 50, Some(3), vec![declared("tests", &[], true), declared("plain", &[], false),
                    declared("nested", &[], true)]),
            row("p/src/plain.rs", 0, 20, None, vec![]),
            row("p/src/tests.rs", 0, 30, Some(10), vec![declared("all", &[], false), declared("missing", &[], false)]),
            row("p/src/tests/all.rs", 0, 40, None, vec![declared("lib", &[], false)]),
            row("p/src/nested/deep.rs", 1, 12, None, vec![]),
            row("p/src/nested.rs", 0, 8, None, vec![declared("deep", &["inline"], false), declared("x", &[], false)]),
            row("p/src/nested/inline/deep.rs", 1, 6, None, vec![]),
            row("p/src/whole.rs", 0, 9, Some(9), vec![declared("helper", &[], true)]),
            row("p/src/whole/helper.rs", 0, 4, None, vec![])];
        let mut tests = vec![HashMap::new(), HashMap::new()];
        tests[0].insert("Rust".to_owned(), TestCode { stats: Stats::new(3, 220, 22, LineClasses { words_in_code: 22,
                ..LineClasses::default() }, HashMap::new()), whole_files: 1 });
        let mut files = vec![hashmap!("Rust".to_owned() => ["p/src/lib.rs", "p/src/tests.rs", "p/src/tests/all.rs",
                "p/src/whole/helper.rs"].map(|path| FileEntry { path: path.to_owned(), stats: Stats::default(),
                nested_languages: HashMap::new(), tests: None }).to_vec()), HashMap::new()];

        let promotion = promote_declared_test_modules(&rows, &mut tests, &mut files);
        assert_eq!(Promotion { seeds: 3, promoted: 5 }, promotion);
        let share = &tests[0]["Rust"];
        // tests.rs adds 20 over its 10, all.rs 40, nested.rs 8, helper.rs 4, and lib.rs and whole.rs stay as they were
        assert_eq!((6, 94, 940, 5), (share.stats.files, share.stats.lines, share.stats.bytes, share.whole_files));
        assert_eq!(94, share.stats.classes.words_in_code);
        assert_eq!((1, 6, 60, 1), { let share = &tests[1]["Rust"]; (share.stats.files, share.stats.lines, share.stats.bytes, share.whole_files) },
                "deep.rs is reached through the inline module of nested.rs and booked in the module of its own row");
        let entry_tests = |path: &str| files[0]["Rust"].iter().find(|entry| entry.path == path).unwrap().tests.as_ref()
                .map(|tests| (tests.files, tests.lines, tests.bytes));
        assert_eq!(None, entry_tests("p/src/lib.rs"));
        assert_eq!(Some((1, 30, 300)), entry_tests("p/src/tests.rs"));
        assert_eq!(Some((1, 40, 400)), entry_tests("p/src/tests/all.rs"));
        assert_eq!(Some((1, 4, 40)), entry_tests("p/src/whole/helper.rs"));
    }

    #[test]
    fn a_run_with_no_seed_books_nothing() {
        let rows = vec![row("p/src/lib.rs", 0, 50, None, vec![declared("plain", &[], false)]),
                row("p/src/plain.rs", 0, 20, None, vec![])];
        let mut tests = vec![HashMap::new()];
        let mut files = vec![HashMap::new()];
        assert_eq!(Promotion::default(), promote_declared_test_modules(&rows, &mut tests, &mut files));
        assert!(tests[0].is_empty());
    }

    #[test]
    fn the_declaring_chain_is_found_upward_through_folders_and_inline_modules() {
        let tree: HashMap<&str, Vec<Declaration>> = hashmap!(
            "p/src/lib.rs" => vec![declared("tests", &[], true), declared("plain", &[], false),
                    declared("deep", &["outer"], true), declared("a", &[], false)],
            "p/src/tests.rs" => vec![declared("all", &[], false)],
            "p/src/tests/all.rs" => vec![],
            "p/src/plain.rs" => vec![declared("more", &[], false)],
            "p/src/plain/more.rs" => vec![],
            "p/src/a.rs" => vec![declared("b", &[], false)],
            "p/src/a/b.rs" => vec![declared("a", &[], false)],
            "p/src/outer/deep.rs" => vec![]);
        let chain = |file: &str| find_declaring_chain(Path::new(file), &mut |path: &Path| tree.get(spell_out(path).as_str()).cloned())
                .map(|chain| chain.iter().map(|path| spell_out(path)).collect::<Vec<_>>());
        assert_eq!(Some(vec!["p/src/lib.rs".to_owned()]), chain("p/src/tests.rs"));
        assert_eq!(Some(vec!["p/src/tests.rs".to_owned(), "p/src/lib.rs".to_owned()]), chain("p/src/tests/all.rs"));
        assert_eq!(Some(vec!["p/src/lib.rs".to_owned()]), chain("p/src/outer/deep.rs"));
        assert_eq!(None, chain("p/src/plain/more.rs"), "a chain ending under no marker was taken");
        assert_eq!(None, chain("p/src/a/b.rs"), "a cycle did not end");
        assert_eq!(None, chain("p/src/lib.rs"));
    }
}
