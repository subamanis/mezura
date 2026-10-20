// Rust's module declarations and its rules for where the file one names sits, reached through the
// MODULES keyword of a language file. The declarations are read beside the extent walk and one row
// per file is kept, so the graph is walked once the counting is over and which thread counted the
// declaring file and which the declared one changes nothing.
use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::{EngineConfig, FileEntry, Language, Stats, TestCode};
use crate::domain::FileStats;
use crate::engine::file_parser::{BYTE_ORDER_MARK, NestedLanguageLookup, TestReport, read_module_declarations};
use crate::engine::modules::ModuleId;
use crate::engine::targets::spell_out;
use super::attribute::RAW_IDENTIFIER;
use super::{attribute, build_byte_table, find_next_brace, find_word_end, is_word_byte, is_word_start, skip_whitespace};

// The files whose declarations resolve beside them. Any other file's resolve under a folder of its
// own name, then beside it for a crate root.
const ROOT_STEMS : [&str; 3] = ["lib", "main", "mod"];
const MODULE_FILE_STEM : &str = "mod";
const PUB : &[u8] = b"pub";
// A declaration under it names a file anywhere, so it is not followed at all, since the default
// place may hold a file another declaration names
const PATH_ATTRIBUTE : &[u8] = b"path";
const COMMENT_AND_ATTRIBUTE_STOPS : [bool; 256] = build_byte_table(b"/\"'#[]");
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
    pub stats: FileStats,
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
        files_by_module: &mut [HashMap<String, Vec<FileEntry>>], languages: &HashMap<String, Language>,
        source: &mut dyn DeclarationSource) -> Promotion
{
    let seeds = rows.iter().flat_map(|row| &row.declarations).filter(|declaration| declaration.seed).count();
    if seeds == 0 {
        return Promotion::default();
    }
    let marked = mark_rows_the_seeds_reach(rows, source);

    let mut promoted = 0;
    let mut entries_by_path: HashMap<(ModuleId, Arc<str>), HashMap<String, usize>> = HashMap::new();
    for row in rows.iter().zip(&marked).filter_map(|(row, marked)| marked.then_some(row)) {
        let language = &languages[&*row.language_name];
        let share = tests_by_module[row.module as usize].entry(row.language_name.to_string())
                .or_insert_with(|| TestCode::from(language));
        if !share.promote_to_whole_file(&row.stats, row.bytes, &language.keywords, row.partial.as_ref()) { continue; }
        promoted += 1;
        let Some(entries) = files_by_module[row.module as usize].get_mut(&*row.language_name) else { continue };
        let index = entries_by_path.entry((row.module, row.language_name.clone())).or_insert_with(|| entries.iter()
                .enumerate().map(|(at, entry)| (entry.path.clone(), at)).collect());
        if let Some(&at) = index.get(&spell_out(&row.path)) {
            entries[at].tests = Some(Stats::of_one_file(&row.stats, row.bytes));
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
    open: Vec<InlineModule>,
    depth: u32,
    keyword_at: Vec<usize>,
    next: usize,
    next_brace: usize,
    comments_and_attributes: CommentsAndAttributes,
}

impl<'a> ModuleDeclarations<'a> {
    pub(crate) fn of(keyword: &'a str, contents: &'a str) -> Self {
        let keyword_at = match keyword.is_empty() {
            true => Vec::new(),
            false => memchr::memmem::find_iter(contents.as_bytes(), keyword.as_bytes()).collect()
        };
        ModuleDeclarations { keyword: keyword.as_bytes(), contents: contents.as_bytes(), found: Vec::new(),
                open: Vec::new(), depth: 0, keyword_at, next: 0, next_brace: 0,
                comments_and_attributes: CommentsAndAttributes::default() }
    }

    #[inline(always)]
    pub(crate) fn quiet_until(&self) -> usize {
        if self.keyword.is_empty() { return 0; }
        let keyword = self.keyword_at.get(self.next).copied().unwrap_or(usize::MAX);
        match self.open.is_empty() {
            true => keyword,
            false => keyword.min(self.next_brace)
        }
    }

    // The ranges are offsets into the line past its lead, the whitespace before its first byte. A
    // declaration starts with the keyword, a visibility or an attribute, or follows the ']' of an
    // attribute opened on a line above. A line whose first code byte is none of those is searched
    // for such a ']', which when found has the lexer read the file up to the line's end, and a
    // declaration on it is looked for only after a brace or a semicolon.
    #[inline(always)]
    pub(crate) fn observe_line(&mut self, line_start: usize, raw_line: &str, lead: usize, ranges: &[(usize, usize)],
            test_spans: &[(usize, usize)])
    {
        let line_end = line_start + raw_line.len();
        let mut holds_the_keyword = self.keyword.is_empty();
        let mut last_keyword = 0;
        while let Some(&at) = self.keyword_at.get(self.next) && at < line_end {
            if at >= line_start {
                holds_the_keyword = true;
                last_keyword = at;
            }
            self.next += 1;
        }
        if self.open.is_empty() && !holds_the_keyword { return; }
        let base = line_start + lead;
        let line = &raw_line.as_bytes()[lead..];
        let Some(mut first) = find_first_code_byte(line, ranges, base == 0) else { return };
        let last_keyword = last_keyword.saturating_sub(base);
        if line.get(first) != Some(&b'#')
                && let Some(end) = self.find_end_of_the_attribute_holding(line, base, first, last_keyword) {
            first = attribute::skip_whitespace_and_comments(line, end);
        }
        self.read_line(base, line, ranges, test_spans, holds_the_keyword, first, last_keyword);
        if !self.open.is_empty() {
            self.next_brace = find_next_brace(self.contents, line_end);
        }
    }

    pub(crate) fn into_declarations(self) -> Vec<Declaration> {
        self.found
    }

    fn read_line(&mut self, base: usize, line: &[u8], ranges: &[(usize, usize)], test_spans: &[(usize, usize)],
            holds_the_keyword: bool, first: usize, last_keyword: usize) {
        let opened = holds_the_keyword && line.get(first).is_some_and(|byte| matches!(byte, b'm' | b'p' | b'#'))
                && self.read_declaration(line, ranges, test_spans, first, base);
        if last_keyword > first {
            self.read_later_declarations(line, ranges, test_spans, last_keyword, base);
            return;
        }
        if self.open.is_empty() && !opened { return; }
        for &(from, to) in ranges {
            for at in memchr::memchr2_iter(b'{', b'}', &line[from..to]) {
                if line[from + at] == b'{' {
                    self.depth += 1;
                    continue;
                }
                self.close_block();
            }
        }
    }

    fn read_later_declarations(&mut self, line: &[u8], ranges: &[(usize, usize)], test_spans: &[(usize, usize)],
            last_keyword: usize, base: usize) {
        let mut counting = !self.open.is_empty();
        for &(from, to) in ranges {
            for at in memchr::memchr3_iter(b'{', b'}', b';', &line[from..to]).map(|at| from + at) {
                match line[at] {
                    b'{' if counting => self.depth += 1,
                    b'}' if counting => self.close_block(),
                    _ => ()
                }
                if at < last_keyword
                        && self.read_declaration(line, ranges, test_spans, skip_whitespace(line, at + 1), base) {
                    counting = true;
                }
            }
        }
    }

    #[inline(always)]
    fn close_block(&mut self) {
        self.depth = self.depth.saturating_sub(1);
        while self.open.last().is_some_and(|module| module.opened_at >= self.depth) {
            self.open.pop();
        }
    }

    // The attributes are read over the whole line, since a string inside one is no code range, and
    // what follows them has to sit inside one. The seed is read where the attributes end, since the
    // test code opens at its marker.
    fn read_declaration(&mut self, line: &[u8], ranges: &[(usize, usize)], test_spans: &[(usize, usize)], first: usize,
            base: usize) -> bool {
        let Some((mut at, names_a_path)) = skip_attributes(line, first) else { return false };
        let Some(&(_, to)) = ranges[..ranges.partition_point(|&(from, _)| from <= at)].last().filter(|&&(_, to)| at < to)
                else { return false };
        let item_at = base + at;
        let mut code = &line[..to];
        if code[at..].starts_with(PUB) && !code.get(at + PUB.len()).is_some_and(|byte| is_word_byte(*byte)) {
            (at, code) = skip_to_code(line, ranges, at + PUB.len());
            if code.get(at) == Some(&b'(') {
                let Some(close) = find_visibility_close(line, ranges, at) else { return false };
                (at, code) = skip_to_code(line, ranges, close + 1);
            }
        }
        if !code[at..].starts_with(self.keyword) { return false; }
        at += self.keyword.len();
        if code.get(at).is_some_and(|byte| is_word_byte(*byte)) { return false; }
        (at, code) = skip_to_code(line, ranges, at);
        if code[at..].starts_with(RAW_IDENTIFIER) { at += RAW_IDENTIFIER.len(); }
        if !code.get(at).is_some_and(|byte| is_word_start(*byte)) { return false; }
        let end = find_word_end(code, at);
        let Ok(name) = std::str::from_utf8(&code[at..end]) else { return false };
        (at, code) = skip_to_code(line, ranges, end);
        // Only comments may sit between the name and its brace or semicolon, which can be lines below
        let next_at = match code.get(at) {
            Some(_) => base + at,
            None => attribute::skip_whitespace_and_comments(self.contents, base + end)
        };
        match self.contents.get(next_at) {
            Some(b';') => {
                if !names_a_path && !self.is_under_a_path_attribute(base + first)
                        && !self.is_inside_a_module_under_a_path_attribute() {
                    let seed = test_spans.iter().any(|&(from, to)| from <= item_at && item_at < to);
                    self.found.push(Declaration { name: name.into(),
                            inline_modules: self.open.iter().map(|module| module.name.clone()).collect(), seed });
                }
                false
            },
            Some(b'{') => {
                self.open.push(InlineModule { name: name.into(), opened_at: self.depth, names_a_path, looks_back_from: base + first,
                        body_from: next_at + 1 });
                true
            },
            _ => false
        }
    }

    fn is_under_a_path_attribute(&mut self, declaration_at: usize) -> bool {
        self.comments_and_attributes.read_up_to(self.contents, declaration_at);
        let mut at = declaration_at;
        loop {
            while at > 0 && self.contents[at - 1].is_ascii_whitespace() { at -= 1; }
            if at == 0 { return false; }
            if let Some(start) = self.comments_and_attributes.find_comment_holding(at - 1) {
                at = start;
                continue;
            }
            match self.comments_and_attributes.find_attribute_holding(at - 1) {
                Some((_, _, true)) => return true,
                Some((start, _, false)) => at = start,
                None => return false
            }
        }
    }

    // Only a declaration inside asks this, since asking where the module opens would lex every file
    // with a mod tests up to it
    fn is_inside_a_module_under_a_path_attribute(&mut self) -> bool {
        (0..self.open.len()).any(|index| {
            let InlineModule { names_a_path, looks_back_from, body_from, .. } = self.open[index];
            names_a_path || self.is_under_a_path_attribute(looks_back_from) || opens_with_a_path_attribute(self.contents, body_from)
        })
    }

    // Read to the end of the line, so an attribute closing on it has its end and one closing below
    // still holds nothing
    fn find_end_of_the_attribute_holding(&mut self, line: &[u8], base: usize, first: usize, last_keyword: usize)
            -> Option<usize> {
        memchr::memchr(b']', line.get(first..last_keyword)?)?;
        self.comments_and_attributes.read_up_to(self.contents, base + line.len());
        let (_, end, _) = self.comments_and_attributes.find_attribute_holding(base + first)?;
        Some(end - base)
    }
}

struct InlineModule {
    name: Box<str>,
    opened_at: u32,
    names_a_path: bool,
    looks_back_from: usize,
    body_from: usize,
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

fn mark_rows_the_seeds_reach(rows: &[ModuleRow], source: &mut dyn DeclarationSource) -> Vec<bool> {
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
    marked
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

#[derive(Default)]
struct CommentsAndAttributes {
    comments: Vec<(usize, usize)>,
    attributes: Vec<(usize, usize, bool)>,
    reading_attribute: Option<usize>,
    brackets: u32,
    read_until: usize,
}

impl CommentsAndAttributes {
    fn read_up_to(&mut self, contents: &[u8], end: usize) {
        let mut at = self.read_until;
        while at < end {
            if !COMMENT_AND_ATTRIBUTE_STOPS[contents[at] as usize] {
                at += 1;
                continue;
            }
            at = match contents[at] {
                b'/' => match attribute::skip_comment(contents, at, contents.len()) {
                    Some(past) => {
                        self.comments.push((at, past));
                        past
                    },
                    None => at + 1
                },
                b'"' => attribute::skip_string(contents, find_string_start(contents, at), contents.len()).unwrap_or(contents.len()),
                b'\'' => attribute::skip_character_literal(contents, at),
                b'#' => self.begin_attribute(contents, at),
                bracket => {
                    self.count_bracket(bracket, at);
                    at + 1
                }
            };
        }
        self.read_until = at;
    }

    // The brackets are counted as the file is read, since find_matching_bracket scans up to 4 KB from
    // each '#[' and a file full of ones that never close pays that for every one. One that never
    // closes holds nothing.
    fn begin_attribute(&mut self, contents: &[u8], at: usize) -> usize {
        if self.reading_attribute.is_some() { return at + 1; }
        let Some((open, inner)) = find_attribute_bracket(contents, at) else { return at + 1 };
        self.reading_attribute = Some(self.attributes.len());
        self.attributes.push((at, at, is_a_path_attribute(contents, open, inner)));
        self.brackets = 1;
        open + 1
    }

    fn count_bracket(&mut self, bracket: u8, at: usize) {
        let Some(index) = self.reading_attribute else { return };
        if bracket == b'[' {
            self.brackets += 1;
            return;
        }
        self.brackets -= 1;
        if self.brackets == 0 {
            self.attributes[index].1 = at + 1;
            self.reading_attribute = None;
        }
    }

    fn find_comment_holding(&self, at: usize) -> Option<usize> {
        let after = self.comments.partition_point(|&(start, _)| start <= at);
        let &(start, end) = self.comments.get(after.checked_sub(1)?)?;
        (at < end).then_some(start)
    }

    fn find_attribute_holding(&self, at: usize) -> Option<(usize, usize, bool)> {
        let after = self.attributes.partition_point(|&(start, _, _)| start <= at);
        let &(start, end, names_a_path) = self.attributes.get(after.checked_sub(1)?)?;
        (at < end).then_some((start, end, names_a_path))
    }
}

// A code range opens on the space after a comment, and the file's first one on its byte order mark
fn find_first_code_byte(line: &[u8], ranges: &[(usize, usize)], opens_the_file: bool) -> Option<usize> {
    ranges.iter().find_map(|&(from, to)| {
        let from = match opens_the_file && from == 0 && line.starts_with(BYTE_ORDER_MARK) {
            true => BYTE_ORDER_MARK.len(),
            false => from
        };
        let at = skip_whitespace(line, from);
        (at < to).then_some(at)
    })
}

fn skip_attributes(line: &[u8], from: usize) -> Option<(usize, bool)> {
    let mut at = attribute::skip_whitespace_and_comments(line, from);
    let mut names_a_path = false;
    while line.get(at) == Some(&b'#') {
        let (past, names) = read_attribute_at(line, at)?;
        names_a_path |= names;
        at = attribute::skip_whitespace_and_comments(line, past);
    }
    Some((at, names_a_path))
}

// Read as Rust's attribute, so a ']' inside a string of one closes nothing
fn read_attribute_at(bytes: &[u8], at: usize) -> Option<(usize, bool)> {
    let (open, inner) = find_attribute_bracket(bytes, at)?;
    Some((attribute::find_matching_bracket(bytes, open, b']')? + 1, is_a_path_attribute(bytes, open, inner)))
}

fn find_attribute_bracket(bytes: &[u8], at: usize) -> Option<(usize, bool)> {
    let mut open = attribute::skip_whitespace_and_comments(bytes, at + 1);
    let inner = bytes.get(open) == Some(&b'!');
    if inner { open = attribute::skip_whitespace_and_comments(bytes, open + 1); }
    (bytes.get(open) == Some(&b'[')).then_some((open, inner))
}

// An inner attribute belongs to the module around it and names no declaration's file
fn is_a_path_attribute(bytes: &[u8], open: usize, inner: bool) -> bool {
    !inner && is_named_path(bytes, open)
}

fn is_named_path(bytes: &[u8], open: usize) -> bool {
    let mut name_start = attribute::skip_whitespace_and_comments(bytes, open + 1);
    if bytes[name_start..].starts_with(RAW_IDENTIFIER) { name_start += RAW_IDENTIFIER.len(); }
    &bytes[name_start..find_word_end(bytes, name_start)] == PATH_ATTRIBUTE
            || attribute::is_applied_through_cfg_attr(&bytes[name_start..], PATH_ATTRIBUTE)
}

// Inner attributes come before anything else in a module's body, so only the leading ones are read
fn opens_with_a_path_attribute(contents: &[u8], body_from: usize) -> bool {
    let mut at = attribute::skip_whitespace_and_comments(contents, body_from);
    while contents.get(at) == Some(&b'#') {
        let Some((open, true)) = find_attribute_bracket(contents, at) else { return false };
        if is_named_path(contents, open) { return true; }
        let Some(close) = attribute::find_matching_bracket(contents, open, b']') else { return false };
        at = attribute::skip_whitespace_and_comments(contents, close + 1);
    }
    false
}

fn find_string_start(contents: &[u8], quote: usize) -> usize {
    let mut at = quote;
    while at > 0 && contents[at - 1] == b'#' { at -= 1; }
    match at > 0 && contents[at - 1] == b'r' && attribute::starts_a_raw_string(contents, at - 1) {
        true => at - 1,
        false => quote
    }
}

// With no code left the line comes back cut where the skip began, so every read after it finds nothing
fn skip_to_code<'a>(line: &'a [u8], ranges: &[(usize, usize)], from: usize) -> (usize, &'a [u8]) {
    ranges.iter().filter(|(_, to)| from < *to).find_map(|&(start, to)| {
        let at = skip_whitespace(&line[..to], from.max(start));
        (at < to).then_some((at, &line[..to]))
    }).unwrap_or((from, &line[..from]))
}

fn find_visibility_close(line: &[u8], ranges: &[(usize, usize)], open: usize) -> Option<usize> {
    ranges.iter().filter(|(_, to)| open < *to).find_map(|&(start, to)| {
        let from = open.max(start);
        memchr::memchr(b')', &line[from..to]).map(|found| from + found)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Keyword, LineClasses, StringRules};

    type TestLine<'a> = (&'a str, Option<&'a [(usize, usize)]>, bool);

    fn read_with_ranges(lines: &[TestLine<'_>]) -> Vec<Declaration> {
        let contents = lines.iter().map(|(line, _, _)| *line).collect::<Vec<_>>().join("\n");
        let mut reader = ModuleDeclarations::of("mod", &contents);
        let mut line_start = 0;
        for (line, ranges, is_test) in lines {
            let lead = line.len() - line.trim_ascii_start().len();
            let whole = [(0, line.trim_ascii().len())];
            let whole_line = [(line_start, usize::MAX)];
            let test_spans: &[(usize, usize)] = if *is_test { &whole_line } else { &[] };
            if !line.trim_ascii().is_empty() {
                reader.observe_line(line_start, line, lead, ranges.unwrap_or(&whole), test_spans);
            }
            line_start += line.len() + 1;
        }
        reader.into_declarations()
    }

    fn read(lines: &[(&str, bool)]) -> Vec<Declaration> {
        read_with_ranges(&lines.iter().map(|(line, is_test)| (*line, None, *is_test)).collect::<Vec<_>>())
    }

    // The spans are offsets into their own line
    fn read_with_test_spans(lines: &[(&str, &[(usize, usize)])]) -> Vec<Declaration> {
        let contents = lines.iter().map(|(line, _)| *line).collect::<Vec<_>>().join("\n");
        let mut reader = ModuleDeclarations::of("mod", &contents);
        let mut line_start = 0;
        for (line, test_spans) in lines {
            let lead = line.len() - line.trim_ascii_start().len();
            let test_spans = test_spans.iter().map(|&(from, to)| (line_start + from, to.saturating_add(line_start))).collect::<Vec<_>>();
            reader.observe_line(line_start, line, lead, &[(0, line.trim_ascii().len())], &test_spans);
            line_start += line.len() + 1;
        }
        reader.into_declarations()
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
                ("#[allow(dead_code)] #[path = \"x.rs\"]", false), ("mod under_a_second_attribute;", false),
                ("# [path = \"x.rs\"]", false), ("mod under_a_spaced_path;", false), ("#[r#path = \"x.rs\"]", false), ("mod under_a_raw_path;", false),
                ("#[cfg_attr(test, path = \"x.rs\")]", true), ("#[cfg(test)]", true), ("mod under_a_conditional_path;", true),
                ("#[cfg_attr(test, allow(dead_code))]", false), ("mod under_a_cfg_attr_without_a_path;", false),
                ("#[cfg_attr(test, cfg_attr(test, path = \"x.rs\"))]", true), ("mod under_a_nested_conditional_path;", true),
                ("#[cfg_attr(test, r#path = \"x.rs\")]", true), ("mod under_a_raw_conditional_path;", true),
                ("#[cfg_attr(test, cfg_attr(test, allow(dead_code)))]", false), ("mod under_a_nested_cfg_attr_without_a_path;", false),
                ("#[cfg_attr(test, /* why */ path = \"x.rs\")]", true), ("mod under_a_noted_conditional_path;", true),
                ("#[cfg_attr(test, rustfmt::skip(')', path = \"x.rs\"))]", true), ("mod under_a_path_inside_a_tool_attribute;", true),
                ("mod plain;", false)]);
        assert_eq!(vec![build_declaration("after_an_item", &[], false), build_declaration("under_a_longer_word", &[], false),
                build_declaration("under_an_attribute_with_its_item", &[], true), build_declaration("under_a_cfg_attr_without_a_path", &[], false),
                build_declaration("under_a_nested_cfg_attr_without_a_path", &[], false),
                build_declaration("under_a_path_inside_a_tool_attribute", &[], true), build_declaration("plain", &[], false)], found);
        assert_eq!(vec![build_declaration("after_an_inner_path", &[], true)],
                read(&[("#![path = \"x.rs\"]", false), ("#[cfg(test)]", true), ("mod after_an_inner_path;", true)]));

        // As the parser hands the lines over, with the string of the attribute outside the code ranges
        let path_line = "#[path = \"elsewhere.rs\"]";
        let doc_line = "#[doc = \"]\"] mod documented;";
        let found = read_with_ranges(&[(path_line, Some(&[(0, 9), (path_line.len() - 1, path_line.len())]), true),
                ("mod aside;", None, true), (doc_line, Some(&[(0, 8), (11, doc_line.len())]), false),
                ("#[cfg(test)] // mod inside_a_comment;", Some(&[(0, 12)]), true), ("mod after_a_comment;", None, true)]);
        assert_eq!(vec![build_declaration("documented", &[], false), build_declaration("after_a_comment", &[], true)], found);
    }

    #[test]
    fn a_path_attribute_written_over_several_lines_is_read_whole() {
        let found = read(&[("#[path =", true), ("    \"x.rs\"]", true), ("mod over_two_lines;", true),
                ("#[cfg(test)]", true), ("#[path = concat!(", true), ("    \"x\",", true), ("    \".rs\")]", true),
                ("mod over_three_lines;", true),
                ("#[path = \"x.rs\"]", false), ("#[cfg(any(", false), ("    test,", false), ("    unix))]", false),
                ("mod under_a_path_and_a_long_attribute;", false),
                ("#[doc = \"a", false), ("]\"]", false), ("mod under_a_bracket_in_a_string;", false),
                ("const LIST: &[u8] = &[", false), ("    1]", false), ("mod after_an_expression;", false),
                ("#[cfg(any(", false), ("    test,", false), ("    unix))]", false), ("mod under_a_long_attribute;", false)]);
        assert_eq!(vec![build_declaration("under_a_bracket_in_a_string", &[], false),
                build_declaration("after_an_expression", &[], false), build_declaration("under_a_long_attribute", &[], false)], found);

        let long_line = format!("static A: &[u8] = &[{}1]", "1, ".repeat(2_000));
        assert_eq!(vec![build_declaration("after_a_long_line", &[], false)],
                read(&[(long_line.as_str(), false), ("mod after_a_long_line;", false)]));
    }

    #[test]
    fn a_path_attribute_is_found_through_the_comments_between_it_and_the_declaration() {
        let tight = "*/mod on_the_closing_line;";
        let spaced = "*/ mod on_the_closing_line_after_a_space;";
        let found = read_with_ranges(&[("#[path = \"x.rs\"]", None, true), ("/* the checks", None, true), ("   live apart */", None, true),
                ("mod under_two_lines;", None, true),
                ("#[path = \"x.rs\"]", None, true), ("/* the checks", None, true), ("   live apart", None, true), ("*/", None, true),
                ("mod under_a_lone_closer;", None, true),
                ("#[path = \"x.rs\"]", None, true), ("/* the checks", None, true), (tight, Some(&[(2, tight.len())]), true),
                ("#[path = \"x.rs\"]", None, true), ("/* the checks", None, true), (spaced, Some(&[(2, spaced.len())]), true),
                ("#[path = \"x.rs\"]", None, true), ("/* outer", None, true), ("   /* inner */", None, true), ("   still outer */", None, true),
                ("mod under_a_nested_comment;", None, true),
                ("#[path = \"x.rs\"]", None, true), ("/* a /* b */* c */", None, true), ("mod under_a_star_after_a_closer;", None, true),
                ("#[path = \"x.rs\"]", None, true), ("/* matches src/**/*.rs */", None, true), ("mod under_a_glob;", None, true),
                ("#[path = \"x.rs\"]", None, true), ("/* a note */ // and another", None, true), ("mod under_two_comments;", None, true),
                ("#[path = \"x.rs\"]", None, true), ("/*", None, true), (" * a note", None, true), (" */ // and another", None, true),
                ("mod under_a_starred_comment;", None, true),
                ("#[path = \"x.rs\"]", None, true), ("/* start", None, true), ("// middle */", None, true), ("mod under_a_closer_after_slashes;", None, true),
                ("#[path = \"x.rs\"] /* a note */ // and another", None, true), ("mod under_an_attribute_and_two_comments;", None, true),
                ("#[path = \"x.rs\"] /* the checks", None, true), ("   live apart */", None, true), ("mod under_an_opener_beside_it;", None, true),
                ("#[path = \"x.rs\"]", None, true), ("/* a note */ #[cfg(test)]", None, true), ("mod under_a_comment_and_an_attribute;", None, true),
                ("#[cfg(test)] /* a note */ #[path = \"x.rs\"]", None, true), ("mod under_a_path_behind_a_comment;", None, true),
                ("#[path =", None, true), ("    \"x.rs\"] // where it lives", None, true), ("mod under_a_long_path_and_a_comment;", None, true),
                ("/** banner **/", None, false), ("#[path = \"x.rs\"]", None, true), ("mod under_a_path_below_a_banner;", None, true),
                ("#[ /* a note */ path = \"x.rs\"]", None, true), ("mod under_a_noted_path;", None, true),
                ("#[path = \"x.rs\"]", None, false), ("struct Between; /* the checks", None, false), ("   live apart */", None, false),
                ("mod after_an_item_and_a_comment;", None, false),
                ("#[path = \"x.rs\"]", None, false), ("/* a note */ struct Between;", None, false), ("mod after_an_item_behind_a_comment;", None, false),
                ("/* the checks", None, false), ("   live apart */", None, false), ("mod under_a_comment_alone;", None, false)]);
        assert_eq!(vec![build_declaration("after_an_item_and_a_comment", &[], false), build_declaration("after_an_item_behind_a_comment", &[], false),
                build_declaration("under_a_comment_alone", &[], false)], found);
    }

    #[test]
    fn what_a_comment_a_string_or_a_character_holds_opens_nothing() {
        let found = read(&[("#[path = \"gen.rs\"] // see /*", false), ("mod generated;", false), ("#[cfg(test)] // */", true), ("mod checks;", true),
                ("pub const S: &str = \"/*\";", false), ("#[cfg(test)]", true), ("#[path = \"x.rs\"] // a note */", true), ("mod under_a_note;", true),
                ("#[path = \"helpers.rs\"]", false), ("/// Loads every file matching src/*", false), ("mod helpers;", false),
                ("pub fn f() {} // the end of it all */", false), ("#[cfg(test)]", true), ("mod after_a_function;", true),
                ("#[path = \"x.rs\"]", false), ("// see /* here", false), ("struct S; // was */", false), ("#[cfg(test)]", true),
                ("mod after_a_struct;", true),
                ("const QUOTE: char = '\"';", false), ("#[path = \"x.rs\"]", true), ("mod after_a_quote_character;", true),
                ("fn f<'a>(x: &'a str) -> &'a str { x }", false), ("#[path = \"x.rs\"]", true), ("mod after_a_lifetime;", true),
                ("const RAW: &str = r#\"a \"/*\" b\"#;", false), ("#[path = \"x.rs\"]", true), ("mod after_a_raw_string;", true),
                ("const Q: char = '\\\"';", false), ("#[path = \"x.rs\"]", true), ("mod after_an_escaped_quote;", true),
                ("const B: &[u8] = br\"\\\";", false), ("#[path = \"x.rs\"]", true), ("mod after_a_raw_byte_string;", true),
                ("const C: [char; 2] = ['é','\"'];", false), ("#[path = \"x.rs\"]", true), ("mod after_a_wide_character;", true),
                ("pub fn q(c: char) -> bool { matches!(c, '\\u{10_FFFF}'|'\"') }", false), ("#[path = \"x.rs\"]", true),
                ("mod after_a_long_escape;", true),
                ("#[rustfmt::skip('\"', \"]\")]", false), ("pub struct A;", false), ("#[cfg(test)]", true), ("#[path = \"x.rs\"]", true),
                ("mod after_a_quote_in_an_attribute;", true),
                ("#[path = \"x.rs\"]", false), ("#[rustfmt::skip('[')]", false), ("mod bracketed;", false), ("#[cfg(test)]", true),
                ("mod after_a_bracket_in_an_attribute;", true), ("pub const CLOSE: char = ']';", false)]);
        assert_eq!(vec![build_declaration("checks", &[], true), build_declaration("after_a_function", &[], true),
                build_declaration("after_a_struct", &[], true), build_declaration("after_a_bracket_in_an_attribute", &[], true)], found);
    }

    #[test]
    fn a_declaration_after_a_comment_on_its_line_is_read() {
        let comment = "/* a note */ mod after_a_comment;";
        let attribute = "#[cfg(test)] /* a note */ mod after_an_attribute_and_a_comment;";
        let two = "/* a */ /* b */ mod after_two_comments;";
        let between = "mod before_a_comment; /* c */ mod after_one;";
        let found = read_with_ranges(&[(comment, Some(&[(12, comment.len())]), false),
                (attribute, Some(&[(0, 13), (25, attribute.len())]), true), (two, Some(&[(15, two.len())]), false),
                (between, Some(&[(0, 22), (29, between.len())]), false)]);
        assert_eq!(vec![build_declaration("after_a_comment", &[], false), build_declaration("after_an_attribute_and_a_comment", &[], true),
                build_declaration("after_two_comments", &[], false), build_declaration("before_a_comment", &[], false),
                build_declaration("after_one", &[], false)], found);
        let marked = "\u{feff}/* a note */#[cfg(test)] mod after_a_byte_order_mark;";
        assert_eq!(vec![build_declaration("after_a_byte_order_mark", &[], true)],
                read_with_ranges(&[(marked, Some(&[(0, 3), (15, marked.len())]), true)]));
    }

    #[test]
    fn a_comment_between_the_words_of_a_declaration_is_read_through() {
        let visibility = "pub /* c */ mod after_a_visibility;";
        let glued = "pub/* c */ mod glued_to_a_visibility;";
        let keyword = "mod /* c */ after_the_keyword;";
        let scoped = "pub(crate) /* c */ mod after_a_scoped_visibility;";
        let scope = "pub /* c */ (crate) mod before_a_scope;";
        let name = "mod after_the_name /* c */ ;";
        let opened = "mod opened /* c */ { mod inside; }";
        let everywhere = "#[cfg(test)] /* a */ pub /* b */ mod /* c */ everywhere /* d */;";
        let two = "mod first; /* x */ pub /* y */ mod second;";
        let commented = "mod read; /* mod unread; */";
        let found = read_with_ranges(&[(visibility, Some(&[(0, 4), (11, visibility.len())]), false),
                (glued, Some(&[(0, 3), (10, glued.len())]), false), (keyword, Some(&[(0, 4), (11, keyword.len())]), false),
                (scoped, Some(&[(0, 11), (18, scoped.len())]), false), (scope, Some(&[(0, 4), (11, scope.len())]), false),
                (name, Some(&[(0, 19), (26, name.len())]), false), (opened, Some(&[(0, 11), (18, opened.len())]), false),
                (everywhere, Some(&[(0, 13), (20, 25), (32, 37), (44, 56), (63, everywhere.len())]), true),
                (two, Some(&[(0, 11), (18, 23), (30, two.len())]), false), (commented, Some(&[(0, 10)]), false)]);
        assert_eq!(vec![build_declaration("after_a_visibility", &[], false), build_declaration("glued_to_a_visibility", &[], false),
                build_declaration("after_the_keyword", &[], false), build_declaration("after_a_scoped_visibility", &[], false),
                build_declaration("before_a_scope", &[], false), build_declaration("after_the_name", &[], false),
                build_declaration("inside", &["opened"], false), build_declaration("everywhere", &[], true),
                build_declaration("first", &[], false), build_declaration("second", &[], false), build_declaration("read", &[], false)], found);
        let glued_keyword = "mod/* c */ glued_to_the_keyword;";
        let inside_a_scope = "pub(/* c */ crate) mod inside_a_scope;";
        assert_eq!(vec![build_declaration("glued_to_the_keyword", &[], false), build_declaration("inside_a_scope", &[], false)],
                read_with_ranges(&[(glued_keyword, Some(&[(0, 3), (10, glued_keyword.len())]), false),
                (inside_a_scope, Some(&[(0, 4), (11, inside_a_scope.len())]), false)]));
    }

    #[test]
    fn a_comment_or_a_string_never_supplies_the_name_or_the_end_of_a_declaration() {
        let string = "pub mod \"x\";";
        let name = "mod /* ; */ ;";
        let opener = "mod x /* ; */ {";
        let line_comment = "mod x // ;";
        let second_name = "mod x /* y */ z;";
        assert!(read_with_ranges(&[(string, Some(&[(0, 8), (11, string.len())]), false), (name, Some(&[(0, 4), (11, name.len())]), false),
                (opener, Some(&[(0, 6), (13, opener.len())]), false), ("}", None, false), (line_comment, Some(&[(0, 6)]), false),
                (second_name, Some(&[(0, 6), (13, second_name.len())]), false)]).is_empty());
    }

    #[test]
    fn a_declaration_after_an_attribute_closing_on_its_line_is_read_from_where_the_attribute_ends() {
        let found = read(&[("#[cfg(", true), ("    test)] mod m;", true),
                ("#[cfg(", true), ("    test", true), (")] mod first; mod second;", true),
                ("#[cfg(", true), ("    test)] mod t { mod x; } mod y;", true),
                ("#[cfg(any(", true), ("    test,", true), ("    unix))] #[allow(dead_code)] mod after_a_second_attribute;", true),
                ("mod outer {", false), ("    #[cfg(", true), ("        test)] mod inside;", true), ("}", false),
                ("#[cfg(", true), ("    test,", true), ("    unix)]", true), ("mod on_the_next_line;", true),
                ("let v = items[i]; model.run();", false)]);
        assert_eq!(vec![build_declaration("m", &[], true), build_declaration("first", &[], true),
                build_declaration("second", &[], true), build_declaration("x", &["t"], true), build_declaration("y", &[], true),
                build_declaration("after_a_second_attribute", &[], true), build_declaration("inside", &["outer"], true),
                build_declaration("on_the_next_line", &[], true)], found);

        // As the parser hands the lines over, with the strings and the comments outside the code ranges
        let after_a_string = "]\"] mod after_a_bracket_in_a_string;";
        let after_a_note = "test)] /* a note */ mod after_a_note;";
        let found = read_with_ranges(&[("#[doc = \"a", Some(&[(0, 8)]), false),
                (after_a_string, Some(&[(2, after_a_string.len())]), false),
                ("#[cfg( // why ]", Some(&[(0, 7)]), true), (after_a_note, Some(&[(0, 7), (19, after_a_note.len())]), true)]);
        assert_eq!(vec![build_declaration("after_a_bracket_in_a_string", &[], false), build_declaration("after_a_note", &[], true)],
                found);
    }

    #[test]
    fn a_path_attribute_closing_on_the_line_of_its_declaration_still_refuses_it() {
        let path_closes = "\"x.rs\"] mod under_a_path;";
        let path_closes_under_a_cfg = "\"x.rs\"] mod under_a_cfg_and_a_path;";
        let path_follows = "test)] #[path = \"x.rs\"] mod under_a_path_after_a_long_attribute;";
        let found = read_with_ranges(&[("#[path =", None, true), (path_closes, Some(&[(6, path_closes.len())]), true),
                ("#[cfg(test)]", None, true), ("#[path =", None, true),
                (path_closes_under_a_cfg, Some(&[(6, path_closes_under_a_cfg.len())]), true),
                ("#[cfg(", None, true), (path_follows, Some(&[(0, 16), (22, path_follows.len())]), true),
                ("mod plain;", None, false)]);
        assert_eq!(vec![build_declaration("plain", &[], false)], found);
    }

    #[test]
    fn nothing_declared_inside_an_inline_module_under_a_path_attribute_is_followed() {
        let found = read(&[("#[path = \"d\"] mod on_one_line { mod y; }", false),
                ("#[path = \"d\"]", false), ("mod outer {", false), ("    mod y;", false), ("}", false), ("mod after;", false),
                ("#[cfg(test)]", true), ("#[path = \"d\"]", true), ("mod tests {", true), ("    mod y;", true), ("}", true),
                ("#[path = \"d\"] mod a { mod b { mod c; } }", false),
                ("mod a { #[path = \"d\"] mod b { mod c; } mod e; }", false),
                ("#[cfg_attr(test, path = \"d\")] mod conditional { mod y; }", false),
                ("#[path = \"d\"]", false), ("mod empty {", false), ("}", false), ("mod after_an_empty_one;", false)]);
        assert_eq!(vec![build_declaration("after", &[], false), build_declaration("e", &["a"], false),
                build_declaration("after_an_empty_one", &[], false)], found);
        assert_eq!(vec![build_declaration("y", &["outer"], false)],
                read(&[("#![path = \"d\"]", false), ("mod outer { mod y; }", false)]));
    }

    #[test]
    fn nothing_declared_inside_an_inline_module_opening_with_an_inner_path_attribute_is_followed() {
        let found = read(&[("#[cfg(test)]", true), ("mod checks {", true), ("    #![path = \"elsewhere\"]", true),
                ("    mod util;", true), ("}", true),
                ("mod noted {", false), ("    // a note before the attributes", false), ("    #![allow(dead_code)]", false),
                ("    #![cfg_attr(test, path = \"d\")]", false), ("    mod inside;", false), ("}", false),
                ("mod on_one_line { #![path = \"d\"] mod y; }", false),
                ("mod allowed { #![allow(unused)] mod z; }", false), ("mod after;", false)]);
        assert_eq!(vec![build_declaration("z", &["allowed"], false), build_declaration("after", &[], false)], found);
    }

    #[test]
    fn a_module_whose_brace_or_semicolon_sits_on_a_later_line_is_read_there() {
        let found = read_with_ranges(&[("#[cfg(test)]", None, true), ("mod checks", None, true), ("{", None, true),
                ("    mod util;", None, true), ("}", None, true),
                ("mod noted // a note", Some(&[(0, 10)]), false), ("// another", Some(&[]), false), ("{ mod inner; }", None, false),
                ("#[cfg(test)]", None, true), ("mod split", None, true), (";", None, false),
                ("mod dropped", None, false), ("let x = 1;", None, false), ("mod after;", None, false)]);
        assert_eq!(vec![build_declaration("util", &["checks"], true), build_declaration("inner", &["noted"], false),
                build_declaration("split", &[], true), build_declaration("after", &[], false)], found);
    }

    #[test]
    fn an_indented_declaration_is_read_through_its_lead() {
        let found = read(&[("    mod indented;", false), ("\tpub mod tabbed;", false), ("  #[cfg(unix)] mod attributed;", false)]);
        assert_eq!(vec![build_declaration("indented", &[], false), build_declaration("tabbed", &[], false), build_declaration("attributed", &[], false)], found);
    }

    #[test]
    fn a_declaration_later_on_a_line_is_read_inside_the_modules_the_line_opened() {
        let found = read(&[("#[cfg(test)] mod t { mod x; }", true), ("mod a; mod b;", false),
                ("mod outer { mod inner; } mod after;", false), ("mod t { #[path = \"q.rs\"] mod pathed; }", false),
                ("#[path = \"q.rs\"]", false), ("mod first; mod second;", false),
                ("pub fn f() { g(); } mod after_a_function;", false), ("fn h() { let mod_x = 1; }", false),
                ("mod m { fn f() { let a = 1; } mod deep; }", false)]);
        assert_eq!(vec![build_declaration("x", &["t"], true), build_declaration("a", &[], false), build_declaration("b", &[], false),
                build_declaration("inner", &["outer"], false), build_declaration("after", &[], false),
                build_declaration("second", &[], false), build_declaration("after_a_function", &[], false),
                build_declaration("deep", &["m"], false)], found);
    }

    #[test]
    fn a_declaration_after_another_item_on_a_top_level_line_is_read_once() {
        let found = read(&[("struct S; #[cfg(test)] mod m;", true), ("fn f() {} mod after;", false),
                ("const X: u8 = 1; mod a; mod b;", false), ("impl S {} #[cfg(test)] mod t;", true), ("use a::b; mod c;", false),
                ("x[0]; mod m;", false), ("#[cfg(unix)] mod first; struct S; mod second;", false),
                ("let model = 1;", false), ("fn modify() { x(); }", false), ("x = 1; model.run();", false),
                ("fn f() {", false), ("    let mod_x = 1;", false), ("}", false), ("mod after_a_body;", false)]);
        assert_eq!(vec![build_declaration("m", &[], true), build_declaration("after", &[], false), build_declaration("a", &[], false),
                build_declaration("b", &[], false), build_declaration("t", &[], true), build_declaration("c", &[], false),
                build_declaration("m", &[], false), build_declaration("first", &[], false), build_declaration("second", &[], false),
                build_declaration("after_a_body", &[], false)], found);
    }

    #[test]
    fn a_declaration_is_a_seed_only_where_it_lies_inside_the_test_code_of_its_line() {
        let found = |line: &str, test_spans: &[(usize, usize)]| read_with_test_spans(&[(line, test_spans)]);
        assert_eq!(vec![build_declaration("t", &[], true), build_declaration("x", &[], false)],
                found("#[cfg(test)] mod t; mod x;", &[(0, 19)]));
        assert_eq!(vec![build_declaration("a", &[], false), build_declaration("t", &[], true)],
                found("mod a; #[cfg(test)] mod t;", &[(7, 26)]));
        assert_eq!(vec![build_declaration("x", &["t"], true), build_declaration("y", &[], false)],
                found("#[cfg(test)] mod t { mod x; } mod y;", &[(0, 29)]));
        assert_eq!(vec![build_declaration("t", &["a"], true), build_declaration("y", &["a"], false)],
                found("mod a { #[cfg(test)] mod t; mod y; }", &[(8, 27)]));
        assert_eq!(vec![build_declaration("a", &[], true), build_declaration("b", &[], false), build_declaration("c", &[], true)],
                found("#[cfg(test)] mod a; mod b; #[cfg(test)] mod c;", &[(0, 19), (27, 46)]));
        assert_eq!(vec![build_declaration("t", &[], true), build_declaration("x", &[], false)],
                found("#[allow(dead_code)] #[cfg(test)] mod t; mod x;", &[(20, 39)]), "an attribute in front of the marker hid the seed");
        assert_eq!(vec![build_declaration("a", &[], false), build_declaration("t", &[], true)],
                found("mod a; /* c */ #[cfg(test)] mod t;", &[(15, 34)]), "a comment in front of the marker hid the seed");
        assert_eq!(vec![build_declaration("cases", &["tests"], true), build_declaration("after", &[], false)],
                read_with_test_spans(&[("#[cfg(test)]", &[(0, usize::MAX)]), ("mod tests {", &[(0, usize::MAX)]),
                        ("    mod cases;", &[(0, usize::MAX)]), ("}", &[(0, 1)]), ("mod after;", &[])]));
    }

    #[test]
    fn a_byte_order_mark_opening_the_file_hides_no_declaration() {
        let found = read(&[("\u{feff}mod outer {", false), ("    mod inside;", true), ("}", false)]);
        assert_eq!(vec![build_declaration("inside", &["outer"], true)], found);
        let found = read(&[("\u{feff}#[path = \"x.rs\"]", false), ("mod below_it;", true), ("mod plain;", false)]);
        assert_eq!(vec![build_declaration("plain", &[], false)], found);
        let found = read(&[("\u{feff} #[cfg(test)] mod checks;", true)]);
        assert_eq!(vec![build_declaration("checks", &[], true)], found);
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
        assert_eq!(vec![build_declaration("a", &["outer"], false), build_declaration("b", &["outer"], false),
                build_declaration("c", &[], false)], found);
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
        ModuleRow { path: join_as_the_scan_does(path), module, language_name: "Rust".into(),
                stats: FileStats { lines, classes: classes_of(lines), keyword_occurences: Vec::new() }, bytes: lines * 10, declarations,
                partial: partial_lines.map(|partial| TestReport { bytes: partial * 10,
                        stats: FileStats { lines: partial, classes: classes_of(partial), keyword_occurences: Vec::new() } }) }
    }

    fn rust_only() -> HashMap<String, Language> {
        hashmap!("Rust".to_owned() => Language::new("Rust", ["rs"], StringRules::escaping_nothing(), ["//"], &[],
                [Keyword::new("structs", ["struct"])]))
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

        let promotion = promote_declared_test_modules(&rows, &mut tests, &mut files, &rust_only(), &mut Tree::default());
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
        assert_eq!(Promotion { seeds: 1, promoted: 1 }, promote_declared_test_modules(&rows, &mut tests, &mut files, &rust_only(), &mut Tree::default()));
    }

    #[test]
    fn a_reached_file_is_a_module_so_its_declarations_never_resolve_beside_it() {
        let rows = vec![build_row("p/src/bin/x.rs", 0, 3, Some(1), vec![build_declaration("y", &[], true)]),
                build_row("p/src/bin/y.rs", 0, 7, None, vec![build_declaration("x", &[], false)])];
        let mut tests = vec![HashMap::new()];
        let mut files = vec![HashMap::new()];
        assert_eq!(Promotion { seeds: 1, promoted: 1 }, promote_declared_test_modules(&rows, &mut tests, &mut files, &rust_only(), &mut Tree::default()));
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
        assert_eq!(Promotion { seeds: 2, promoted: 1 }, promote_declared_test_modules(&rows, &mut tests, &mut files, &rust_only(), &mut Tree::default()));
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
        assert_eq!(Promotion { seeds: 2, promoted: 2 }, promote_declared_test_modules(&rows, &mut tests, &mut files, &rust_only(), &mut tree));
        let share = &tests[0]["Rust"];
        assert_eq!((2, 5, 2), (share.stats.files, share.stats.lines, share.whole_files),
                "helpers.rs and inner.rs through the files without a row, and neither same-named file a folder up");
    }

    #[test]
    fn a_seed_naming_nothing_moves_nothing_and_an_empty_file_counts_one_file() {
        let rows = vec![build_row("p/src/lib.rs", 0, 5, Some(2), vec![build_declaration("absent", &[], true), build_declaration("empty", &[], true)]),
                build_row("p/src/empty.rs", 0, 0, None, vec![])];
        let mut tests = vec![HashMap::new()];
        let mut files = vec![HashMap::new()];
        assert_eq!(Promotion { seeds: 2, promoted: 1 }, promote_declared_test_modules(&rows, &mut tests, &mut files, &rust_only(), &mut Tree::default()));
        let share = &tests[0]["Rust"];
        assert_eq!((1, 0, 1), (share.stats.files, share.stats.lines, share.whole_files));
    }

    #[test]
    fn a_promoted_file_moves_its_keywords_with_it_less_what_its_own_marker_counted() {
        let mut rows = vec![build_row("p/src/lib.rs", 0, 5, Some(2), vec![build_declaration("cases", &[], true)]),
                build_row("p/src/cases.rs", 0, 9, Some(3), vec![])];
        rows[1].stats.keyword_occurences = vec![4];
        rows[1].partial.as_mut().unwrap().stats.keyword_occurences = vec![1];
        let mut tests = vec![HashMap::new()];
        let mut files = vec![HashMap::new()];
        assert_eq!(Promotion { seeds: 1, promoted: 1 }, promote_declared_test_modules(&rows, &mut tests, &mut files, &rust_only(), &mut Tree::default()));
        let share = &tests[0]["Rust"];
        assert_eq!((6, hashmap!("structs".to_owned() => 3)), (share.stats.lines, share.stats.keyword_occurences.clone()));
    }

    #[test]
    fn a_run_with_no_seed_books_nothing() {
        let rows = vec![build_row("p/src/lib.rs", 0, 50, None, vec![build_declaration("plain", &[], false)]),
                build_row("p/src/plain.rs", 0, 20, None, vec![])];
        let mut tests = vec![HashMap::new()];
        let mut files = vec![HashMap::new()];
        assert_eq!(Promotion::default(), promote_declared_test_modules(&rows, &mut tests, &mut files, &rust_only(), &mut Tree::default()));
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
