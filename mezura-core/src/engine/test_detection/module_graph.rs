// The declarations are read beside the extent walk and one row per file is kept, so the graph is
// walked once the counting is over and which thread counted the declaring file and which the
// declared one changes nothing.
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{EngineConfig, FileEntry, LineClasses, Stats, TestCode};
use crate::engine::file_parser::{NestedLanguageLookup, TestReport, read_module_declarations};
use crate::engine::modules::ModuleId;
use crate::engine::targets::spell_out;
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
const EACH_BYTE : u64 = 0x0101_0101_0101_0101;
const HIGH_BITS : u64 = 0x8080_8080_8080_8080;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Declaration {
    pub name: Box<str>,
    // Outermost first
    pub inline_modules: Vec<Box<str>>,
    pub seed: bool,
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

pub(crate) fn promote_declared_test_modules(rows: &[ModuleRow], tests_by_module: &mut [HashMap<String, TestCode>],
        files_by_module: &mut [HashMap<String, Vec<FileEntry>>], source: &mut dyn DeclarationSource) -> Promotion
{
    let seeds = rows.iter().flat_map(|row| &row.declarations).filter(|declaration| declaration.seed).count();
    if seeds == 0 {
        return Promotion::default();
    }
    let by_path = rows.iter().enumerate().map(|(at, row)| (PathKey::of(&row.path), at)).collect::<PathIndex<'_>>();
    let mut marked = vec![false; rows.len()];
    let mut read_without_a_row = HashSet::new();
    let mut pending = Vec::new();
    for row in rows {
        for declaration in row.declarations.iter().filter(|declaration| declaration.seed) {
            pending.extend(find_declared_file(&row.path, &row.language_name, declaration, false, &by_path, source));
        }
    }
    // A file already marked ends the walk there, so one that two seeds reach is booked once
    while let Some(found) = pending.pop() {
        match found {
            Found::Row(at) => {
                if marked[at] { continue; }
                marked[at] = true;
                for declaration in &rows[at].declarations {
                    pending.extend(find_declared_file(&rows[at].path, &rows[at].language_name, declaration, true, &by_path, source));
                }
            },
            Found::WithoutARow(path, language_name, declarations) => {
                if !read_without_a_row.insert(path.clone()) { continue; }
                for declaration in &declarations {
                    pending.extend(find_declared_file(&path, &language_name, declaration, true, &by_path, source));
                }
            }
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

pub(crate) fn find_declaring_chain(file: &Path, language_name: &str, source: &mut dyn DeclarationSource) -> Option<Vec<PathBuf>> {
    let mut visited = HashSet::from([file.to_path_buf()]);
    let mut chain = Vec::new();
    climb_to_a_marker(file, language_name, source, &mut visited, &mut chain).then_some(chain)
}

pub(crate) trait DeclarationSource {
    fn is_file(&mut self, path: &Path) -> bool;
    fn read_declarations(&mut self, path: &Path, language_name: &str) -> Option<Vec<Declaration>>;
    fn list_files(&mut self, folder: &Path) -> Vec<PathBuf>;
}

pub(crate) struct FilesOnDisk<'a> {
    pub lookup: &'a NestedLanguageLookup<'a>,
    pub config: &'a EngineConfig,
    pub is_whole: &'a dyn Fn(&Path) -> bool,
}

impl DeclarationSource for FilesOnDisk<'_> {
    fn is_file(&mut self, path: &Path) -> bool {
        path.is_file()
    }

    fn read_declarations(&mut self, path: &Path, language_name: &str) -> Option<Vec<Declaration>> {
        let contents = std::fs::read_to_string(path).ok()?;
        let language = self.lookup.languages.get(language_name)?;
        let mut declarations = read_module_declarations(&contents, language, self.lookup, self.config)?;
        if (self.is_whole)(path) {
            declarations.iter_mut().for_each(|declaration| declaration.seed = false);
        }
        Some(declarations)
    }

    fn list_files(&mut self, folder: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(folder).map(|listing| listing.flatten().map(|entry| entry.path()).collect()).unwrap_or_default()
    }
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

    // An attribute spanning lines ends the look-back, so a path above one of those goes unseen and
    // the declaration is followed to the default place
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
            let Some((after, names_a_path)) = skip_attributes(line, 0) else { return false };
            let carries_an_item = after == 0
                    || (after < line.len() && !COMMENT_STARTS.iter().any(|comment| line[after..].starts_with(comment)));
            if carries_an_item { return false; }
            if names_a_path { return true; }
            end = start;
        }
        false
    }
}

fn build_declared_file_candidates(declaring: &Path, declaration: &Declaration) -> Vec<PathBuf> {
    let (Some(dir), Some(stem)) = (declaring.parent(), declaring.file_stem().and_then(|stem| stem.to_str())) else {
        return Vec::new();
    };
    let extension = build_dotted_extension_of(declaring);
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

type PathIndex<'a> = HashMap<PathKey<'a>, usize, BuildHasherDefault<BytesHasher>>;

// A file named on the command line keeps its forward slashes while the directory scan joins with
// the platform's, so the separators are folded into one in the hash and in the comparison
#[derive(Clone, Copy)]
struct PathKey<'a>(&'a [u8]);

impl<'a> PathKey<'a> {
    fn of(path: &'a Path) -> Self {
        PathKey(path.as_os_str().as_encoded_bytes())
    }
}

impl PartialEq for PathKey<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
                && self.0.iter().zip(other.0).all(|(mine, theirs)| fold_separator(*mine) == fold_separator(*theirs))
    }
}

impl Eq for PathKey<'_> {}

impl Hash for PathKey<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for chunk in self.0.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            state.write_u64(fold_separators(u64::from_le_bytes(word)));
        }
    }
}

// Whole words at a time, since the default hasher costs more than the rest of the promotion
#[derive(Default)]
struct BytesHasher(u64);

impl Hasher for BytesHasher {
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.write_u64(u64::from_le_bytes(word));
        }
    }

    fn write_u64(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

fn fold_separator(byte: u8) -> u8 {
    if cfg!(windows) && byte == b'\\' { b'/' } else { byte }
}

// Every byte equal to a backslash becomes a slash, the eight of a word at once
fn fold_separators(word: u64) -> u64 {
    if !cfg!(windows) { return word; }
    let differences = word ^ (u64::from(b'\\') * EACH_BYTE);
    let non_zero = ((differences & !HIGH_BITS) + !HIGH_BITS) | differences;
    let backslashes = (!non_zero & HIGH_BITS) >> 7;
    word ^ (backslashes * u64::from(b'\\' ^ b'/'))
}

enum Found {
    Row(usize),
    WithoutARow(PathBuf, Arc<str>, Vec<Declaration>),
}

// A file reached through a declaration is a module and no crate root, so only the pair under its
// own folder is tried. A file on disk with no row is the one named, read and booked nothing.
fn find_declared_file(declaring: &Path, language_name: &Arc<str>, declaration: &Declaration, reached: bool,
        by_path: &PathIndex<'_>, source: &mut dyn DeclarationSource) -> Option<Found>
{
    let candidates = build_declared_file_candidates(declaring, declaration);
    let pairs = if reached { &candidates[..candidates.len().min(2)] } else { &candidates[..] };
    for pair in pairs.chunks(2) {
        if let Some(at) = pair.iter().find_map(|candidate| by_path.get(&PathKey::of(candidate)).copied()) {
            return Some(Found::Row(at));
        }
        if let Some(candidate) = pair.iter().find(|candidate| source.is_file(candidate)) {
            let declarations = source.read_declarations(candidate, language_name).unwrap_or_default();
            return Some(Found::WithoutARow(candidate.clone(), language_name.clone(), declarations));
        }
    }
    None
}

fn climb_to_a_marker(file: &Path, language_name: &str, source: &mut dyn DeclarationSource, visited: &mut HashSet<PathBuf>,
        chain: &mut Vec<PathBuf>) -> bool
{
    if chain.len() == MOST_HOPS { return false; }
    for (declarer, seed) in find_declarers_of(file, language_name, source, visited).unwrap_or_default() {
        chain.push(declarer.clone());
        if seed { return true; }
        visited.insert(declarer.clone());
        if climb_to_a_marker(&declarer, language_name, source, visited, chain) { return true; }
        chain.pop();
    }
    false
}

// The file's own folder answers for a crate root beside it too, so the siblings are read at every level
fn find_declarers_of(file: &Path, language_name: &str, source: &mut dyn DeclarationSource, visited: &HashSet<PathBuf>)
        -> Option<Vec<(PathBuf, bool)>>
{
    let extension = build_dotted_extension_of(file);
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
        let mut siblings = source.list_files(&holder).into_iter()
                .filter(|path| path.extension() == file.extension() && !candidates.contains(path)).collect::<Vec<_>>();
        siblings.sort();
        candidates.extend(siblings);
        let mut declarers = Vec::new();
        for candidate in candidates {
            if candidate == file || visited.contains(&candidate) { continue; }
            let Some(declarations) = source.read_declarations(&candidate, language_name) else { continue };
            for declaration in declarations.iter().filter(|declaration| *declaration.name == *name && declaration.inline_modules == inline) {
                let places = build_declared_file_candidates(&candidate, declaration);
                if places.iter().find(|place| source.is_file(place)).is_some_and(|place| place.as_path() == file) {
                    declarers.push((candidate.clone(), declaration.seed));
                }
            }
        }
        if !declarers.is_empty() {
            declarers.sort_by_key(|(_, seed)| !seed);
            return Some(declarers);
        }
        let holder_name = holder.file_name()?.to_str()?.to_owned();
        inline.insert(0, holder_name.into());
        holder = holder.parent()?.to_path_buf();
    }
}

fn build_dotted_extension_of(file: &Path) -> String {
    file.extension().map(|extension| format!(".{}", extension.to_string_lossy())).unwrap_or_default()
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

    fn build_declaration(name: &str, inline_modules: &[&str], seed: bool) -> Declaration {
        Declaration { name: name.into(), inline_modules: inline_modules.iter().map(|name| (*name).into()).collect(), seed }
    }

    #[test]
    fn a_declaration_is_read_after_attributes_and_a_visibility_and_a_seed_is_one_on_a_test_line() {
        let found = read(&[("mod plain;", false), ("pub mod shown;", false), ("pub(crate) mod inner;", false),
                ("pub(in crate::a) mod deep;", false), ("#[cfg(unix)] mod unix;", false),
                ("#[cfg(test)] mod checks;", true), ("mod tests;", true), ("mod r#type;", false),
                ("mod  spaced ;", false), ("mod x; // a comment stays outside the range", false)]);
        assert_eq!(vec![build_declaration("plain", &[], false), build_declaration("shown", &[], false), build_declaration("inner", &[], false),
                build_declaration("deep", &[], false), build_declaration("unix", &[], false), build_declaration("checks", &[], true),
                build_declaration("tests", &[], true), build_declaration("type", &[], false), build_declaration("spaced", &[], false),
                build_declaration("x", &[], false)], found);
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
                ("#[pathological]", false), ("mod under_a_longer_word;", false),
                ("#[path = \"x.rs\"] mod on_the_line_above;", false), ("#[cfg(test)]", true), ("mod under_an_attribute_with_its_item;", true),
                ("#[allow(dead_code)] #[path = \"x.rs\"]", false), ("mod under_a_second_attribute;", false), ("mod plain;", false)]);
        assert_eq!(vec![build_declaration("after_an_item", &[], false), build_declaration("under_a_longer_word", &[], false),
                build_declaration("under_an_attribute_with_its_item", &[], true), build_declaration("plain", &[], false)], found);

        // As the parser hands the lines over, with the string of the attribute outside the code ranges
        let path_line = "#[path = \"elsewhere.rs\"]";
        let doc_line = "#[doc = \"]\"] mod documented;";
        let found = read_with_ranges(&[(path_line, Some(&[(0, 9), (path_line.len() - 1, path_line.len())]), true),
                ("mod aside;", None, true), (doc_line, Some(&[(0, 8), (11, doc_line.len())]), false),
                ("#[cfg(test)] // mod inside_a_comment;", Some(&[(0, 12)]), true), ("mod after_a_comment;", None, true)]);
        assert_eq!(vec![build_declaration("documented", &[], false), build_declaration("after_a_comment", &[], true)], found);
    }

    #[test]
    fn an_indented_declaration_is_read_through_its_lead() {
        let found = read(&[("    mod indented;", false), ("\tpub mod tabbed;", false), ("  #[cfg(unix)] mod attributed;", false)]);
        assert_eq!(vec![build_declaration("indented", &[], false), build_declaration("tabbed", &[], false), build_declaration("attributed", &[], false)], found);
    }

    #[test]
    fn a_declaration_inside_an_inline_module_carries_it_and_a_macro_block_adds_nothing() {
        let found = read(&[("mod outer {", false), ("    fn f() { if a { } }", false), ("    mod a;", false),
                ("    mod inner {", false), ("        mod b;", false), ("    }", false), ("    cfg_if! {", false),
                ("        mod c;", false), ("    }", false), ("}", false), ("mod after;", false),
                ("#[cfg(test)]", true), ("mod tests {", true), ("    mod cases;", true), ("}", true), ("mod last;", false)]);
        assert_eq!(vec![build_declaration("a", &["outer"], false), build_declaration("b", &["outer", "inner"], false),
                build_declaration("c", &["outer"], false), build_declaration("after", &[], false), build_declaration("cases", &["tests"], true),
                build_declaration("last", &[], false)], found);
    }

    #[test]
    fn braces_outside_the_code_ranges_are_not_counted() {
        let with_a_string = "let s = \"}\"; mod a;";
        let found = read_with_ranges(&[("mod outer {", None, false),
                (with_a_string, Some(&[(0, 8), (11, with_a_string.len())]), false), ("mod b;", None, false),
                ("} // }", Some(&[(0, 1)]), false), ("mod c;", None, false)]);
        assert_eq!(vec![build_declaration("b", &["outer"], false), build_declaration("c", &[], false)], found);
    }

    #[test]
    fn a_declaration_resolves_beside_a_root_file_and_under_the_name_of_any_other() {
        let of = |declaring: &str, name: &str, inline: &[&str]| build_declared_file_candidates(Path::new(declaring),
                &build_declaration(name, inline, false)).iter().map(|path| spell_out(path)).collect::<Vec<_>>();
        assert_eq!(vec!["src/tests.rs", "src/tests/mod.rs"], of("src/lib.rs", "tests", &[]));
        assert_eq!(vec!["src/tests.rs", "src/tests/mod.rs"], of("src/main.rs", "tests", &[]));
        assert_eq!(vec!["src/a/b.rs", "src/a/b/mod.rs"], of("src/a/mod.rs", "b", &[]));
        assert_eq!(vec!["src/a/b.rs", "src/a/b/mod.rs", "src/b.rs", "src/b/mod.rs"], of("src/a.rs", "b", &[]));
        assert_eq!(vec!["src/bin/tool/x.rs", "src/bin/tool/x/mod.rs", "src/bin/x.rs", "src/bin/x/mod.rs"],
                of("src/bin/tool.rs", "x", &[]));
        assert_eq!(vec!["src/outer/inner/x.rs", "src/outer/inner/x/mod.rs"], of("src/lib.rs", "x", &["outer", "inner"]));
        assert_eq!(vec!["src/a/outer/x.rs", "src/a/outer/x/mod.rs", "src/outer/x.rs", "src/outer/x/mod.rs"],
                of("src/a.rs", "x", &["outer"]));
        assert_eq!(vec!["x.rs", "x/mod.rs"], of("lib.rs", "x", &[]));
    }

    // Joined the way the directory scan joins
    fn join_as_the_scan_does(path: &str) -> PathBuf {
        path.split('/').fold(PathBuf::new(), |joined, component| joined.join(component))
    }

    fn build_row(path: &str, module: ModuleId, lines: usize, partial_lines: Option<usize>, declarations: Vec<Declaration>) -> ModuleRow {
        let classes_of = |lines| LineClasses { words_in_code: lines, ..LineClasses::default() };
        ModuleRow { path: join_as_the_scan_does(path), module, language_name: "Rust".into(), lines, classes: classes_of(lines),
                bytes: lines * 10, declarations,
                partial: partial_lines.map(|partial| TestReport { bytes: partial * 10,
                        stats: FileStats { lines: partial, classes: classes_of(partial), keyword_occurences: Vec::new() } }) }
    }

    #[derive(Default)]
    struct Tree(HashMap<&'static str, Vec<Declaration>>);

    impl DeclarationSource for Tree {
        fn is_file(&mut self, path: &Path) -> bool {
            self.0.contains_key(spell_out(path).as_str())
        }

        fn read_declarations(&mut self, path: &Path, _: &str) -> Option<Vec<Declaration>> {
            self.0.get(spell_out(path).as_str()).cloned()
        }

        fn list_files(&mut self, folder: &Path) -> Vec<PathBuf> {
            let folder = spell_out(folder);
            self.0.keys().filter(|path| Path::new(path).parent().is_some_and(|parent| spell_out(parent) == folder))
                    .map(PathBuf::from).collect()
        }
    }

    #[test]
    fn the_files_a_seed_reaches_are_booked_whole_and_the_walk_follows_every_declaration() {
        let rows = vec![
            build_row("p/src/lib.rs", 0, 50, Some(3), vec![build_declaration("tests", &[], true), build_declaration("plain", &[], false),
                    build_declaration("nested", &[], true)]),
            build_row("p/src/plain.rs", 0, 20, None, vec![]),
            build_row("p/src/tests.rs", 0, 30, Some(10), vec![build_declaration("all", &[], false), build_declaration("missing", &[], false)]),
            build_row("p/src/tests/all.rs", 0, 40, None, vec![build_declaration("lib", &[], false)]),
            build_row("p/src/nested/deep.rs", 1, 12, None, vec![]),
            build_row("p/src/nested.rs", 0, 8, None, vec![build_declaration("deep", &["inline"], false), build_declaration("x", &[], false)]),
            build_row("p/src/nested/inline/deep.rs", 1, 6, None, vec![]),
            build_row("p/src/whole.rs", 0, 9, Some(9), vec![build_declaration("helper", &[], true)]),
            build_row("p/src/whole/helper.rs", 0, 4, None, vec![])];
        let mut tests = vec![HashMap::new(), HashMap::new()];
        tests[0].insert("Rust".to_owned(), TestCode { stats: Stats::new(3, 220, 22, LineClasses { words_in_code: 22,
                ..LineClasses::default() }, HashMap::new()), whole_files: 1 });
        let mut files = vec![hashmap!("Rust".to_owned() => ["p/src/lib.rs", "p/src/tests.rs", "p/src/tests/all.rs",
                "p/src/whole/helper.rs"].map(|path| FileEntry { path: path.to_owned(), stats: Stats::default(),
                nested_languages: HashMap::new(), tests: None }).to_vec()), HashMap::new()];

        let promotion = promote_declared_test_modules(&rows, &mut tests, &mut files, &mut Tree::default());
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
    fn a_file_named_on_the_command_line_keeps_its_slashes_and_is_still_found() {
        let mut rows = vec![build_row("p/src/lib.rs", 0, 5, Some(2), vec![build_declaration("tests", &[], true)]),
                build_row("p/src/tests.rs", 0, 30, Some(10), vec![])];
        rows[1].path = PathBuf::from("p/src/tests.rs");
        let mut tests = vec![HashMap::new()];
        let mut files = vec![HashMap::new()];
        assert_eq!(Promotion { seeds: 1, promoted: 1 }, promote_declared_test_modules(&rows, &mut tests, &mut files, &mut Tree::default()));
    }

    #[test]
    fn a_reached_file_is_a_module_so_its_declarations_never_resolve_beside_it() {
        let rows = vec![build_row("p/src/bin/x.rs", 0, 3, Some(1), vec![build_declaration("y", &[], true)]),
                build_row("p/src/bin/y.rs", 0, 7, None, vec![build_declaration("x", &[], false)])];
        let mut tests = vec![HashMap::new()];
        let mut files = vec![HashMap::new()];
        assert_eq!(Promotion { seeds: 1, promoted: 1 }, promote_declared_test_modules(&rows, &mut tests, &mut files, &mut Tree::default()));
        let share = &tests[0]["Rust"];
        assert_eq!((1, 7, 1), (share.stats.files, share.stats.lines, share.whole_files));
    }

    #[test]
    fn a_file_two_seeds_reach_is_booked_once() {
        let rows = vec![build_row("p/src/bin/one.rs", 0, 3, Some(1), vec![build_declaration("shared", &[], true)]),
                build_row("p/src/bin/two.rs", 0, 3, Some(1), vec![build_declaration("shared", &[], true)]),
                build_row("p/src/bin/shared.rs", 0, 7, None, vec![])];
        let mut tests = vec![HashMap::new()];
        let mut files = vec![HashMap::new()];
        assert_eq!(Promotion { seeds: 2, promoted: 1 }, promote_declared_test_modules(&rows, &mut tests, &mut files, &mut Tree::default()));
        let share = &tests[0]["Rust"];
        assert_eq!((1, 7, 1), (share.stats.files, share.stats.lines, share.whole_files));
    }

    #[test]
    fn a_declared_file_the_count_left_out_is_read_for_what_it_declares_and_booked_nothing() {
        let rows = vec![build_row("p/src/lib.rs", 0, 5, Some(2), vec![build_declaration("tests", &[], true)]),
                build_row("p/src/tests/helpers.rs", 0, 3, None, vec![]),
                build_row("p/src/common.rs", 0, 30, None, vec![]),
                build_row("p/src/a.rs", 0, 5, Some(2), vec![build_declaration("helpers", &[], true)]),
                build_row("p/src/helpers.rs", 0, 40, None, vec![]),
                build_row("p/src/a/helpers/inner.rs", 0, 2, None, vec![])];
        let mut tree = Tree(hashmap!(
            "p/src/tests.rs" => vec![build_declaration("helpers", &[], false), build_declaration("common", &[], false)],
            "p/src/a/helpers.rs" => vec![build_declaration("inner", &[], false)]));
        let mut tests = vec![HashMap::new()];
        let mut files = vec![HashMap::new()];
        assert_eq!(Promotion { seeds: 2, promoted: 2 }, promote_declared_test_modules(&rows, &mut tests, &mut files, &mut tree));
        let share = &tests[0]["Rust"];
        assert_eq!((2, 5, 2), (share.stats.files, share.stats.lines, share.whole_files),
                "helpers.rs and inner.rs through the files without a row, and neither same-named file a folder up");
    }

    #[test]
    fn a_seed_naming_nothing_and_a_file_with_no_lines_move_nothing() {
        let rows = vec![build_row("p/src/lib.rs", 0, 5, Some(2), vec![build_declaration("absent", &[], true), build_declaration("empty", &[], true)]),
                build_row("p/src/empty.rs", 0, 0, None, vec![])];
        let mut tests = vec![HashMap::new()];
        let mut files = vec![HashMap::new()];
        assert_eq!(Promotion { seeds: 2, promoted: 0 }, promote_declared_test_modules(&rows, &mut tests, &mut files, &mut Tree::default()));
        let share = &tests[0]["Rust"];
        assert_eq!((0, 0, 0), (share.stats.files, share.stats.lines, share.whole_files));
    }

    #[test]
    fn a_run_with_no_seed_books_nothing() {
        let rows = vec![build_row("p/src/lib.rs", 0, 50, None, vec![build_declaration("plain", &[], false)]),
                build_row("p/src/plain.rs", 0, 20, None, vec![])];
        let mut tests = vec![HashMap::new()];
        let mut files = vec![HashMap::new()];
        assert_eq!(Promotion::default(), promote_declared_test_modules(&rows, &mut tests, &mut files, &mut Tree::default()));
        assert!(tests[0].is_empty());
    }

    #[test]
    fn the_declaring_chain_is_found_upward_through_folders_and_inline_modules() {
        let mut tree = Tree(hashmap!(
            "p/src/lib.rs" => vec![build_declaration("tests", &[], true), build_declaration("plain", &[], false),
                    build_declaration("deep", &["outer"], true), build_declaration("a", &[], false)],
            "p/src/tests.rs" => vec![build_declaration("all", &[], false)],
            "p/src/tests/all.rs" => vec![],
            "p/src/plain.rs" => vec![build_declaration("more", &[], false)],
            "p/src/plain/more.rs" => vec![],
            "p/src/a.rs" => vec![build_declaration("b", &[], false)],
            "p/src/a/b.rs" => vec![build_declaration("a", &[], false)],
            "p/src/outer/deep.rs" => vec![],
            "q/src/lib.rs" => vec![build_declaration("main", &[], true)],
            "q/src/main.rs" => vec![build_declaration("lib", &[], false)]));
        let mut chain = |file: &str| find_declaring_chain(Path::new(file), "Rust", &mut tree)
                .map(|chain| chain.iter().map(|path| spell_out(path)).collect::<Vec<_>>());
        assert_eq!(Some(vec!["p/src/lib.rs".to_owned()]), chain("p/src/tests.rs"));
        assert_eq!(Some(vec!["p/src/tests.rs".to_owned(), "p/src/lib.rs".to_owned()]), chain("p/src/tests/all.rs"));
        assert_eq!(Some(vec!["p/src/lib.rs".to_owned()]), chain("p/src/outer/deep.rs"));
        assert_eq!(None, chain("p/src/plain/more.rs"), "a chain ending under no marker was taken");
        assert_eq!(None, chain("p/src/a/b.rs"), "a chain reaching lib.rs outside a marker was taken");
        assert_eq!(None, chain("p/src/lib.rs"));
        assert_eq!(None, chain("q/src/lib.rs"), "two files declaring each other did not end the search");
    }

    #[test]
    fn the_declaring_chain_prefers_a_marker_and_takes_a_declarer_only_where_its_declaration_lands_here() {
        let mut tree = Tree(hashmap!(
            "r/src/bin/tool.rs" => vec![build_declaration("cases", &["checks"], true)],
            "r/src/bin/checks/cases.rs" => vec![],
            "r/src/bin/one.rs" => vec![build_declaration("shared", &[], false)],
            "r/src/bin/two.rs" => vec![build_declaration("shared", &[], true)],
            "r/src/bin/shared.rs" => vec![],
            "r/src/lib.rs" => vec![build_declaration("clock", &[], false), build_declaration("clock", &[], true),
                    build_declaration("a", &[], false)],
            "r/src/clock.rs" => vec![],
            "r/src/a.rs" => vec![build_declaration("x", &[], true)],
            "r/src/a/x.rs" => vec![],
            "r/src/x.rs" => vec![]));
        let mut chain = |file: &str| find_declaring_chain(Path::new(file), "Rust", &mut tree)
                .map(|chain| chain.iter().map(|path| spell_out(path)).collect::<Vec<_>>());
        assert_eq!(Some(vec!["r/src/bin/tool.rs".to_owned()]), chain("r/src/bin/checks/cases.rs"),
                "a crate root declaring through an inline module sits a level up, among the siblings there");
        assert_eq!(Some(vec!["r/src/bin/two.rs".to_owned()]), chain("r/src/bin/shared.rs"),
                "the declarer under a marker wins over the one sorting first");
        assert_eq!(Some(vec!["r/src/lib.rs".to_owned()]), chain("r/src/clock.rs"), "a module declared twice is a seed once");
        assert_eq!(Some(vec!["r/src/a.rs".to_owned()]), chain("r/src/a/x.rs"));
        assert_eq!(None, chain("r/src/x.rs"), "a declaration landing under the sibling's own folder was taken");
    }
}
