// Reading one file and deciding what each of its lines is: code, comment, or neither. The hot path
// of the whole program, and the only file here where the work is the algorithm rather than the
// arrangement.
//
// A line is scanned once for every symbol the language declares, in as few memchr passes as the
// symbols allow, and what comes back are the positions of string delimiters, comment openers and the
// two multiline markers. The rest of the file decides what those positions mean when they overlap,
// which is where every language-specific trap lives.
use std::{collections::HashMap, fs::File, io::Read as IoRead, path::Path, str};

use memchr::memmem;

use crate::{EngineConfig, Language, NestedLanguage, phase_timing};
use crate::domain::{CommentPair, FileStats};

pub const MAX_RETAINED_FILE_BUFFER_BYTES: usize = 4_194_304;

const NO_SLOT : u16 = u16::MAX;

// The four kinds of declared symbol, as indices into the per-kind arrays of a scan
const STRINGS    : u8 = 0;
const COMMENTS   : u8 = 1;
const COM_STARTS : u8 = 2;
const COM_ENDS   : u8 = 3;

// What one side of a string pair may do. An ordinary quote is both sides in one symbol; a pair
// whose halves differ gets one slot per half, and its opener cannot close nor its closer open.
// 'RAW' is 'EITHER' without the backslash rule, for a symbol that serves as both ends of a form
// that escapes nothing: Go's and Odin's backtick against JavaScript's template literal.
// A character literal is a symbol that only exists paired on its own line: the scan looks for its
// other half right away, emits the two as opener and closer when both are there, and emits nothing
// at all when they are not, which is what keeps a lifetime's lone ' from opening anything.
const ROLE_EITHER  : u8 = 0;
const ROLE_OPEN    : u8 = 1;
const ROLE_CLOSE   : u8 = 2;
const ROLE_LITERAL : u8 = 3;
const ROLE_RAW     : u8 = 4;

// One declared symbol. 'next' chains every symbol that begins with the same byte, longest first,
// so that a '"""' is recognised before the '"' that starts it.
//
// 'anchor' is how far behind the searched byte the symbol begins, and it is not zero only for a
// symbol that starts with an ordinary letter: searching 'r#"' by its 'r' would visit every 'for'
// and 'return' in a Rust file and add a second memchr pass to every line. Such a symbol is
// searched by its last punctuation byte instead, the quote it already shares with the other
// string symbols, and checked backwards from there.
// 'filler' is zero for every ordinary symbol. For a leveled one it is the counted byte: the slot's
// bytes are the prefix, and a match must find a run of the filler after it, then 'suffix'.
#[derive(Debug, Clone, Copy)]
struct Slot {
    symbol: u8,
    kind: u8,
    role: u8,
    len: u8,
    second: u8,
    anchor: u8,
    filler: u8,
    suffix: u8,
    next: u16,
}

#[derive(Debug, Clone, Copy)]
struct Chunk {
    bytes: [u8; 3],
    len: u8,
}

// One declared symbol on its way into the plan, before it becomes a slot
struct PlanEntry {
    kind: u8,
    symbol: u8,
    role: u8,
    filler: u8,
    suffix: u8,
    bytes: Box<[u8]>,
}

impl PlanEntry {
    fn of(kind: u8, symbol: u8, role: u8, bytes: &[u8]) -> PlanEntry {
        PlanEntry { kind, symbol, role, filler: 0, suffix: 0, bytes: bytes.into() }
    }

    fn leveled(kind: u8, symbol: u8, prefix: &[u8], suffix: u8) -> PlanEntry {
        PlanEntry { kind, symbol, role: ROLE_EITHER, filler: b'=', suffix, bytes: prefix.into() }
    }
}

// Every symbol begins with one byte and memchr searches up to three bytes in a single SIMD pass, so
// symbols are grouped by their first byte and the groups packed into as few passes as the language
// allows: one for most, two for the handful declaring more than three distinct first bytes. A pass
// yields candidate positions, and only there is the rest of a symbol compared.
#[derive(Debug, Clone)]
pub struct ScanPlan {
    chunks: Vec<Chunk>,
    first: [u16; 256],
    slots: Vec<Slot>,
    symbols: Vec<Box<[u8]>>,
    sorted_kinds: [bool; 4],
    // Whether a line opening with a line comment symbol is a comment and nothing else. False where a
    // block opener begins with one, as Lua's '--[[' begins with '--', CMake's '#[[' and Julia's '#='
    // with '#': there the same bytes open a block that runs on past this line.
    line_comment_ends_the_line: bool,
}

impl ScanPlan {
    pub fn build(language: &Language) -> ScanPlan {
        // The single line symbols first, the character literals after them and the crossing ones
        // last, which is the numbering 'Language::get_string_pair_of' answers to
        let mut entries : Vec<PlanEntry> = Vec::new();
        for (i, symbol) in language.string_symbols.iter().enumerate() {
            entries.push(PlanEntry::of(STRINGS, i as u8, ROLE_EITHER, symbol.as_bytes()));
        }
        for (i, symbol) in language.char_literal_symbols.iter().enumerate() {
            let index = (language.string_symbols.len() + i) as u8;
            entries.push(PlanEntry::of(STRINGS, index, ROLE_LITERAL, symbol.as_bytes()));
        }
        for (i, crossing) in language.multiline_strings.iter().enumerate() {
            let index = (language.string_symbols.len() + language.char_literal_symbols.len() + i) as u8;
            let (open, close) = (&crossing.open, &crossing.close);
            if open != close {
                entries.push(PlanEntry::of(STRINGS, index, ROLE_OPEN, open.as_bytes()));
                entries.push(PlanEntry::of(STRINGS, index, ROLE_CLOSE, close.as_bytes()));
            } else {
                let role = if crossing.escapes {ROLE_EITHER} else {ROLE_RAW};
                entries.push(PlanEntry::of(STRINGS, index, role, open.as_bytes()));
            }
        }
        for (i, symbol) in language.comment_symbols.iter().enumerate() {
            entries.push(PlanEntry::of(COMMENTS, i as u8, ROLE_EITHER, symbol.as_bytes()));
        }
        // Numbered by the language itself, so this and the helpers that answer to those numbers
        // cannot disagree about the order. A leveled slot holds its prefix as the bytes.
        for (i, pair) in language.comment_pairs().enumerate() {
            let index = i as u8;
            match pair {
                CommentPair::Plain { start, end } | CommentPair::Nesting { start, end } => {
                    entries.push(PlanEntry::of(COM_STARTS, index, ROLE_EITHER, start.as_bytes()));
                    entries.push(PlanEntry::of(COM_ENDS, index, ROLE_EITHER, end.as_bytes()));
                },
                CommentPair::Leveled(pair) => {
                    entries.push(PlanEntry::leveled(COM_STARTS, index, pair.start_prefix.as_bytes(), pair.start_suffix));
                    entries.push(PlanEntry::leveled(COM_ENDS, index, pair.end_prefix.as_bytes(), pair.end_suffix));
                }
            }
        }
        entries.retain(|entry| !entry.bytes.is_empty());
        let line_comment_ends_the_line = !entries.iter().filter(|entry| entry.kind == COM_STARTS)
                .any(|start| entries.iter().filter(|entry| entry.kind == COMMENTS)
                        .any(|comment| start.bytes.starts_with(&comment.bytes)));
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.bytes.len()));

        let anchors = anchors_of(&entries);
        let mut first = [NO_SLOT; 256];
        let (mut slots, mut symbols) = (Vec::with_capacity(entries.len()), Vec::with_capacity(entries.len()));
        for (entry, anchor) in entries.iter().zip(&anchors) {
            let index = slots.len() as u16;
            let anchor = *anchor;
            slots.push(Slot {
                symbol: entry.symbol,
                kind: entry.kind,
                role: entry.role,
                len: entry.bytes.len() as u8,
                second: if entry.bytes.len() > 1 { entry.bytes[1] } else { 0 },
                anchor,
                filler: entry.filler,
                suffix: entry.suffix,
                next: NO_SLOT,
            });
            symbols.push(entry.bytes.clone());
            let head = &mut first[entry.bytes[anchor as usize] as usize];
            if *head == NO_SLOT {
                *head = index;
            } else {
                let mut cursor = *head as usize;
                while slots[cursor].next != NO_SLOT { cursor = slots[cursor].next as usize }
                slots[cursor].next = index;
            }
        }

        let (chunks, mut sorted_kinds) = pack_into_chunks(&entries, &anchors);
        // An anchored match begins behind the byte that found it, so a kind holding symbols
        // anchored at two different depths reports them out of line order and is sorted afterwards.
        // One depth throughout, which is what most kinds of most languages have, still arrives in
        // the order the line is written in.
        for (kind, sorted) in sorted_kinds.iter_mut().enumerate() {
            let mut depths = entries.iter().zip(&anchors)
                    .filter(|(entry, _)| entry.kind as usize == kind).map(|(_, anchor)| *anchor);
            let Some(first) = depths.next() else { continue };
            if depths.any(|depth| depth != first) { *sorted = true }
        }
        ScanPlan { chunks, first, slots, symbols, sorted_kinds, line_comment_ends_the_line }
    }
}

// Where in each symbol the byte that finds it sits. Every symbol could be searched by its first
// byte, and the choice exists because the bytes are not equally cheap: a letter is visited on every
// word of the file, and any byte at all costs a memchr pass over every line once three others are
// already spoken for. So the fewest bytes that reach every symbol are chosen, and each symbol is
// anchored on the first of its own bytes among them. See 'Slot'.
//
// Two symbols are what this is for. 'r#"' and 'R"(' begin with a letter and are found by the quote
// their language declares anyway. ')"' begins with a bracket, which stands in front of every call
// in C++, and is found by the same quote.
fn anchors_of(entries: &[PlanEntry]) -> Vec<u8> {
    // A symbol offering one byte has no say in the matter, and those are searched whatever else
    // is decided
    let mut searched : Vec<u8> = Vec::new();
    for entry in entries {
        let mut bytes = get_candidate_bytes_of(entry);
        let Some(first) = bytes.next() else { continue };
        if bytes.all(|byte| byte == first) && !searched.contains(&first) { searched.push(first) }
    }
    while let Some(byte) = find_the_byte_reaching_most_of(entries, &searched) {
        searched.push(byte);
    }

    // A symbol of nothing but letters reaches none of them and keeps its first byte, which is the
    // only case the loop above leaves unanswered
    entries.iter().map(|entry| entry.bytes.iter().position(|byte| searched.contains(byte))
            .unwrap_or(0) as u8).collect()
}

fn get_candidate_bytes_of(entry: &PlanEntry) -> impl Iterator<Item = u8> + '_ {
    entry.bytes.iter().copied().filter(|byte| !byte.is_ascii_alphanumeric())
}

fn is_reached_by(entry: &PlanEntry, searched: &[u8]) -> bool {
    get_candidate_bytes_of(entry).any(|byte| searched.contains(&byte))
}

// The byte that would reach the most of what nothing reaches yet, or nothing when everything is
// reached. On a tie the one met first wins, so a language's plan is the same on every run.
fn find_the_byte_reaching_most_of(entries: &[PlanEntry], searched: &[u8]) -> Option<u8> {
    let waiting = entries.iter().filter(|entry| get_candidate_bytes_of(entry).next().is_some()
            && !is_reached_by(entry, searched)).collect::<Vec<&PlanEntry>>();

    let mut best : Option<(u8, usize)> = None;
    for entry in &waiting {
        for byte in get_candidate_bytes_of(entry) {
            let reach = waiting.iter().filter(|other|
                    get_candidate_bytes_of(other).any(|other_byte| other_byte == byte)).count();
            if best.is_none_or(|(_, most)| reach > most) { best = Some((byte, reach)) }
        }
    }
    best.map(|(byte, _)| byte)
}

// Two kinds sharing a first byte must be searched in the same pass, or that byte gets visited twice.
// Kinds are grouped by that overlap and the groups packed whole, which is what leaves every output
// vector already in the order the positions appear on the line. A group of more than three distinct
// bytes cannot be one pass, so it is split and its kinds are marked as needing a sort after all.
fn pack_into_chunks(entries: &[PlanEntry], anchors: &[u8]) -> (Vec<Chunk>, [bool; 4]) {
    let mut bytes_of_kind : [Vec<u8>; 4] = Default::default();
    for (entry, anchor) in entries.iter().zip(anchors) {
        let searched = entry.bytes[*anchor as usize];
        let set = &mut bytes_of_kind[entry.kind as usize];
        if !set.contains(&searched) { set.push(searched) }
    }

    let mut group_of = [0usize, 1, 2, 3];
    for a in 0..4 {
        for b in (a + 1)..4 {
            if bytes_of_kind[a].iter().any(|x| bytes_of_kind[b].contains(x)) {
                let (from, to) = (group_of[b], group_of[a]);
                for slot in group_of.iter_mut() { if *slot == from { *slot = to } }
            }
        }
    }

    let (mut chunks, mut sorted_kinds) : (Vec<Vec<u8>>, [bool; 4]) = (Vec::new(), [false; 4]);
    for group in 0..4 {
        let mut group_bytes : Vec<u8> = Vec::new();
        for kind in 0..4 {
            if group_of[kind] != group { continue }
            for byte in &bytes_of_kind[kind] {
                if !group_bytes.contains(byte) { group_bytes.push(*byte) }
            }
        }
        if group_bytes.is_empty() { continue }

        if group_bytes.len() > 3 {
            for kind in 0..4 { if group_of[kind] == group { sorted_kinds[kind] = true } }
            for piece in group_bytes.chunks(3) { chunks.push(piece.to_vec()) }
            continue;
        }
        match chunks.iter_mut().find(|chunk| chunk.len() + group_bytes.len() <= 3) {
            Some(chunk) => chunk.extend_from_slice(&group_bytes),
            None => chunks.push(group_bytes)
        }
    }

    let chunks = chunks.into_iter().map(|bytes| {
        let mut padded = [0u8; 3];
        padded[..bytes.len()].copy_from_slice(&bytes);
        Chunk { bytes: padded, len: bytes.len() as u8 }
    }).collect();

    (chunks, sorted_kinds)
}

fn get_or_build_plan_of(language: &Language) -> &ScanPlan {
    language.scan_plan.get_or_init(|| ScanPlan::build(language))
}

// The per line working memory, owned by the consumer thread and cleared rather than reallocated.
#[derive(Debug, Default)]
pub struct ScanBuffers {
    raw_strings: Vec<(usize, u8, u8)>,
    strings: Vec<usize>,
    string_symbols: Vec<u8>,
    comments: Vec<usize>,
    // Position, pair, and the level a leveled occurrence carried, zero for every other pair
    com_starts: Vec<(usize, u8, u8)>,
    com_ends: Vec<(usize, u8, u8)>,
    consumed: Vec<usize>,
    // The stretches of the line that are code, as ranges into it, instead of a copy of them
    // concatenated into a String
    code_ranges: Vec<(usize, usize)>,
}

impl ScanBuffers {
    fn reset(&mut self, slots: usize) {
        self.raw_strings.clear();
        self.strings.clear();
        self.string_symbols.clear();
        self.comments.clear();
        self.com_starts.clear();
        self.com_ends.clear();
        self.consumed.clear();
        self.consumed.resize(slots, 0);
        self.code_ranges.clear();
    }
}

// The keyword scratch cannot live in ScanBuffers: while a LineInfo borrows the cleansed line out of
// it, the whole struct is borrowed, and counting the keywords of that very line needs a free one.
#[derive(Debug, Default)]
pub struct ParseBuffers {
    scan: ScanBuffers,
    alias_indices: Vec<usize>,
    // every stretch of the file that is code, gathered line by line so that the keywords can be
    // searched once over the whole buffer instead of once per alias per line
    code_spans: Vec<(u32, u32)>,
    pub timing: phase_timing::Totals,
}

fn is_not_escaped(pos: usize, bytes: &[u8]) -> bool {
    let mut slashes = 0;
    let mut offset = 1;
    while pos >= offset && bytes[pos - offset] == b'\\' {
        offset += 1;
        slashes += 1;
    }
    slashes % 2 == 0
}

fn scan_line(line: &str, language: &Language, buffers: &mut ScanBuffers) {
    let plan = get_or_build_plan_of(language);
    let line_bytes = line.as_bytes();
    buffers.reset(plan.slots.len());

    for chunk in &plan.chunks {
        match chunk.len {
            1 => for at in memchr::memchr_iter(chunk.bytes[0], line_bytes) {
                take_symbols_at(at, line_bytes, plan, buffers)
            },
            2 => for at in memchr::memchr2_iter(chunk.bytes[0], chunk.bytes[1], line_bytes) {
                take_symbols_at(at, line_bytes, plan, buffers)
            },
            _ => for at in memchr::memchr3_iter(chunk.bytes[0], chunk.bytes[1], chunk.bytes[2], line_bytes) {
                take_symbols_at(at, line_bytes, plan, buffers)
            }
        }
    }

    // Only a language with more than three distinct first bytes in one kind gets here, since
    // otherwise a single pass already yields them in the order they are written
    if plan.sorted_kinds[STRINGS as usize] {
        let length_of = |symbol: u8, role: u8| {
            let (open, close) = language.get_string_pair_of(symbol);
            match role { ROLE_CLOSE => close.len(), _ => open.len() }
        };
        buffers.raw_strings.sort_unstable_by(|(a_at, a_symbol, a_role), (b_at, b_symbol, b_role)|
                a_at.cmp(b_at).then_with(|| length_of(*b_symbol, *b_role).cmp(&length_of(*a_symbol, *a_role))));
    }
    if plan.sorted_kinds[COMMENTS as usize] { buffers.comments.sort_unstable() }
    if plan.sorted_kinds[COM_STARTS as usize] {
        buffers.com_starts.sort_unstable_by(|(a_at, a_symbol, a_level), (b_at, b_symbol, b_level)|
                a_at.cmp(b_at).then_with(|| language.comment_start_len(*b_symbol, *b_level)
                        .cmp(&language.comment_start_len(*a_symbol, *a_level))));
    }
    if plan.sorted_kinds[COM_ENDS as usize] {
        buffers.com_ends.sort_unstable_by(|(a_at, a_symbol, a_level), (b_at, b_symbol, b_level)|
                a_at.cmp(b_at).then_with(|| language.comment_end_len(*b_symbol, *b_level)
                        .cmp(&language.comment_end_len(*a_symbol, *a_level))));
    }
}

fn take_symbols_at(at: usize, line_bytes: &[u8], plan: &ScanPlan, buffers: &mut ScanBuffers) {
    let mut cursor = plan.first[line_bytes[at] as usize];
    while cursor != NO_SLOT {
        let index = cursor as usize;
        let slot = plan.slots[index];
        cursor = slot.next;

        // An anchored symbol begins behind the byte that found it
        let Some(start) = at.checked_sub(slot.anchor as usize) else { continue };
        // Each symbol is searched without overlapping itself, so "///" holds one "//" and not two.
        // A counted slot is exempt: every level shares the one slot, so ']]' and ']=]' are two
        // different symbols rather than one overlapping itself, and the shorter would hide the
        // longer that begins inside it.
        if slot.filler == 0 && start < buffers.consumed[index] { continue }
        let matched = match (slot.anchor, slot.len) {
            (0, 1) if slot.filler == 0 => true,
            (0, 2) if slot.filler == 0 => line_bytes.get(at + 1) == Some(&slot.second),
            _ => line_bytes[start..].starts_with(&plan.symbols[index])
        };
        if !matched { continue }
        // A leveled symbol is its prefix, a counted run of the filler, and the closing byte; the
        // count is carried beside the position so only an end with the same count can answer it
        let mut level = 0u8;
        let mut width = slot.len as usize;
        if slot.filler != 0 {
            let mut cursor = start + slot.len as usize;
            while line_bytes.get(cursor) == Some(&slot.filler) && level < u8::MAX {
                cursor += 1;
                level += 1;
            }
            if line_bytes.get(cursor) != Some(&slot.suffix) { continue }
            width = cursor + 1 - start;
        }
        // An escape cancels a string symbol and nothing else, and only where the language says the
        // form escapes at all: inside a raw one, one-sided or two-sided, the backslash is a byte
        if slot.kind == STRINGS && (slot.role == ROLE_EITHER || slot.role == ROLE_LITERAL)
                && start != 0 && !is_not_escaped(start, line_bytes) { continue }

        // A character literal exists only whole: its other half is looked for here and now, and
        // both halves go in together as an opener and its closer, or neither does. Whatever sits
        // between them is inside the taken pair, so the resolution drops it on its own.
        if slot.role == ROLE_LITERAL {
            let symbol_bytes = &plan.symbols[index];
            let mut cursor = start + width;
            let closed_at = loop {
                let Some(offset) = memchr::memchr(symbol_bytes[0], &line_bytes[cursor..]) else { break None };
                let candidate = cursor + offset;
                if line_bytes[candidate..].starts_with(symbol_bytes) && is_not_escaped(candidate, line_bytes)
                        && holds_one_character(&line_bytes[start + width..candidate]) {
                    break Some(candidate);
                }
                cursor = candidate + 1;
            };
            let Some(closed_at) = closed_at else {
                buffers.consumed[index] = start + width;
                continue;
            };
            buffers.raw_strings.push((start, slot.symbol, ROLE_OPEN));
            buffers.raw_strings.push((closed_at, slot.symbol, ROLE_CLOSE));
            buffers.consumed[index] = closed_at + width;
            continue;
        }

        buffers.consumed[index] = start + width;
        match slot.kind {
            STRINGS => buffers.raw_strings.push((start, slot.symbol, slot.role)),
            COMMENTS => buffers.comments.push(start),
            COM_STARTS => buffers.com_starts.push((start, slot.symbol, level)),
            _ => buffers.com_ends.push((start, slot.symbol, level))
        }
    }
}

pub struct KeywordMatcher {
    aliases_with_indices: Vec<(memmem::Finder<'static>, usize, usize)>,
}

impl KeywordMatcher {
    pub fn build(language: &Language) -> Option<KeywordMatcher> {
        let mut aliases_with_indices = Vec::new();
        for (keyword_index, keyword) in language.keywords.iter().enumerate() {
            for alias in &keyword.aliases {
                aliases_with_indices.push((memmem::Finder::new(alias.as_str()).into_owned(), alias.len(), keyword_index));
            }
        }
        if aliases_with_indices.is_empty() {
            None
        } else {
            Some(KeywordMatcher { aliases_with_indices })
        }
    }
}

// The maps a section's language is found through. 'extension_to_name' and 'set_aside' cover the
// whole shipped set even when a run narrowed its languages, so that '--languages vue' still knows
// what JavaScript is; a caller with no sections in play hands empty maps.
pub struct NestedLanguageLookup<'a> {
    pub languages: &'a HashMap<String, Language>,
    pub extension_to_name: &'a HashMap<String, std::sync::Arc<str>>,
    pub set_aside: &'a HashMap<String, Language>,
}

impl NestedLanguageLookup<'_> {
    // What a tag says its section is written in, which people write either way: 'lang="scss"' is an
    // extension and 'type="text/typescript"' is a language's name. The extension is tried first,
    // since it is the form the declared defaults use and the one the user's priority rules answer
    // for, and the name after it, so a language whose name is a whole word is found by that word.
    fn find_by_spelling(&self, spelling: &str) -> Option<&Language> {
        let lowered = spelling.to_lowercase();
        if let Some(name) = self.extension_to_name.get(&lowered) {
            return self.find_by_name(name.as_ref());
        }
        self.languages.values().chain(self.set_aside.values())
                .find(|language| language.name.to_lowercase() == lowered)
    }

    pub fn find_by_name(&self, name: &str) -> Option<&Language> {
        self.languages.get(name).or_else(|| self.set_aside.get(name))
    }
}

// One matcher per language, built on first use. Sections make the per-file set open ended, which
// is why the cache travels instead of a single matcher.
#[derive(Default)]
pub struct KeywordMatchers {
    by_language: HashMap<String, Option<KeywordMatcher>>,
}

impl KeywordMatchers {
    fn for_language(&mut self, language: &Language) -> Option<&KeywordMatcher> {
        self.by_language.entry(language.name.clone())
                .or_insert_with(|| KeywordMatcher::build(language)).as_ref()
    }
}

// What one file counted to: the shell language's own lines, and one entry per nested language
// that had sections in the file. A file of a language with no regions is a report with no sections.
pub struct FileReport {
    pub shell: FileStats,
    pub sections: Vec<SectionReport>,
}

pub struct SectionReport {
    pub language: String,
    pub stats: FileStats,
    pub bytes: usize,
}

impl FileReport {
    pub fn total_lines(&self) -> usize {
        self.shell.lines + self.sections.iter().map(|section| section.stats.lines).sum::<usize>()
    }

    // The whole file as one number, which is what the shell language's row shows: a container file
    // weighs all of its lines. The keywords stay the shell's own, because a section's keywords
    // belong to the section's language and are carried by the sections themselves.
    pub fn into_whole(mut self) -> FileStats {
        for section in &self.sections {
            self.shell.lines += section.stats.lines;
            self.shell.code_lines += section.stats.code_lines;
            self.shell.comment_lines += section.stats.comment_lines;
        }
        self.shell
    }
}

pub fn parse_file(path: &Path, lang_name: &str, buf: &mut String, buffers: &mut ParseBuffers,
    lookup: &NestedLanguageLookup, matchers: &mut KeywordMatchers, config: &EngineConfig)
-> Result<FileReport,String>
{
    // None unless MEZURA_PHASE_TIMING is set, so a normal run never reads the clock at all
    let mut at = phase_timing::ENABLED.then(phase_timing::now);

    let mut file = match File::open(path){
        Ok(f) => f,
        Err(x) => return Err(x.to_string())
    };
    if let Some(t) = at {
        buffers.timing.open_nanos += phase_timing::nanos_since(t);
        at = Some(phase_timing::now());
    }

    buf.clear();
    if let Err(x) = file.read_to_string(buf) {
        return Err(x.to_string());
    }
    if let Some(t) = at {
        buffers.timing.read_nanos += phase_timing::nanos_since(t);
        buffers.timing.bytes += buf.len() as u64;
        buffers.timing.files += 1;
        at = Some(phase_timing::now());
    }

    let report = parse_lines(buf, lookup.languages.get(lang_name).unwrap(), lookup, matchers, config, buffers);
    if let Some(t) = at { buffers.timing.parse_nanos += phase_timing::nanos_since(t); }

    Ok(report)
}

// 'str::lines' splits through the standard library's own byte search, a SWAR loop over two words at
// a time; memchr is already a dependency and does the same with SIMD. Same lines out, including the
// trailing '\r' that 'lines' drops.
struct LineIter<'a> {
    contents: &'a str,
    newlines: memchr::Memchr<'a>,
    start: usize,
}

impl<'a> Iterator for LineIter<'a> {
    type Item = (usize, &'a str);

    fn next(&mut self) -> Option<(usize, &'a str)> {
        match self.newlines.next() {
            Some(at) => {
                let mut end = at;
                if end > self.start && self.contents.as_bytes()[end - 1] == b'\r' {
                    end -= 1;
                }
                let line = (self.start, &self.contents[self.start..end]);
                self.start = at + 1;
                Some(line)
            },
            None => {
                if self.start >= self.contents.len() {
                    return None;
                }
                let line = (self.start, &self.contents[self.start..]);
                self.start = self.contents.len();
                Some(line)
            }
        }
    }
}

fn get_lines_of(contents: &str) -> LineIter<'_> {
    LineIter { contents, newlines: memchr::memchr_iter(b'\n', contents.as_bytes()), start: 0 }
}

// The carry from one line to the next, one set per language in play: the shell's survives a
// section, a section's starts fresh at its opener and is dropped at its closer, the way the file
// really reads.
#[derive(Default)]
struct WalkState {
    open_comment: Option<(u8, u32)>,
    open_str_symbol: Option<u8>,
    continued_comment: bool,
}

// The lines of one nested language, added up over every section of it in the file
struct SectionBucket<'a> {
    language: &'a Language,
    stats: FileStats,
    spans: Vec<(u32, u32)>,
    bytes: usize,
}

fn parse_lines(contents: &str, language: &Language, lookup: &NestedLanguageLookup, matchers: &mut KeywordMatchers,
    config: &EngineConfig, buffers: &mut ParseBuffers) -> FileReport
{
    let ParseBuffers { scan, alias_indices, code_spans, .. } = buffers;
    let mut shell_stats = match config.count_keywords {
        false => FileStats::default(),
        true => FileStats::with_keywords(&language.keywords)
    };
    code_spans.clear();

    let mut shell = WalkState::default();
    let mut buckets: Vec<SectionBucket> = Vec::new();
    let mut lines = get_lines_of(contents);
    let mut handed_back = None;
    while let Some((line_start, raw_line)) = handed_back.take().or_else(|| lines.next()) {
        let had_code = walk_line(raw_line, line_start, language, config, config.count_keywords,
                scan, &mut shell, &mut shell_stats, code_spans);

        // A region opener only counts where the shell left it as code, so one sitting inside a
        // comment or a string of the shell opens nothing
        if had_code && !language.nested_languages.is_empty()
                && let Some((region, inner)) = find_region_opening(raw_line.trim_ascii(), &scan.code_ranges, language, lookup) {
            let section_from = end_of_line(contents, line_start, raw_line);
            // A section is only a section if it closes. Nothing forces an opener to be a tag rather
            // than the same text written inside one, and handing a language every line to the end
            // of the file on the strength of one word costs the whole file when it was not one.
            let Some(closer_at) = find_tag_ignoring_case(&contents.as_bytes()[section_from..],
                    region.end.as_bytes()) else { continue };
            let closer_at = section_from + closer_at;

            // The tag line itself belongs to the shell, and anything it left open is cut off at
            // the section boundary: per the HTML reading, what follows the tag is section content
            shell = WalkState::default();

            let bucket_at = match buckets.iter().position(|bucket| bucket.language.name == inner.name) {
                Some(at) => at,
                None => {
                    buckets.push(SectionBucket { language: inner, stats: match config.count_keywords {
                        false => FileStats::default(),
                        true => FileStats::with_keywords(&inner.keywords)
                    }, spans: Vec::new(), bytes: 0 });
                    buckets.len() - 1
                }
            };
            let bucket = &mut buckets[bucket_at];
            let mut inner_state = WalkState::default();
            let mut section_to = contents.len();
            for (inner_start, inner_raw) in lines.by_ref() {
                // Per the HTML reading the closer ends the section wherever it stands, even inside
                // a string of the section's language: that is why one writes '<\/script>' in
                // JavaScript. The closer's line belongs to the shell.
                if inner_start + inner_raw.len() > closer_at {
                    section_to = inner_start;
                    handed_back = Some((inner_start, inner_raw));
                    break;
                }
                walk_line(inner_raw, inner_start, inner, config, config.count_keywords,
                        scan, &mut inner_state, &mut bucket.stats, &mut bucket.spans);
            }
            bucket.bytes += section_to - section_from;
        }
    }

    if config.count_keywords {
        if let Some(matcher) = matchers.for_language(language) {
            count_keywords(contents, code_spans, matcher, &mut shell_stats, alias_indices);
        }
        for bucket in &mut buckets {
            if let Some(matcher) = matchers.for_language(bucket.language) {
                count_keywords(contents, &bucket.spans, matcher, &mut bucket.stats, alias_indices);
            }
        }
    }

    FileReport {
        shell: shell_stats,
        sections: buckets.into_iter().map(|bucket| SectionReport {
            language: bucket.language.name.clone(), stats: bucket.stats, bytes: bucket.bytes }).collect()
    }
}

// One line into the counts of one language. Returns whether the line left code behind, which is
// the only thing the section machinery needs to know about it.
fn walk_line(raw_line: &str, line_start: usize, language: &Language, config: &EngineConfig,
    collecting_spans: bool, scan: &mut ScanBuffers, state: &mut WalkState,
    file_stats: &mut FileStats, code_spans: &mut Vec<(u32, u32)>) -> bool
{
    file_stats.lines += 1;

    // Ascii-only trimming, since the unicode whitespace classification of trim() costs
    // a significant part of the total run time, for lines that are code either way
    let line = raw_line.trim_ascii();
    if line.is_empty() { state.continued_comment = false; return false; }
    let base = line_start + (raw_line.len() - raw_line.trim_ascii_start().len());

    // A line joined to the one before it by a continuation symbol is the tail of that line's
    // comment, and nothing on it is read: in C '// a comment \' makes the whole next line
    // comment too, however it is written.
    if state.continued_comment {
        file_stats.comment_lines += 1;
        state.continued_comment = ends_with_continuation(line, language);
        return false;
    }
    let continued = ends_with_continuation(line, language);

    let line_info = get_bounds(line, language, state.open_comment, &state.open_str_symbol,
            config.braces_as_code, scan);

    state.open_comment = line_info.open_comment_after;
    // Only a symbol declared to cross lines carries its string to the next one, so the damage
    // of an unbalanced quote is this line and not the rest of the file. A line ending in the
    // continuation symbol is the exception: there the quote has not been left open by mistake,
    // the language says the line goes on.
    let continues_a_string = continued && language.line_continuation.as_ref()
            .is_some_and(|continuation| continuation.in_strings);
    state.open_str_symbol = line_info.open_str_sybol_after
            .filter(|symbol| language.string_crosses_lines(*symbol) || continues_a_string);

    // Whether this line ended inside a comment that the next one carries on. Only asked of a
    // line that left no code and no open string behind, which is what a line comment leaves.
    state.continued_comment = continued && state.open_str_symbol.is_none() && state.open_comment.is_none()
            && line_info.code.is_none() && !line_info.has_string_literal
            && language.line_continuation.as_ref().is_some_and(|continuation| continuation.in_comments);

    if line_info.code.is_some() {
        let is_no_content = !line_info.has_string_literal
                && says_nothing(&scan.code_ranges, line, config.braces_as_code);
        if config.braces_as_code || !is_no_content {
            file_stats.code_lines += 1;
            if collecting_spans {
                push_trimmed_spans(code_spans, &scan.code_ranges, line, base);
            }
        }
        true
    } else if line_info.has_string_literal {
        file_stats.code_lines += 1;
        false
    } else {
        file_stats.comment_lines += 1;
        false
    }
}

// Where the line after this one begins, which is where a section's bytes start: past the line and
// past its own newline, whichever width the file wrote it with
fn end_of_line(contents: &str, line_start: usize, raw_line: &str) -> usize {
    let mut end = line_start + raw_line.len();
    if contents.as_bytes().get(end) == Some(&b'\r') { end += 1; }
    if contents.as_bytes().get(end) == Some(&b'\n') { end += 1; }
    end
}

// A region opener that survived the shell's own reading of the line, with the language its section
// is in. None when the tag does not close on this line, when its section also closes on this line,
// or when no language can be found for it: all three count as shell, which is what they were before
// regions existed.
fn find_region_opening<'a>(line: &str, code_ranges: &[(usize, usize)], language: &'a Language,
    lookup: &'a NestedLanguageLookup) -> Option<(&'a NestedLanguage, &'a Language)>
{
    let bytes = line.as_bytes();
    for (from, to) in code_ranges {
        let mut cursor = *from;
        while let Some(offset) = memchr::memchr(b'<', &bytes[cursor..*to]) {
            let at = cursor + offset;
            cursor = at + 1;
            for region in &language.nested_languages {
                if !starts_with_ignoring_case(&bytes[at..], region.start.as_bytes()) {
                    continue;
                }
                let after_start = at + region.start.len();
                // Where the name of the tag ends, so that '<scriptures>' is a word in a page and
                // not the opener of a script block
                match bytes.get(after_start) {
                    Some(byte) if byte.is_ascii_whitespace() || *byte == b'>' => (),
                    _ => continue
                }
                // The tag has to close on its own line; split over two, the line stays shell
                let Some(tag_close) = memchr::memchr(b'>', &bytes[after_start..]) else { continue };
                // A section that opens and closes on one line stays shell whole, tags and all
                if find_tag_ignoring_case(&bytes[after_start + tag_close..], region.end.as_bytes()).is_some() {
                    continue;
                }
                let tag_text = &line[after_start..after_start + tag_close];
                let named = find_attribute_value(tag_text, "lang")
                        .or_else(|| find_attribute_value(tag_text, "type").map(strip_mime_family));
                let inner = named.and_then(|value| lookup.find_by_spelling(value))
                        .or_else(|| lookup.find_by_spelling(&region.default));
                if let Some(inner) = inner {
                    return Some((region, inner));
                }
            }
        }
    }
    None
}

// The value of one attribute inside a tag's text, with either quote or none: lang="ts", lang='ts'
// and lang=ts all answer ts. The name has to be preceded by whitespace so that 'slang=' is not
// 'lang=', and the '=' may carry spaces around it.
fn find_attribute_value<'a>(tag_text: &'a str, name: &str) -> Option<&'a str> {
    let bytes = tag_text.as_bytes();
    let mut cursor = 0;
    while let Some(offset) = find_case_insensitive(&bytes[cursor..], name.as_bytes()) {
        let at = cursor + offset;
        cursor = at + 1;
        if at != 0 && !bytes[at - 1].is_ascii_whitespace() {
            continue;
        }
        let rest = tag_text[at + name.len()..].trim_ascii_start();
        let Some(value) = rest.strip_prefix('=') else { continue };
        let value = value.trim_ascii_start();
        return Some(match value.as_bytes().first() {
            Some(&quote @ (b'"' | b'\'')) => value[1..].split(quote as char).next().unwrap_or(""),
            _ => value.split_ascii_whitespace().next().unwrap_or("")
        });
    }
    None
}

// 'type="text/typescript"' names its language after the slash, and a bare 'type="module"' has none
fn strip_mime_family(value: &str) -> &str {
    value.rsplit('/').next().unwrap_or(value)
}

// Asked at every '<' of every line of a markup file, which is why it is a comparison and not a
// search for a match at zero: a search that answers "no" has walked the rest of the line first, and
// on one long line that is the whole line once per '<'.
fn starts_with_ignoring_case(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.len() >= needle.len()
            && haystack[..needle.len()].eq_ignore_ascii_case(needle)
}

// A tag anywhere in the text, found through one memchr pass on the byte it begins with and a
// comparison only where that lands, in both cases of that byte so that '</SCRIPT>' is found too.
fn find_tag_ignoring_case(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let first = *needle.first()?;
    memchr::memchr2_iter(first.to_ascii_lowercase(), first.to_ascii_uppercase(), haystack)
            .find(|at| starts_with_ignoring_case(&haystack[*at..], needle))
}

fn find_case_insensitive(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len())
            .find(|&at| haystack[at..at + needle.len()].eq_ignore_ascii_case(needle))
}

// 'code' is the span of ScanBuffers::code_ranges that belongs to this line. None means the line left
// no code behind at all, which is not the same as an empty span: a line whose code is only whitespace
// still produced a cleansed line, and counts as neither code nor comment.
// An open comment travels with its depth, which is 1 for every pair that does not nest and the
// count of unclosed openers for one that does.
#[derive(Debug, PartialEq)]
struct LineInfo {
    code: Option<(usize, usize)>,
    has_string_literal: bool,
    open_comment_after: Option<(u8, u32)>,
    open_str_sybol_after: Option<u8>
}

impl LineInfo {
    pub fn none_str(open_comment_after: Option<(u8, u32)>, has_string_literal: bool, open_str_sybol_after: Option<u8>) -> LineInfo {
        LineInfo { code: None, has_string_literal, open_comment_after, open_str_sybol_after }
    }

    pub fn code_span(span: (usize, usize), has_string_literal: bool) -> LineInfo {
        LineInfo { code: Some(span), has_string_literal, open_comment_after: None, open_str_sybol_after: None }
    }

    pub fn code_span_with(span: (usize, usize), has_string_literal: bool, open_comment_after: Option<(u8, u32)>,
        open_str_sybol_after: Option<u8>) -> LineInfo
    {
        LineInfo { code: Some(span), has_string_literal, open_comment_after, open_str_sybol_after }
    }

    pub fn with_open_comment(symbol: u8) -> LineInfo {
        LineInfo { code: None, has_string_literal: false, open_comment_after: Some((symbol, 1)), open_str_sybol_after: None }
    }

    pub fn open_comment_at(symbol: u8, depth: u32) -> LineInfo {
        LineInfo { code: None, has_string_literal: false, open_comment_after: Some((symbol, depth)), open_str_sybol_after: None }
    }

    pub fn with_open_symbol(symbol: u8) -> LineInfo {
        LineInfo { code: None, has_string_literal: true, open_comment_after: None, open_str_sybol_after: Some(symbol) }
    }

    pub fn none_all(has_string_literal: bool) -> LineInfo {
        LineInfo { code: None, has_string_literal, open_comment_after: None, open_str_sybol_after: None }
    }
}

// An empty stretch is not recorded, so that "did this line leave any code behind" is one question
// What a character literal is allowed to hold: one character, or an escape sequence, which is what
// tells a real literal from two unrelated symbols that happen to sit on one line. Without it a
// lifetime's tick pairs with the apostrophe of a word inside a string, the false literal swallows
// that string's opening quote, and the quote left over carries to the end of the file. A single
// byte above ASCII cannot be judged alone, so anything that is one whole character passes.
fn holds_one_character(between: &[u8]) -> bool {
    match between.first() {
        None => false,
        Some(b'\\') => true,
        Some(byte) if byte.is_ascii() => between.len() == 1,
        // The leading byte of a multi-byte character, so the run has to be exactly that character
        Some(_) => std::str::from_utf8(between).is_ok_and(|text| text.chars().count() == 1)
    }
}

// The symbol has to be the last thing on the line and not itself escaped, so a Windows path ending
// in a backslash inside a raw string does not join the next line to it.
fn ends_with_continuation(line: &str, language: &Language) -> bool {
    let Some(continuation) = &language.line_continuation else { return false };
    let bytes = line.as_bytes();
    bytes.ends_with(continuation.symbol.as_bytes())
            && is_not_escaped(bytes.len() - continuation.symbol.len(), bytes)
}

fn push_code(ranges: &mut Vec<(usize, usize)>, from: usize, to: usize) {
    if to > from {
        ranges.push((from, to));
    }
}

// Whether the code this line left behind says anything, asked the same way everywhere. With the
// strings and comments stripped, a line holding no letter and no digit is punctuation the language
// required rather than anything the programmer said: '}', '});', '],', ')'. Bytes above 0x7f count as
// content, so an identifier in a non-latin alphabet reads as code and not as punctuation. Under
// '--braces-as-code' that punctuation is code, so only whitespace says nothing.
//
// One function because two of them disagreed: the bounds asked for whitespace and 'walk_line' asked
// for letters, so '}  // end of function' was code to neither and a comment to neither, and its
// comment was reported nowhere.
fn says_nothing(ranges: &[(usize, usize)], line: &str, braces_as_code: bool) -> bool {
    let bytes = line.as_bytes();
    if braces_as_code {
        ranges.iter().all(|(from, to)| bytes[*from..*to].iter().all(|b| b.is_ascii_whitespace()))
    } else {
        !ranges.iter().any(|(from, to)|
                bytes[*from..*to].iter().any(|b| b.is_ascii_alphanumeric() || *b >= 0x80))
    }
}

fn line_info_with_str_symbol(ranges: usize, str_symbol: u8) -> LineInfo {
    if ranges == 0 {
        LineInfo::with_open_symbol(str_symbol)
    } else {
        LineInfo::code_span_with((0, ranges), true, None, Some(str_symbol))
    }
}

fn get_bounds(line: &str, language: &Language, open_comment: Option<(u8, u32)>,
    open_str_symbol: &Option<u8>, braces_as_code: bool, buffers: &mut ScanBuffers) -> LineInfo
{
    // A line comment runs to the end of its line, so a line that opens with one is comment through
    // and through and nothing the scan could find past it changes that. Only with nothing left open
    // above: inside a block or a crossing string the same bytes are text.
    // The buffers are left as the line before them left them, which is safe only because nothing
    // reads them when no code span comes back.
    if open_comment.is_none() && open_str_symbol.is_none()
            && get_or_build_plan_of(language).line_comment_ends_the_line
            && language.comment_symbols.iter().any(|symbol| line.as_bytes().starts_with(symbol.as_bytes())) {
        return LineInfo::none_all(false);
    }

    scan_line(line, language, buffers);
    resolve_string_delimiters(language, open_str_symbol, buffers);
    let ScanBuffers { strings: str_indices, string_symbols: str_symbols, comments: comment_indices,
            com_starts: com_start_indices, com_ends: com_end_indices, code_ranges, .. } = buffers;

    match open_comment {
        None => if open_str_symbol.is_some() && str_indices.is_empty() {
            return LineInfo::none_str(None, true, *open_str_symbol);
        },
        // Only the end of the pair that opened the block closes it, so a line holding none of
        // those is comment through and through, whatever other symbols sit on it. For a pair
        // that nests, another of its own starts changes the depth, so it counts as an event too;
        // for a leveled pair, an end only counts when it carries the level the opener did.
        Some((open_pair, carried)) => {
            let leveled = language.comment_is_leveled(open_pair);
            let has_end = com_end_indices.iter().any(|(_, symbol, level)|
                    *symbol == open_pair && (!leveled || *level as u32 == carried));
            let deepens = language.comment_nests(open_pair)
                    && com_start_indices.iter().any(|(_, symbol, _)| *symbol == open_pair);
            if !has_end && !deepens {
                return LineInfo::open_comment_at(open_pair, carried);
            }
        }
    }

    // A '//' that sits inside a '*/' is part of it and not a comment of its own
    comment_indices.retain(|x| !is_intersecting_with_multi_line_end_symbol(*x, language, com_end_indices));

    resolve_comment_and_multiline_start_overlap(line, language, comment_indices, com_start_indices);

    if !com_end_indices.is_empty() && !com_start_indices.is_empty() {
        resolve_double_counting_of_adjacent_start_and_end_symbols(com_start_indices, com_end_indices,
            open_comment.is_some(), language);
    }

    if str_indices.is_empty() && comment_indices.is_empty() && com_start_indices.is_empty() && com_end_indices.is_empty() {
        push_code(code_ranges, 0, line.len());
        return LineInfo::code_span((0, code_ranges.len()), false);
    }

    let (mut start_com_counter, mut end_com_counter, mut str_counter, mut comment_counter) = (0,0,0,0);
    let (mut open_com_m, mut is_str_open_m) = (open_comment, open_str_symbol.is_some());

    let has_more_comments = |counter| counter < comment_indices.len(); 
    let has_more_strs = |counter| counter < str_indices.len();
    let has_more_ends = |counter| counter < com_end_indices.len();
    let has_more_starts = |counter| counter < com_start_indices.len();
    let next_symbol_is_comment = |comment_counter: usize, str_counter: usize,
        start_counter: usize| {
        if !has_more_comments(comment_counter) {return false; }
        if has_more_strs(str_counter) && comment_indices[comment_counter] > str_indices[str_counter] {
            return false;
        }
        if has_more_starts(start_counter) && comment_indices[comment_counter] > com_start_indices[start_counter].0 {
            return false;
        }
        true
    };
    let next_symbol_is_string = |comment_counter: usize, str_counter: usize,
        start_counter: usize| {
        if !has_more_strs(str_counter) {return false;}
        if has_more_comments(comment_counter)  && str_indices[str_counter] > comment_indices[comment_counter] {
            return false;
        }
        if has_more_starts(start_counter) && str_indices[str_counter] > com_start_indices[start_counter].0 {
            return false;
        }
        true
    };
    let next_symbol_is_com_start = |comment_counter: usize, str_counter: usize,
        start_counter: usize| {
        if !has_more_starts(start_counter) {return false;}
        if has_more_comments(comment_counter) && com_start_indices[start_counter].0 > comment_indices[comment_counter] {
            return false;
        }
        if has_more_strs(str_counter) && com_start_indices[start_counter].0 > str_indices[str_counter] {
            return false;
        }
        true
    };
    let progress_counters_after = |index, comment_counter: &mut usize, str_counter: &mut usize,
        start_counter: &mut usize, end_counter: &mut usize| {
        while *comment_counter < comment_indices.len() && comment_indices[*comment_counter] < index {
            *comment_counter += 1;
        }
        while *str_counter < str_indices.len() && str_indices[*str_counter] < index {
            *str_counter += 1;
        }
        while *start_counter < com_start_indices.len() && com_start_indices[*start_counter].0 < index {
            *start_counter += 1;
        }
        while *end_counter < com_end_indices.len() && com_end_indices[*end_counter].0 < index {
            *end_counter += 1;
        }
    };
    let skipped_com_end_symbol = |last_symbol_index: usize, end_com_counter: usize, cur_index: usize| {
        has_more_ends(end_com_counter) && com_end_indices[end_com_counter].0 < cur_index && com_end_indices[end_com_counter].0 >= last_symbol_index
    };

    let mut has_string_literal = false;
    let mut slice_start_index = 0;
    let mut last_symbol_index = 0;
    loop {
        if is_str_open_m {
            last_symbol_index = str_indices[str_counter];
            let index_after = last_symbol_index
                    + language.get_string_pair_of(str_symbols[str_counter]).1.len();
            if index_after >= line.len() {
                if code_ranges.is_empty() {return LineInfo::none_all(true);}
                else {return LineInfo::code_span((0, code_ranges.len()), true);}
            }

            progress_counters_after(last_symbol_index, &mut comment_counter, &mut str_counter,
                    &mut start_com_counter, &mut end_com_counter);

            is_str_open_m = false;
            str_counter += 1;
            has_string_literal = true;
            slice_start_index = index_after;
        } else if let Some((open_pair, carried)) = open_com_m {
            // Ends of the other pairs inside this block are text. Walking the counters past them
            // is safe: everything before the closing position is dead once the block closes there.
            // For a pair that nests, each of its own starts before an end deepens the block, and
            // the closer is the end at which the count comes back to zero. For a leveled pair,
            // 'carried' is the level and only an end with the same count is looked at.
            let leveled = language.comment_is_leveled(open_pair);
            let nests = language.comment_nests(open_pair);
            let mut depth = if leveled { 1 } else { carried };
            let closing = loop {
                while end_com_counter < com_end_indices.len()
                        && (com_end_indices[end_com_counter].1 != open_pair
                            || (leveled && com_end_indices[end_com_counter].2 as u32 != carried)) {
                    end_com_counter += 1;
                }
                if end_com_counter == com_end_indices.len() { break None; }
                let end_at = com_end_indices[end_com_counter].0;

                if nests {
                    while start_com_counter < com_start_indices.len() && com_start_indices[start_com_counter].0 < end_at {
                        if com_start_indices[start_com_counter].1 == open_pair { depth = depth.saturating_add(1); }
                        start_com_counter += 1;
                    }
                }
                depth -= 1;
                if depth == 0 { break Some(end_at); }
                end_com_counter += 1;
            };
            let Some(closed_at) = closing else {
                let mut carry = carried;
                if nests {
                    while start_com_counter < com_start_indices.len() {
                        if com_start_indices[start_com_counter].1 == open_pair { depth = depth.saturating_add(1); }
                        start_com_counter += 1;
                    }
                    carry = depth;
                }
                // A block that stays open leaves a comment line whenever nothing but whitespace sat
                // outside it, whichever kind of pair it is
                if !has_string_literal && says_nothing(code_ranges, line, braces_as_code) {
                    return LineInfo::open_comment_at(open_pair, carry);
                }
                return LineInfo::code_span_with((0, code_ranges.len()), has_string_literal, Some((open_pair, carry)), None);
            };
            last_symbol_index = closed_at;
            let end_level = if leveled { carried as u8 } else { 0 };
            let index_after = last_symbol_index + language.comment_end_len(open_pair, end_level);
            if index_after >= line.len() {
                if says_nothing(code_ranges, line, braces_as_code) {return LineInfo::none_all(has_string_literal);}
                else {return LineInfo::code_span((0, code_ranges.len()), has_string_literal);}
            }

            // Every counter goes past the closer's own bytes and not merely past where it began, so
            // a symbol standing at the byte after it is reached by the ordinary dispatch below
            // rather than by a second copy of it here. The copy is what dropped the bookkeeping the
            // dispatch does: a start adopted without advancing its counter was counted a second time
            // by the depth walk, and a string opened without advancing its own had its opening quote
            // read back as the closing one.
            open_com_m = None;
            progress_counters_after(index_after, &mut comment_counter, &mut str_counter,
                    &mut start_com_counter, &mut end_com_counter);
            slice_start_index = index_after;
        } else {
            if next_symbol_is_comment(comment_counter, str_counter, start_com_counter) {
                push_code(code_ranges, slice_start_index, comment_indices[comment_counter]);
                if says_nothing(code_ranges, line, braces_as_code) {return LineInfo::none_all(has_string_literal);}
                else {return LineInfo::code_span((0, code_ranges.len()), has_string_literal);}
            } else if next_symbol_is_string(comment_counter, str_counter, start_com_counter) {
                let this_index = str_indices[str_counter];
                if skipped_com_end_symbol(last_symbol_index, end_com_counter, this_index) {
                    end_com_counter += 1;
                }
                push_code(code_ranges, slice_start_index, this_index);
                str_counter += 1;
                if !has_more_strs(str_counter) {
                    return line_info_with_str_symbol(code_ranges.len(), str_symbols[str_counter-1]);
                }
                
                is_str_open_m = true;
                has_string_literal = true;
                last_symbol_index = this_index;
            } else if next_symbol_is_com_start(comment_counter, str_counter, start_com_counter) {
                let (this_index, this_symbol, this_level) = com_start_indices[start_com_counter];
                if skipped_com_end_symbol(last_symbol_index, end_com_counter, this_index) {
                    end_com_counter += 1;
                }

                push_code(code_ranges, slice_start_index, this_index);
                // A nesting or leveled pair falls through to the open branch even with no ends
                // left: further starts of a nesting one still deepen the carried state, and the
                // leveled one carries its level either way
                if !has_more_ends(end_com_counter) && !language.comment_nests(this_symbol)
                        && !language.comment_is_leveled(this_symbol) {
                    if !has_string_literal && says_nothing(code_ranges, line, braces_as_code) {
                        return LineInfo::with_open_comment(this_symbol);
                    }
                    return LineInfo::code_span_with((0, code_ranges.len()), has_string_literal, Some((this_symbol, 1)), None);
                }

                open_com_m = Some((this_symbol,
                        if language.comment_is_leveled(this_symbol) { this_level as u32 } else { 1 }));
                start_com_counter += 1;
                last_symbol_index = this_index;
            } else {
                push_code(code_ranges, slice_start_index, line.len());
                return LineInfo::code_span((0, code_ranges.len()), has_string_literal);
            }
        }
    }
}

// The collision window around each symbol is that symbol's own span: an end beginning inside a
// start's bytes, or a start beginning inside an end's bytes. One shared length here is what
// miscounted Lua and HTML, whose pairs have unequal lengths, whenever a block closed and reopened
// on one line: the oversized window saw a collision where ']]--[[' merely touches, discarded the
// reopening start, and the rest of the file counted as code.
fn resolve_double_counting_of_adjacent_start_and_end_symbols(start_indices: &mut Vec<(usize, u8, u8)>,
    end_indices: &mut Vec<(usize, u8, u8)>, is_comment_open: bool, language: &Language)
{
   fn resolve_collision(start_indices: &mut Vec<(usize, u8, u8)>, end_indices: &mut Vec<(usize, u8, u8)>, start_counter: &mut usize,
       end_counter: &mut usize, is_comment_open_m: &mut bool, language: &Language)
   {
       if *is_comment_open_m {
           start_indices.remove(*start_counter);
           if *start_counter < start_indices.len() && start_indices[*start_counter].0 <
                   end_indices[*end_counter].0 + language.comment_end_len(end_indices[*end_counter].1, end_indices[*end_counter].2) {
               start_indices.remove(*start_counter);
           }
           *end_counter += 1;
       } else {
           end_indices.remove(*end_counter);
           if *end_counter < end_indices.len() && end_indices[*end_counter].0 <
                   start_indices[*start_counter].0 + language.comment_start_len(start_indices[*start_counter].1, start_indices[*start_counter].2) {
               end_indices.remove(*end_counter);
           }
           *start_counter += 1;
       }
       *is_comment_open_m = !*is_comment_open_m;
   }

   let mut is_comment_open_m = is_comment_open;
   let (mut start_counter, mut end_counter) = (0,0);
   loop {
       if start_counter == start_indices.len() || end_counter == end_indices.len() {break;}

       let (start_index, start_symbol, start_level) = start_indices[start_counter];
       let (end_index, end_symbol, end_level) = end_indices[end_counter];

       if end_index > start_index && end_index < start_index + language.comment_start_len(start_symbol, start_level) ||
                start_index > end_index && start_index < end_index + language.comment_end_len(end_symbol, end_level) {
            resolve_collision(start_indices, end_indices, &mut start_counter, &mut end_counter, &mut is_comment_open_m, language);
       } else {
           if start_index < end_index {
               start_counter += 1;
               if start_counter < start_indices.len() {
                   if start_indices[start_counter].0 > end_index {
                       is_comment_open_m = true;
                   }
               } else {
                   break;
               }
           }
           else {
               end_counter += 1;
               if end_counter < end_indices.len() {
                   if end_indices[end_counter].0 > start_counter {
                       is_comment_open_m = false;
                   }
               } else {
                   break;
               }
           }
       }
   }
}

// The trim decides whether a keyword at the start of the line has an acceptable prefix: a tab is not
// one, an empty prefix is. Trimming a concatenation means trimming the front of the first stretch and
// the back of the last, dropping any that empty out completely.
fn push_trimmed_spans(spans: &mut Vec<(u32, u32)>, ranges: &[(usize, usize)], line: &str, base: usize) {
    let bytes = line.as_bytes();
    let (mut head, mut tail) = (0usize, ranges.len());
    let (mut head_from, mut tail_to) = (0usize, 0usize);

    while head < tail {
        let (from, to) = ranges[head];
        let mut at = from;
        while at < to && bytes[at].is_ascii_whitespace() { at += 1; }
        if at < to { head_from = at; break; }
        head += 1;
    }
    if head == tail { return; }

    while tail > head {
        let (from, to) = ranges[tail - 1];
        let floor = if tail - 1 == head { head_from } else { from };
        let mut at = to;
        while at > floor && bytes[at - 1].is_ascii_whitespace() { at -= 1; }
        if at > floor { tail_to = at; break; }
        tail -= 1;
    }

    for (i, (from, to)) in ranges.iter().enumerate().take(tail).skip(head) {
        let from = if i == head { head_from } else { *from };
        let to = if i == tail - 1 { tail_to } else { *to };
        spans.push(((base + from) as u32, (base + to) as u32));
    }
}

// One search per alias over the whole file rather than one per alias per line, which is the shape
// memmem is good at. A hit counts only if it lies entirely inside one stretch of code, and its
// neighbours are read inside that same stretch, so what a string literal removed is not treated as
// touching what follows it.
fn count_keywords(contents: &str, spans: &[(u32, u32)], matcher: &KeywordMatcher,
    file_stats: &mut FileStats, indices: &mut Vec<usize>)
{
    // The two sides are different questions and '(' is where they part. After the word it opens an
    // argument list and belongs to the declaration: Delphi's 'TFoo = class(TObject)' and Erlang's
    // '-module(greeter).' count as nothing at all if it is refused. Before the word it means the word
    // heads an s-expression, which the alias already handles by including the bracket, as Clojure
    // does with '(defn'; accepting it there too counts '(defn' twice, once through each alias.
    fn is_acceptable_before(byte: Option<&u8>) -> bool {
        match byte {
            None => true,
            Some(b) => *b == b' ' || *b == b'}' || *b == b'{' || *b == b','
        }
    }

    fn is_acceptable_after(byte: Option<&u8>) -> bool {
        matches!(byte, Some(b'(')) || is_acceptable_before(byte)
    }

    if spans.is_empty() { return; }
    let bytes = contents.as_bytes();

    for (alias_finder, alias_len, keyword_index) in &matcher.aliases_with_indices {
        indices.clear();
        indices.extend(alias_finder.find_iter(bytes));
        if indices.is_empty() { continue; }

        // Indices directly next to each other are one hit
        let mut counter = 0;
        while !indices.is_empty() && counter < indices.len()-1 {
            if indices[counter] + alias_len == indices[counter+1] {
                indices.remove(counter);
                indices.remove(counter);
            }
            counter += 1;
        }

        // both lists ascend, so the stretch that could hold the next hit is never behind us
        let mut span = 0;
        for at in indices.iter() {
            while span < spans.len() && (spans[span].1 as usize) <= *at { span += 1; }
            if span == spans.len() { break; }

            let (from, to) = (spans[span].0 as usize, spans[span].1 as usize);
            if *at < from || at + alias_len > to { continue; }

            let before = if *at > from { bytes.get(*at - 1) } else { None };
            let after = if at + alias_len < to { bytes.get(at + alias_len) } else { None };
            if is_acceptable_before(before) && is_acceptable_after(after) {
                file_stats.keyword_occurences[*keyword_index] += 1;
            }
        }
    }
}

// Every string symbol the scan found, reduced to the ones that actually open or close a string.
// The number of symbols a language declares is not fixed: the one rule is that only the symbol
// that opened a string can close it, so anything of another kind in between is text. A pair whose
// halves differ splits the rule in two: its opener cannot close and its closer cannot open, so a
// stray '"#' sitting in code is text and not the start of anything.
fn resolve_string_delimiters(language: &Language, open_str_symbol: &Option<u8>, buffers: &mut ScanBuffers) {
    let ScanBuffers { raw_strings, strings, string_symbols, .. } = buffers;

    let mut open = *open_str_symbol;
    let mut consumed_up_to = 0;

    for &(at, symbol, role) in raw_strings.iter() {
        // What sits inside a symbol that was already taken is part of it, not a symbol of its own
        if at < consumed_up_to {
            continue;
        }
        let length = match open {
            Some(open_symbol) => {
                if open_symbol != symbol || role == ROLE_OPEN { continue; }
                open = None;
                language.get_string_pair_of(symbol).1.len()
            }
            None => {
                if role == ROLE_CLOSE { continue; }
                open = Some(symbol);
                language.get_string_pair_of(symbol).0.len()
            }
        };
        consumed_up_to = at + length;
        strings.push(at);
        string_symbols.push(symbol);
    }
}

// When a comment symbol and a multiline start overlap only one of them is real: whichever begins
// first swallows the other, and on a tie the longer one wins. All three shapes occur. A '/*' inside a
// '//' opens nothing. PowerShell's '<#' contains a '#', and reading that as a comment of its own
// stops the block ever opening, which silently breaks every block comment in the language. Lua's
// '--[[' begins exactly where its own '--' does, with the same result if the shorter one wins.
fn resolve_comment_and_multiline_start_overlap(line: &str, language: &Language,
    comment_indices: &mut Vec<usize>, com_start_indices: &mut Vec<(usize, u8, u8)>)
{
    if comment_indices.is_empty() || com_start_indices.is_empty() {
        return;
    }
    let longest_comment_at = |at: usize| {
        language.comment_symbols.iter()
                .filter(|symbol| line.as_bytes()[at..].starts_with(symbol.as_bytes()))
                .map(|symbol| symbol.len())
                .max()
                .unwrap_or(0)
    };

    com_start_indices.retain(|(start, _, _)| !comment_indices.iter()
            .any(|at| start > at && *start < at + longest_comment_at(*at)));
    comment_indices.retain(|at| !com_start_indices.iter()
            .any(|(start, symbol, level)| at > start && *at < start + language.comment_start_len(*symbol, *level)));

    // On a tie the longer symbol wins, and with several pairs the start at that position is the
    // longest of them, since same-position candidates arrive longest first
    comment_indices.retain(|at| match com_start_indices.iter().find(|(start, _, _)| start == at) {
        Some((_, symbol, level)) => longest_comment_at(*at) >= language.comment_start_len(*symbol, *level),
        None => true
    });
    com_start_indices.retain(|(at, _, _)| !comment_indices.contains(at));
}

fn is_intersecting_with_multi_line_end_symbol(index: usize, language: &Language, end_vec: &[(usize, u8, u8)]) -> bool {
    for (i, symbol, level) in end_vec {
        let symbol_len = language.comment_end_len(*symbol, *level);
        if index < symbol_len {
            if *i == 0 {return true;}
        } else {
            if *i == index - symbol_len + 1 {return true;}
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, LazyLock};

    use super::*;
    use crate::{Keyword, MultilineString, Stats};
    use crate::test_paths::{FIXTURES_DIR, LANGUAGES_DIR};
    use crate::engine::identity::{IdentifiedBy, LanguageLookup, build_language_map_by};

    // The four sample files the parser cases below read. They carry no telling extension, because
    // the language is the one the test names and not the one a suffix would imply, which is what
    // lets the same file be counted as Java and then as C#.
    fn sample_file(name: &str) -> std::path::PathBuf {
        Path::new(FIXTURES_DIR).join("parser").join(name)
    }

    // The working memory belongs to the counting thread, so a test that cares about one line gets a
    // fresh one. The parser hands back ranges into the line, and the text a test wants to read is
    // rebuilt from them here.
    #[derive(Debug, PartialEq)]
    struct TextInfo {
        cleansed_string: Option<String>,
        has_string_literal: bool,
        open_comment_after: Option<(u8, u32)>,
        open_str_sybol_after: Option<u8>
    }

    impl TextInfo {
        fn from_slice(slice: &str) -> TextInfo {
            TextInfo { cleansed_string: Some(slice.to_owned()), has_string_literal: false, open_comment_after: None, open_str_sybol_after: None }
        }
        fn from_slice_w_literal(slice: &str) -> TextInfo {
            TextInfo { cleansed_string: Some(slice.to_owned()), has_string_literal: true, open_comment_after: None, open_str_sybol_after: None }
        }
        fn with_open_comment(symbol: u8) -> TextInfo {
            TextInfo { cleansed_string: None, has_string_literal: false, open_comment_after: Some((symbol, 1)), open_str_sybol_after: None }
        }
        fn with_open_comment_at(symbol: u8, depth: u32) -> TextInfo {
            TextInfo { cleansed_string: None, has_string_literal: false, open_comment_after: Some((symbol, depth)), open_str_sybol_after: None }
        }
        fn with_open_symbol(symbol: u8) -> TextInfo {
            TextInfo { cleansed_string: None, has_string_literal: true, open_comment_after: None, open_str_sybol_after: Some(symbol) }
        }
        fn none_all(has_string_literal: bool) -> TextInfo {
            TextInfo { cleansed_string: None, has_string_literal, open_comment_after: None, open_str_sybol_after: None }
        }
        fn new(cleansed_string: Option<String>, has_string_literal: bool, open_comment_after: Option<(u8, u32)>, open_str_sybol_after: Option<u8>) -> TextInfo {
            TextInfo { cleansed_string, has_string_literal, open_comment_after, open_str_sybol_after }
        }
    }

    fn text_of(line: &str, info: LineInfo, buffers: &ScanBuffers) -> TextInfo {
        TextInfo {
            cleansed_string: info.code.map(|(from, to)|
                    buffers.code_ranges[from..to].iter().map(|(a, b)| &line[*a..*b]).collect::<String>()),
            has_string_literal: info.has_string_literal,
            open_comment_after: info.open_comment_after,
            open_str_sybol_after: info.open_str_sybol_after
        }
    }

    fn bounds_multi(line: &str, language: &Language, open_comment: Option<u8>, open_str_symbol: &Option<u8>) -> TextInfo {
        bounds_multi_deep(line, language, open_comment.map(|symbol| (symbol, 1)), open_str_symbol)
    }

    fn bounds_multi_deep(line: &str, language: &Language, open_comment: Option<(u8, u32)>, open_str_symbol: &Option<u8>) -> TextInfo {
        let mut buffers = ScanBuffers::default();
        let info = get_bounds(line, language, open_comment, open_str_symbol, false, &mut buffers);
        text_of(line, info, &buffers)
    }

    fn keywords_of(line: &str, matcher: &KeywordMatcher, file_stats: &mut FileStats) {
        count_keywords(line, &[(0, line.len() as u32)], matcher, file_stats, &mut Vec::new());
    }

    fn str_delimiters(line: &str, language: &Language, open_str_symbol: &Option<u8>) -> (Vec<usize>, Vec<u8>) {
        let mut buffers = ScanBuffers::default();
        scan_line(line, language, &mut buffers);
        resolve_string_delimiters(language, open_str_symbol, &mut buffers);
        (buffers.strings, buffers.string_symbols)
    }

    fn comment_delimiters(line: &str, language: &Language) -> Vec<usize> {
        let mut buffers = ScanBuffers::default();
        scan_line(line, language, &mut buffers);
        buffers.comments
    }

    fn comment_delimiters_w_multiline(line: &str, language: &Language, com_end_indices: &[usize]) -> Vec<usize> {
        let ends = com_end_indices.iter().map(|at| (*at, 0u8, 0u8)).collect::<Vec<_>>();
        let mut buffers = ScanBuffers::default();
        scan_line(line, language, &mut buffers);
        buffers.comments.retain(|x| !is_intersecting_with_multi_line_end_symbol(*x, language, &ends));
        buffers.comments
    }

    static CLASS : LazyLock<Keyword> = LazyLock::new(|| Keyword {
        descriptive_name : "classes".to_owned(),
        aliases : vec!["class".to_owned()]
    });

    static INTERFACE : LazyLock<Keyword> = LazyLock::new(|| Keyword {
        descriptive_name : "interfaces".to_owned(),
        aliases : vec!["interface".to_owned()]
    });

    static ENUM : LazyLock<Keyword> = LazyLock::new(|| Keyword {
        descriptive_name : "enums".to_owned(),
        aliases : vec!["enum".to_owned()]
    });

    static STRUCT : LazyLock<Keyword> = LazyLock::new(|| Keyword {
        descriptive_name : "structs".to_owned(),
        aliases : vec!["struct".to_owned()]
    });

    static TRAIT : LazyLock<Keyword> = LazyLock::new(|| Keyword {
        descriptive_name : "traits".to_owned(),
        aliases : vec!["trait".to_owned()]
    });

    static JAVA : LazyLock<Language> = LazyLock::new(|| Language {
        name : "java".to_owned(),
        extensions : vec!["java".to_owned()],
        filenames : vec![],
        string_symbols : vec!["\"".to_owned()],
        char_literal_symbols : vec![],
        line_continuation : None,
        nested_languages : vec![],
        multiline_strings : vec![],
        comment_symbols : vec!["//".to_owned()],
        multiline_comments : vec![("/*".to_owned(), "*/".to_owned())],
        nesting_comments : vec![],
        leveled_comments : vec![],
        keywords : vec![CLASS.clone(),INTERFACE.clone()],
        scan_plan : std::sync::OnceLock::new()
    });

    static PHP : LazyLock<Language> = LazyLock::new(|| Language {
        name : "PHP".to_owned(),
        extensions : vec!["php".to_owned()],
        filenames : vec![],
        string_symbols : vec!["\"".to_owned(), "'".to_owned()],
        char_literal_symbols : vec![],
        line_continuation : None,
        nested_languages : vec![],
        multiline_strings : vec![],
        comment_symbols : vec!["//".to_owned(),"#".to_owned()],
        multiline_comments : vec![("/*".to_owned(), "*/".to_owned())],
        nesting_comments : vec![],
        leveled_comments : vec![],
        keywords : vec![CLASS.clone()],
        scan_plan : std::sync::OnceLock::new()
    });

    static PYTHON : LazyLock<Language> = LazyLock::new(|| Language {
        name : "py".to_owned(),
        extensions : vec!["py".to_owned()],
        filenames : vec![],
        string_symbols : vec!["\"".to_owned(), "'".to_owned()],
        char_literal_symbols : vec![],
        line_continuation : None,
        nested_languages : vec![],
        multiline_strings : vec![],
        comment_symbols : vec!["#".to_owned()],
        multiline_comments : vec![],
        nesting_comments : vec![],
        leveled_comments : vec![],
        keywords : vec![CLASS.clone()],
        scan_plan : std::sync::OnceLock::new()
    });

    static RUST : LazyLock<Language> = LazyLock::new(|| Language {
        name : "rust".to_owned(),
        extensions : vec!["rs".to_owned()],
        filenames : vec![],
        string_symbols : vec!["\"".to_owned()],
        char_literal_symbols : vec![],
        line_continuation : None,
        nested_languages : vec![],
        multiline_strings : vec![],
        comment_symbols : vec!["//".to_owned()],
        multiline_comments : vec![("/*".to_owned(), "*/".to_owned())],
        nesting_comments : vec![],
        leveled_comments : vec![],
        keywords : vec![STRUCT.clone(),ENUM.clone(),TRAIT.clone()],
        scan_plan : std::sync::OnceLock::new()
    });

    // Four string symbols and three comment ones, past the two of each that most languages declare,
    // and the docstring symbols where python declares them: among the ones that cross lines, which
    // numbers them after the plain quotes.
    static PYTHON_FULL : LazyLock<Language> = LazyLock::new(|| Language {
        name : "py".to_owned(),
        extensions : vec!["py".to_owned()],
        filenames : vec![],
        string_symbols : vec!["\"".to_owned(), "'".to_owned()],
        char_literal_symbols : vec![],
        line_continuation : None,
        nested_languages : vec![],
        multiline_strings : vec![MultilineString::escaping("\"\"\""), MultilineString::escaping("'''")],
        comment_symbols : vec!["#".to_owned(), "//".to_owned(), "--".to_owned()],
        multiline_comments : vec![],
        nesting_comments : vec![],
        leveled_comments : vec![],
        keywords : vec![CLASS.clone()],
        scan_plan : std::sync::OnceLock::new()
    });

    static LANGUAGE_MAP_REF : LazyLock<Arc<HashMap<String,Language>>> = LazyLock::new(||
            Arc::new(crate::languages::keyed_by_name(crate::language_file::parse_languages_in_dir(LANGUAGES_DIR).unwrap().0)));

    static JAVA_MATCHER : LazyLock<KeywordMatcher> = LazyLock::new(|| KeywordMatcher::build(&JAVA).unwrap());

    static NO_EXTENSIONS : LazyLock<HashMap<String, Arc<str>>> = LazyLock::new(HashMap::new);
    static NO_SET_ASIDE : LazyLock<HashMap<String, Language>> = LazyLock::new(HashMap::new);

    // With the real extension map, so a fixture or a stress case whose sections name a language
    // resolves it the way a run does; without priority rules, which no fixture contests
    static SHIPPED_EXTENSIONS : LazyLock<HashMap<String, Arc<str>>> = LazyLock::new(||
            build_language_map_by(IdentifiedBy::Extension, &LANGUAGE_MAP_REF, &HashMap::new(), &HashMap::new()).0);

    fn shipped_lookup() -> NestedLanguageLookup<'static> {
        NestedLanguageLookup { languages: &LANGUAGE_MAP_REF, extension_to_name: &SHIPPED_EXTENSIONS, set_aside: &NO_SET_ASIDE }
    }

    // The whole file as its language's row sees it, which is what these tests always asserted
    fn parse_file_whole(path: &Path, lang_name: &str, buf: &mut String, config: &EngineConfig) -> Result<FileStats, String> {
        parse_file_report(path, lang_name, buf, config).map(FileReport::into_whole)
    }

    fn parse_file_report(path: &Path, lang_name: &str, buf: &mut String, config: &EngineConfig) -> Result<FileReport, String> {
        parse_file(path, lang_name, buf, &mut ParseBuffers::default(), &shipped_lookup(),
                &mut KeywordMatchers::default(), config)
    }

    fn parse_lines_whole(contents: &str, language: &Language) -> FileStats {
        parse_lines(contents, language, &NestedLanguageLookup { languages: &NO_SET_ASIDE,
                extension_to_name: &NO_EXTENSIONS, set_aside: &NO_SET_ASIDE },
                &mut KeywordMatchers::default(), &EngineConfig::default(), &mut ParseBuffers::default()).into_whole()
    }

    // Seeded from the language and then given the one file, which is what a real run does: the seed
    // is what puts a slot in for every keyword the language declares, so one that never occurs still
    // reports its zero instead of being missing.
    fn content_info_of(file: FileStats, lang_name: &str) -> Stats {
        let language = LANGUAGE_MAP_REF.get(lang_name).unwrap();
        let mut stats = Stats::from(language);
        stats.add_file(&file, 0, &language.keywords);
        stats
    }

    #[test]
    fn test_correct_parsing_of_the_sample_files() {
        let mut buf = String::with_capacity(150);

        let mut config = EngineConfig::default();
        let result = parse_file_whole(&sample_file("a.txt"), "Java", &mut buf, &config);
        let result = content_info_of(result.unwrap(), "Java");
        assert_eq!(Stats::new(1, 0, 44, 13, 17, hashmap!("classes".to_owned()=>3,"interfaces".to_owned()=>0)), result);
        buf.clear();
        // The keywords keep their slots and stay at zero, which is what a run produces: the seed
        // comes from the language and not from the file, so hiding them stops the counting and not
        // the language's own list of what it would have counted.
        config.count_keywords = false;
        let result = parse_file_whole(&sample_file("a.txt"), "Java", &mut buf, &config);
        let result = content_info_of(result.unwrap(), "Java");
        assert_eq!(Stats::new(1, 0, 44, 13, 17, hashmap!("classes".to_owned()=>0,"interfaces".to_owned()=>0)), result);
        buf.clear();
        config.count_keywords = true;
        let result = parse_file_whole(&sample_file("a.txt"), "C#", &mut buf, &EngineConfig::default());
        let result = content_info_of(result.unwrap(), "C#");
        assert_eq!(Stats::new(1, 0, 44, 13, 17, hashmap!("structs".to_owned()=>0,"classes".to_owned()=>3,"interfaces".to_owned()=>0)), result);
        buf.clear();
        
        let result = parse_file_whole(&sample_file("d.txt"), "C#", &mut buf, &EngineConfig::default());
        let result = content_info_of(result.unwrap(), "C#");
        assert_eq!(Stats::new(1, 0, 19, 7, 10, hashmap!("structs".to_owned()=>0,"classes".to_owned()=>5,"interfaces".to_owned()=>0)), result);
        buf.clear();
        let result = parse_file_whole(&sample_file("d.txt"), "Java", &mut buf, &EngineConfig::default());
        let result = content_info_of(result.unwrap(), "Java");
        assert_eq!(Stats::new(1, 0, 19, 7, 10, hashmap!("classes".to_owned()=>5,"interfaces".to_owned()=>0)), result);
        buf.clear();

        let result = parse_file_whole(&sample_file("b.txt"), "Java", &mut buf, &EngineConfig::default());
        let result = content_info_of(result.unwrap(), "Java");
        assert_eq!(Stats::new(1, 0, 19, 11, 5, hashmap!("classes".to_owned()=>7,"interfaces".to_owned()=>0)), result);
        buf.clear();

        // The 'class' on the line between two lone apostrophes counts: Python declares its plain
        // quotes single-line, so the quote above it dies at its own line instead of swallowing it
        let result = parse_file_whole(&sample_file("c.txt"), "Python", &mut buf, &EngineConfig::default());
        let result = content_info_of(result.unwrap(), "Python");
        assert_eq!(Stats::new(1, 0, 11, 6, 3, hashmap!("classes".to_owned()=>3)), result);
        buf.clear();
    }

    // That the flag reaches the parser at all, rather than only that it parses from the command line
    // and survives a config file, which is all anything else checks.
    #[test]
    fn braces_as_code_moves_the_no_content_lines_into_code() {
        let mut buf = String::with_capacity(150);
        let path = sample_file("a.txt");
        let count_with = |flag: bool, buf: &mut String| {
            let config = EngineConfig { braces_as_code: flag, ..Default::default() };
            let stats = parse_file_whole(&path, "Java", buf, &config).unwrap();
            (stats.lines, stats.code_lines, stats.comment_lines)
        };

        // a.txt has 10 lines that are nothing but a brace, of which 2 carry a comment as well, and
        // 6 blank ones. The three categories always add up to the total. Those 2 move twice over:
        // the flag makes their brace code, and a line holding code and a comment is code, so they
        // leave the comment count as they enter the code one.
        assert_eq!((44, 13, 17), count_with(false, &mut buf));
        buf.clear();
        assert_eq!((44, 23, 15), count_with(true, &mut buf));
    }

    // A keyword cut in half by a string literal must not count: each surviving stretch is searched
    // where it lies rather than glued to the next one.
    #[test]
    fn a_keyword_split_by_a_string_is_not_a_keyword() {
        let line = "str\"X\"uct a;";
        let mut file_stats = FileStats::with_keywords(&[STRUCT.clone(),ENUM.clone(),TRAIT.clone()]);
        let matcher = KeywordMatcher::build(&RUST).unwrap();
        let mut buffers = ScanBuffers::default();
        let info = get_bounds(line, &RUST, None, &None, false, &mut buffers);
        let mut spans = Vec::new();
        assert!(info.code.is_some());
        push_trimmed_spans(&mut spans, &buffers.code_ranges, line, 0);
        count_keywords(line, &spans, &matcher, &mut file_stats, &mut Vec::new());
        assert_eq!(0, file_stats.keyword_occurences[0]);

        // and the same word, whole, still counts
        let line = "struct a;";
        let mut file_stats = FileStats::with_keywords(&[STRUCT.clone(),ENUM.clone(),TRAIT.clone()]);
        let mut buffers = ScanBuffers::default();
        let info = get_bounds(line, &RUST, None, &None, false, &mut buffers);
        let mut spans = Vec::new();
        assert!(info.code.is_some());
        push_trimmed_spans(&mut spans, &buffers.code_ranges, line, 0);
        count_keywords(line, &spans, &matcher, &mut file_stats, &mut Vec::new());
        assert_eq!(1, file_stats.keyword_occurences[0]);
    }

    #[test]
    fn finds_keywords_correctly() {
        let line = String::from("Hello world!");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        keywords_of(&line, &JAVA_MATCHER, &mut file_stats);
        assert_eq!(make_file_stats(0,0), file_stats);

        let line = String::from("class");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        keywords_of(&line, &JAVA_MATCHER, &mut file_stats);
        assert_eq!(make_file_stats(1,0), file_stats);

        let line = String::from("1class");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        keywords_of(&line, &JAVA_MATCHER, &mut file_stats);
        assert_eq!(make_file_stats(0,0), file_stats);

        let line = String::from("hello class word!");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        keywords_of(&line, &JAVA_MATCHER, &mut file_stats);
        assert_eq!(make_file_stats(1,0), file_stats);

        let line = String::from("class class class");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        keywords_of(&line, &JAVA_MATCHER, &mut file_stats);
        assert_eq!(make_file_stats(3,0), file_stats);

        let line = String::from("classclass");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        keywords_of(&line, &JAVA_MATCHER, &mut file_stats);
        assert_eq!(make_file_stats(0,0), file_stats);

        let line = String::from("hello,class{word!");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        keywords_of(&line, &JAVA_MATCHER, &mut file_stats);
        assert_eq!(make_file_stats(1,0), file_stats);
        
        let line = String::from("classe,");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        keywords_of(&line, &JAVA_MATCHER, &mut file_stats);
        assert_eq!(make_file_stats(0,0), file_stats);
        
        let line = String::from("class interfaceclass classinterface interface");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        keywords_of(&line, &JAVA_MATCHER, &mut file_stats);
        assert_eq!(make_file_stats(1,1), file_stats);
        
        let line = String::from("{class,interface}");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        keywords_of(&line, &JAVA_MATCHER, &mut file_stats);
        assert_eq!(make_file_stats(1,1), file_stats);
        
        let line = String::from("{class.interface}");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        keywords_of(&line, &JAVA_MATCHER, &mut file_stats);
        assert_eq!(make_file_stats(0,0), file_stats);

        // A bracket after the word opens an argument or an inheritance list and belongs to the
        // declaration, which is what Delphi's 'TFoo = class(TObject)' and Erlang's '-module(x).'
        // are. A bracket before it means the word is the head of an s-expression, and accepting
        // that side too would count Clojure's '(defn' twice, once through each of its aliases.
        let line = String::from("TFoo = class(TObject)");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        keywords_of(&line, &JAVA_MATCHER, &mut file_stats);
        assert_eq!(make_file_stats(1,0), file_stats);

        let line = String::from("(class foo)");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        keywords_of(&line, &JAVA_MATCHER, &mut file_stats);
        assert_eq!(make_file_stats(0,0), file_stats);
    }

    fn make_file_stats(class_occurances: usize, interface_occurances: usize) -> FileStats {
        fn get_keyword_map(class_occurances: usize, interface_occurances: usize) -> Vec<usize> {
            vec![class_occurances, interface_occurances]
        }

        FileStats {
            lines: 0,
            code_lines: 0,
            comment_lines: 0,
            keyword_occurences : get_keyword_map(class_occurances, interface_occurances)
        }
    }

    #[test]
    fn get_str_indicies_test() {
        let single_str_opt = &Some(1u8);
        let double_str_opt = &Some(0u8);
        let line = String::from("Hello");
        assert_eq!(Vec::<usize>::new(),str_delimiters(&line, &PYTHON, &None).0);
        let line = String::from("\"Hello\"");
        assert_eq!((vec![0,6],vec![0u8,0u8]),str_delimiters(&line, &PYTHON, &None));
        let line = String::from("\"'\"Hello");
        assert_eq!((vec![0,2],vec![0u8,0u8]),str_delimiters(&line, &PYTHON, &None));
        assert_eq!((vec![1,2],vec![1u8,0u8]),str_delimiters(&line, &PYTHON, single_str_opt));
        assert_eq!((vec![0,1],vec![0u8,1u8]),str_delimiters(&line, &PYTHON, double_str_opt));
        let line = String::from("''\"\"Hello");
        assert_eq!(vec![0,1,2,3],str_delimiters(&line, &PYTHON, &None).0);
        assert_eq!(vec![0,1],str_delimiters(&line, &PYTHON, single_str_opt).0);
        assert_eq!(vec![2,3],str_delimiters(&line, &PYTHON, double_str_opt).0);
        let line = String::from("'\"'\"''\"He'l\"lo");
        assert_eq!(vec![0,2,3,6,9],str_delimiters(&line, &PYTHON, &None).0);
        assert_eq!(vec![0,1,3,4,5,6,11],str_delimiters(&line, &PYTHON, single_str_opt).0);
        assert_eq!(vec![1,2,4,5,9,11],str_delimiters(&line, &PYTHON, double_str_opt).0);
        assert_eq!(vec![1,3,6,11],str_delimiters(&line, &JAVA, double_str_opt).0);
        let line = String::from(r#"\'\\'\\'\\\''"#);
        assert_eq!(vec![4,7,12], str_delimiters(&line, &PYTHON, &None).0);
        assert_eq!(vec![4,7,12], str_delimiters(&line, &PYTHON, single_str_opt).0);
        let line = String::from(r#"["❌🔤","💭🔜","📗","📘",]"#);
        assert!(str_delimiters(&line, &PYTHON, &None).0.len() == 8);
        assert!(str_delimiters(&line, &RUST, double_str_opt).0.len() == 8);
        let line = String::from(r#"[\'⣾\', '⣷', '⣯', '⣟', '⡿']"#); 
        assert!(str_delimiters(&line, &PYTHON, &None).0.len() == 8);
        assert!(str_delimiters(&line, &RUST, &None).0.is_empty());
        let line = String::from(r#"['⣾", '⣷", '⣯"]"#); 
        assert_eq!(vec![1u8,1u8,0u8,0u8],
                str_delimiters(&line, &PYTHON, &None).1);
        let line = String::from(r#"'\'\'\''"#); 
        assert_eq!(vec![0,7], str_delimiters(&line, &PYTHON, &None).0);
        let line = String::from(r#""\"\\"""#); //  """\"""
        assert_eq!(vec![0,5,6], str_delimiters(&line, &RUST, &None).0);
        assert_eq!(vec![0,5,6], str_delimiters(&line, &PYTHON, &None).0);
        let line = String::from(r#"\\\"\"\\""#);
        assert_eq!(vec![8], str_delimiters(&line, &RUST, &None).0);
        assert_eq!(vec![8], str_delimiters(&line, &PYTHON, &None).0);
    }

    // The number of string symbols is not two any more, and the two rules that make more than two
    // work: only the symbol that opened a string closes it, and where two of them start at the same
    // place the longer one wins.
    #[test]
    fn a_language_can_declare_more_than_two_string_symbols() {
        let indices_of = |line: &str| str_delimiters(&String::from(line), &PYTHON_FULL, &None);

        // The third and the fourth symbol are seen at all, which is what the old merge could not do
        assert_eq!(vec![0, 4], indices_of(r#""abc""#).0);
        assert_eq!(vec![0, 4], indices_of(r#"'abc'"#).0);

        // '"""' is one symbol and not three '"', so the docstring opens once and closes once. Its
        // number is 2, since the crossing symbols are numbered after the plain ones.
        let (indices, symbols) = indices_of(r#""""a docstring""""#);
        assert_eq!(vec![0, 14], indices);
        assert_eq!(vec![2u8, 2u8], symbols);

        // Only the symbol that opened closes: the quote of an apostrophe inside a string is text,
        // and so is a '"""' that turns up inside a plain '"'
        assert_eq!(vec![0, 10], indices_of(r#""it's fine""#).0);
        assert_eq!(vec![0, 8], indices_of(r#"'a """ b'"#).0);

        // A line that leaves one open reports its symbol, and the next line closes with that one
        let (indices, symbols) = indices_of(r#"x = """ open"#);
        assert_eq!((vec![4], vec![2u8]), (indices, symbols));
        let open = Some(2u8);
        assert_eq!(vec![5], str_delimiters(&String::from("still\"\"\""), &PYTHON_FULL, &open).0);
    }

    // The bug the general merge fixed on its way in, found by comparing it against the two symbol
    // version it replaced over every line of up to six characters drawn from '"', '\'', '\\' and 'a',
    // in each of the three states a previous line can leave: of 16,383 cases the two disagreed on
    // 642, all of them this one. Inside an open string, the other symbol is text, and it stays text
    // even when every occurrence of the symbol that could close the string is escaped. The old merge
    // had two paths for "only one kind of symbol survived the escaping" and only one of them asked
    // what was open.
    #[test]
    fn the_other_symbol_stays_text_when_the_one_that_could_close_the_string_is_escaped() {
        let open_single = Some(1u8);
        let open_double = Some(0u8);

        // A '"' while a '...' string is open, and the only ''' on the line is escaped
        assert_eq!((vec![], vec![]), str_delimiters(&String::from("\"\\'"), &PYTHON, &open_single));
        assert_eq!((vec![], vec![]), str_delimiters(&String::from("'\\\""), &PYTHON, &open_double));
        assert_eq!((vec![], vec![]), str_delimiters(&String::from("a\"b\\'c"), &PYTHON, &open_single));

        // And the same line closes the string as soon as one unescaped occurrence is there
        assert_eq!(vec![3], str_delimiters(&String::from("\"\\''"), &PYTHON, &open_single).0);
    }

    #[test]
    fn a_language_can_declare_more_than_two_comment_symbols() {
        let indices_of = |line: &str| comment_delimiters(&String::from(line), &PYTHON_FULL);

        assert_eq!(vec![4], indices_of("code# a comment"));
        assert_eq!(vec![4], indices_of("code// a comment"));
        // The third one, which the old merge never looked for
        assert_eq!(vec![4], indices_of("code-- a comment"));
        // All of them on one line, in the order they are written and not in the order they are declared
        assert_eq!(vec![2, 6, 10], indices_of("a --b //c #d"));
    }

    // A block comment whose opening starts with the line comment symbol, which is Lua's shape
    static LUA : LazyLock<Language> = LazyLock::new(|| Language {
        name : "lua".to_owned(),
        extensions : vec!["lua".to_owned()],
        filenames : vec![],
        string_symbols : vec!["\"".to_owned(), "'".to_owned()],
        char_literal_symbols : vec![],
        line_continuation : None,
        nested_languages : vec![],
        multiline_strings : vec![],
        comment_symbols : vec!["--".to_owned()],
        multiline_comments : vec![("--[[".to_owned(), "]]".to_owned())],
        nesting_comments : vec![],
        leveled_comments : vec![],
        keywords : vec![],
        scan_plan : std::sync::OnceLock::new()
    });

    // '--[[' opens a block; it is not a '--' line comment that happens to be followed by brackets.
    // Without the longest-first rule the block never opened and its contents counted as code.
    #[test]
    fn the_longer_symbol_wins_when_a_comment_and_a_block_start_together() {
        // the block opens and stays open
        assert_eq!(TextInfo::with_open_comment(0), bounds_multi("--[[", &LUA, None, &None));
        assert_eq!(TextInfo::with_open_comment(0), bounds_multi("--[[ opening", &LUA, None, &None));
        // and a plain line comment still behaves like one
        assert_eq!(TextInfo::none_all(false), bounds_multi("-- just a comment", &LUA, None, &None));
        // code before the block is kept, the block is not
        assert_eq!(TextInfo::new(Some("x = 1 ".to_owned()), false, Some((0, 1)), None),
                bounds_multi("x = 1 --[[ opens here", &LUA, None, &None));
        // and it closes on ']]'
        assert_eq!(TextInfo::from_slice(" y = 2"), bounds_multi("]] y = 2", &LUA, Some(0), &None));
    }

    // A block comment whose opening holds the line comment symbol inside it, which is PowerShell's shape
    static POWERSHELL : LazyLock<Language> = LazyLock::new(|| Language {
        name : "powershell".to_owned(),
        extensions : vec!["ps1".to_owned()],
        filenames : vec![],
        string_symbols : vec!["\"".to_owned(), "'".to_owned()],
        char_literal_symbols : vec![],
        line_continuation : None,
        nested_languages : vec![],
        multiline_strings : vec![],
        comment_symbols : vec!["#".to_owned()],
        multiline_comments : vec![("<#".to_owned(), "#>".to_owned())],
        nesting_comments : vec![],
        leveled_comments : vec![],
        keywords : vec![],
        scan_plan : std::sync::OnceLock::new()
    });

    // The '#' of '<#' is not a comment of its own. Reading it as one leaves the block closed for the
    // whole file, so every block comment in the language counts as code, in silence.
    #[test]
    fn a_comment_symbol_inside_the_block_opening_belongs_to_the_opening() {
        // the block opens and stays open
        assert_eq!(TextInfo::with_open_comment(0), bounds_multi("<#", &POWERSHELL, None, &None));
        assert_eq!(TextInfo::with_open_comment(0), bounds_multi("<# opening", &POWERSHELL, None, &None));
        // code before it is kept, the block is not
        assert_eq!(TextInfo::new(Some("$x = 1 ".to_owned()), false, Some((0, 1)), None),
                bounds_multi("$x = 1 <# opens here", &POWERSHELL, None, &None));
        // a plain line comment still behaves like one
        assert_eq!(TextInfo::none_all(false), bounds_multi("# just a comment", &POWERSHELL, None, &None));
        // and the block closes on '#>' without its '#' reading as a comment
        assert_eq!(TextInfo::from_slice(" $y = 2"), bounds_multi("#> $y = 2", &POWERSHELL, Some(0), &None));
    }

    // Two block comment pairs at once, which is Pascal's shape ('{ }' beside '(* *)') and D's
    // ('/* */' beside '/+ +/'). The rule is the one strings already follow: only the end of the
    // pair that opened the block closes it, and the other pair's symbols inside it are text.
    static PASCAL : LazyLock<Language> = LazyLock::new(|| Language {
        name : "pascal".to_owned(),
        extensions : vec!["pas".to_owned()],
        filenames : vec![],
        string_symbols : vec!["'".to_owned()],
        char_literal_symbols : vec![],
        line_continuation : None,
        nested_languages : vec![],
        multiline_strings : vec![],
        comment_symbols : vec!["//".to_owned()],
        multiline_comments : vec![("{".to_owned(), "}".to_owned()), ("(*".to_owned(), "*)".to_owned())],
        nesting_comments : vec![],
        leveled_comments : vec![],
        keywords : vec![],
        scan_plan : std::sync::OnceLock::new()
    });

    // D's plain pair beside its nesting one, which is the shape that forces the distinction to be
    // per pair: '/* /* */' is closed in D, '/+ /+ +/' is not
    static D_LANG : LazyLock<Language> = LazyLock::new(|| Language {
        name : "d".to_owned(),
        extensions : vec!["d".to_owned()],
        filenames : vec![],
        string_symbols : vec!["\"".to_owned()],
        char_literal_symbols : vec![],
        line_continuation : None,
        nested_languages : vec![],
        multiline_strings : vec![],
        comment_symbols : vec!["//".to_owned()],
        multiline_comments : vec![("/*".to_owned(), "*/".to_owned())],
        nesting_comments : vec![("/+".to_owned(), "+/".to_owned())],
        leveled_comments : vec![],
        keywords : vec![],
        scan_plan : std::sync::OnceLock::new()
    });

    // Lua's long bracket, one declaration covering '--[[ ]]', '--[=[ ]=]' and every level above:
    // the run of '=' is counted at the opener and only an end with the same count closes.
    static LUA_LEVELED : LazyLock<Language> = LazyLock::new(|| Language::new(
            "lua-leveled", ["lua"], ["\"", "'"], ["--"], &[], [])
            .with_leveled_comments(&[crate::LeveledPair::of("--[=*[", "]=*]").unwrap()]));

    #[test]
    fn a_leveled_pair_closes_only_at_an_end_carrying_the_same_count() {
        // level zero is the plain '--[[ ]]' shape
        assert_eq!(TextInfo::from_slice_w_literal("x = 1  y = "),
                bounds_multi("x = 1 --[[ note ]] y = ''", &LUA_LEVELED, None, &None));
        // a ']]' inside a level-two block is text, and the block closes at ']==]'
        assert_eq!(TextInfo::none_all(false), bounds_multi("--[==[ a ]] b ]==]", &LUA_LEVELED, None, &None));

        // the level crosses lines: a lower end does not close, the matching one does
        assert_eq!(TextInfo::with_open_comment_at(0, 1), bounds_multi("--[=[ open", &LUA_LEVELED, None, &None));
        assert_eq!(TextInfo::with_open_comment_at(0, 1),
                bounds_multi_deep("]] not yet", &LUA_LEVELED, Some((0, 1)), &None));
        assert_eq!(TextInfo::from_slice(" done"),
                bounds_multi_deep("]=] done", &LUA_LEVELED, Some((0, 1)), &None));

        // '--[=' with no second bracket is no opener at all, just a line comment
        assert_eq!(TextInfo::from_slice("x = 1 "), bounds_multi("x = 1 --[= not a block", &LUA_LEVELED, None, &None));

        // level zero crossing lines carries zero, which is a level and not an absence
        assert_eq!(TextInfo::with_open_comment_at(0, 0), bounds_multi("--[[ open", &LUA_LEVELED, None, &None));
        assert_eq!(TextInfo::from_slice(" done"),
                bounds_multi_deep("]] done", &LUA_LEVELED, Some((0, 0)), &None));
        // and a language whose only pair is the leveled one still takes the multiline path
        assert!(LUA_LEVELED.supports_multiline_comments());
    }

    static OCAML : LazyLock<Language> = LazyLock::new(|| Language::new(
            "ocaml-like", ["ml"], [""; 0], [""; 0], &[], [])
            .with_multiline_strings(&["\""])
            .with_nesting_comments(&[("(*", "*)")]));

    #[test]
    fn a_nesting_pair_closes_only_when_as_many_ends_as_starts_have_passed() {
        // one line, both levels open and close, and the code after the outer close is kept
        assert_eq!(TextInfo::from_slice(" d"),
                bounds_multi("(* a (* b *) c *) d", &OCAML, None, &None));
        // the same shape left open carries how deep it went
        assert_eq!(TextInfo::with_open_comment_at(0, 2), bounds_multi("(* one (* two", &OCAML, None, &None));
        // an end on a later line closes one level and the block stays open
        assert_eq!(TextInfo::with_open_comment_at(0, 1),
                bounds_multi_deep("still *) inside", &OCAML, Some((0, 2)), &None));
        // and the last end lets the code after it through
        assert_eq!(TextInfo::from_slice(" x"), bounds_multi_deep("done *) x", &OCAML, Some((0, 1)), &None));
        // a deeper start on a passing line deepens the carried state
        assert_eq!(TextInfo::with_open_comment_at(0, 3),
                bounds_multi_deep("more (* here", &OCAML, Some((0, 2)), &None));
    }

    #[test]
    fn the_plain_pair_of_a_language_does_not_nest_while_its_nesting_pair_does() {
        // D's '/*' still closes at the first '*/', nested-looking or not
        assert_eq!(TextInfo::from_slice(" tail */"),
                bounds_multi("/* a /* b */ tail */", &D_LANG, None, &None));
        // its '/+' counts depth
        assert_eq!(TextInfo::from_slice(" d"), bounds_multi("/+ a /+ b +/ c +/ d", &D_LANG, None, &None));
        assert_eq!(TextInfo::with_open_comment_at(1, 2), bounds_multi("/+ one /+ two", &D_LANG, None, &None));
    }

    #[test]
    fn a_second_comment_pair_opens_and_only_its_own_end_closes_it() {
        // each pair opens and closes on its own
        assert_eq!(TextInfo::none_all(false), bounds_multi("{ comment }", &PASCAL, None, &None));
        assert_eq!(TextInfo::none_all(false), bounds_multi("(* comment *)", &PASCAL, None, &None));
        assert_eq!(TextInfo::from_slice_w_literal("x := 1;  "),
                bounds_multi("x := 1; { note } '4'", &PASCAL, None, &None));

        // the other pair's end inside a block is text, and the block still closes with its own
        assert_eq!(TextInfo::none_all(false), bounds_multi("{ close with *) no, with }", &PASCAL, None, &None));
        assert_eq!(TextInfo::none_all(false), bounds_multi("(* a } inside *)", &PASCAL, None, &None));

        // a block left open remembers which pair opened it
        assert_eq!(TextInfo::with_open_comment(0), bounds_multi("{ open", &PASCAL, None, &None));
        assert_eq!(TextInfo::with_open_comment(1), bounds_multi("(* open", &PASCAL, None, &None));
        // the other pair's end does not close it across lines either
        assert_eq!(TextInfo::with_open_comment(0), bounds_multi("still *) going", &PASCAL, Some(0), &None));
        assert_eq!(TextInfo::with_open_comment(1), bounds_multi("still } going", &PASCAL, Some(1), &None));
        // its own does
        assert_eq!(TextInfo::from_slice(" x := 2;"), bounds_multi("} x := 2;", &PASCAL, Some(0), &None));
        assert_eq!(TextInfo::from_slice(" x := 2;"), bounds_multi("*) x := 2;", &PASCAL, Some(1), &None));

        // one line with both pairs in turn, and the code between them kept
        assert_eq!(TextInfo::from_slice_w_literal("  "),
                bounds_multi("{ a } (* b *) ''", &PASCAL, None, &None));
        // the '{' block swallows a '(*' opener sitting inside it
        assert_eq!(TextInfo::from_slice(" c"), bounds_multi("{ a (* b } c", &PASCAL, None, &None));
    }

    #[test]
    fn the_d_shape_where_both_pairs_share_a_first_byte_still_matches_by_pair() {
        assert_eq!(TextInfo::none_all(false), bounds_multi("/* comment */", &D_LANG, None, &None));
        assert_eq!(TextInfo::none_all(false), bounds_multi("/+ comment +/", &D_LANG, None, &None));
        // '*/' does not close a '/+' block, '+/' does not close a '/*' block. Either way the open
        // pair survives the line, and either kind of pair leaves a comment line behind it.
        assert_eq!(TextInfo::with_open_comment(1), bounds_multi("/+ a */ still open", &D_LANG, None, &None));
        assert_eq!(TextInfo::with_open_comment(0), bounds_multi("/* a +/ still open", &D_LANG, None, &None));
        // and across lines
        assert_eq!(TextInfo::with_open_comment(1), bounds_multi("text */ text", &D_LANG, Some(1), &None));
        assert_eq!(TextInfo::from_slice(" code"), bounds_multi("+/ code", &D_LANG, Some(1), &None));
        // a block of one pair followed by code and a block of the other on one line
        assert_eq!(TextInfo::from_slice(" a  b"), bounds_multi("/* x */ a /+ y +/ b", &D_LANG, None, &None));
    }

    // Strings that open with one symbol and close with another: Rust's raw form and C#'s verbatim
    // form. Their pairs ride beside the ordinary quotes, and inside them nothing escapes.
    static RUST_RAW : LazyLock<Language> = LazyLock::new(|| Language::new(
            "rust-raw", ["rs"], [""; 0], ["//"], &[("/*", "*/")], [])
            .with_multiline_strings(&["\""])
            .with_string_pairs(&[("r#\"", "\"#")]));

    static CSHARP_VERBATIM : LazyLock<Language> = LazyLock::new(|| Language::new(
            "csharp-verbatim", ["cs"], ["\""], ["//"], &[("/*", "*/")], [])
            .with_multiline_strings(&["\"\"\""])
            .with_string_pairs(&[("@\"", "\"")]));

    // The shape of the shipped Rust file: a crossing quote, and the character literal beside it
    static RUST_CHARS : LazyLock<Language> = LazyLock::new(|| Language::new(
            "rust-chars", ["rs"], [""; 0], ["//"], &[("/*", "*/")], [])
            .with_char_literals(&["'"])
            .with_multiline_strings(&["\""]));

    // A character literal that does not close on its own line is not a literal at all, and one
    // that does shields whatever it holds. The first is what keeps a lifetime's lone ' from
    // swallowing the rest of its line, and the second is what keeps the quote of '"' from opening
    // a string that never closes and turning every following line of the file into code.
    #[test]
    fn a_character_literal_pairs_on_its_own_line_or_is_not_a_literal_at_all() {
        // the quote inside the literal opens nothing, so nothing is carried to the next line
        assert_eq!(TextInfo::from_slice_w_literal("let c = ;"),
                bounds_multi("let c = '\"';", &RUST_CHARS, None, &None));
        // a lone ' is a lifetime, not an open literal: the whole line is plain code
        assert_eq!(TextInfo::from_slice("let x: &'a str = y;"),
                bounds_multi("let x: &'a str = y;", &RUST_CHARS, None, &None));
        // two lone ticks on one line do not pair either, because what sits between them is not one
        // character. Without that rule a lifetime pairs with the apostrophe of a word inside a
        // string, the false literal swallows that string's opening quote, and the quote left over
        // carries to the end of the file.
        assert_eq!(TextInfo::from_slice("fn get<'a>(x: &'a str) -> &'a str {"),
                bounds_multi("fn get<'a>(x: &'a str) -> &'a str {", &RUST_CHARS, None, &None));
        assert_eq!(TextInfo::from_slice_w_literal("let msg: &'static str = ;"),
                bounds_multi("let msg: &'static str = \"don't panic\";", &RUST_CHARS, None, &None));
        // and an escape sequence of any length is still one character
        assert_eq!(TextInfo::from_slice_w_literal("let u = ;"),
                bounds_multi("let u = '\\u{1F600}';", &RUST_CHARS, None, &None));
        // escapes inside the literal behave as in any string: '\'' and '\\' close where Rust says
        assert_eq!(TextInfo::from_slice_w_literal("let q = ;"),
                bounds_multi("let q = '\\'';", &RUST_CHARS, None, &None));
        assert_eq!(TextInfo::from_slice_w_literal("let b = ;"),
                bounds_multi("let b = '\\\\';", &RUST_CHARS, None, &None));
        // inside a comment or a string the symbol is not a literal
        assert_eq!(TextInfo::none_all(false), bounds_multi("// don't", &RUST_CHARS, None, &None));
        assert_eq!(TextInfo::from_slice_w_literal("let s = ;"),
                bounds_multi("let s = \"don't\";", &RUST_CHARS, None, &None));
        // and a crossing quote left open from an earlier line is closed by its own symbol, with
        // the literal's halves inside it read as text
        assert_eq!(TextInfo::new(Some(" after".to_owned()), true, None, None),
                bounds_multi("tick ' text\" after", &RUST_CHARS, None, &Some(1)));
    }

    // Searching 'r#"' by its 'r' would visit every 'for' and 'return' in a Rust file and need a
    // second memchr pass per line. The symbol is found by the quote the language already declares,
    // checked backwards, so declaring the pair costs nothing per line: measured same-binary with
    // the pair declared and not, 1.01 ± 0.08 over a Rust-heavy tree.
    #[test]
    fn a_symbol_led_by_a_letter_is_searched_by_a_byte_the_scan_wanted_anyway() {
        let plan = ScanPlan::build(&RUST_RAW);
        assert!(plan.chunks.iter().all(|c| !c.bytes[..c.len as usize].contains(&b'r')),
                "the scan searches for 'r', which floods on ordinary code");
        // '"', '/' and '*' cover everything the language declares, in the one pass it had before
        assert_eq!(1, plan.chunks.len());

        // The byte is chosen from the ones the language already looks for, not from the end of the
        // symbol: anchored on its last punctuation, C++'s 'R"(' would be found by '(', which stands
        // in front of every call in the language.
        let cpp_raw = Language::new("cpp-like", ["cpp"], ["\""], ["//"], &[("/*", "*/")], [])
                .with_string_pairs(&[("R\"(", ")\"")]);
        let plan = ScanPlan::build(&cpp_raw);
        assert!(plan.chunks.iter().all(|c| !c.bytes[..c.len as usize].contains(&b'(')),
                "the opener is searched by '(' in a language made of brackets");
    }

    // The same reasoning reaches a symbol that begins with punctuation nobody else wants. C++'s
    // ')"' would put the bracket that stands in front of every call into the scan, and the quote it
    // also holds is searched anyway, so declaring the raw pair adds no byte and no pass at all.
    #[test]
    fn a_symbol_is_searched_by_a_byte_another_symbol_needs_before_one_of_its_own() {
        let plain = Language::new("cpp-like", ["cpp"], ["\""], ["//"], &[("/*", "*/")], []);
        let with_raw = Language::new("cpp-like", ["cpp"], ["\""], ["//"], &[("/*", "*/")], [])
                .with_string_pairs(&[("R\"(", ")\"")]);

        let bytes_of = |language: &Language| {
            let plan = ScanPlan::build(language);
            let mut bytes = plan.chunks.iter()
                    .flat_map(|c| c.bytes[..c.len as usize].to_vec()).collect::<Vec<u8>>();
            bytes.sort_unstable();
            (bytes, plan.chunks.len())
        };
        assert_eq!(bytes_of(&plain), bytes_of(&with_raw));
        // '*' is dropped rather than kept: '*/' holds the '/' that '//' forces into the scan, and
        // every pointer, product and comment continuation line stops being a candidate position
        assert_eq!((vec![b'"', b'/'], 1), bytes_of(&with_raw));
    }

    // C++'s raw string crosses lines by itself and keeps everything inside it, which is what makes
    // it worth declaring: a file that opens one and writes a quote, a comment opener or a bracket
    // inside it used to count the rest of the line as whatever those symbols said.
    #[test]
    fn a_cpp_raw_string_keeps_the_quotes_and_brackets_inside_it() {
        let cpp = Language::new("cpp-like", ["cpp"], ["\""], ["//"], &[("/*", "*/")], [])
                .with_string_pairs(&[("R\"(", ")\"")]);

        assert_eq!(TextInfo::from_slice_w_literal("auto s = ;"),
                bounds_multi(r#"auto s = R"(say "hi" // and (stay) code)";"#, &cpp, None, &None));
        // a prefixed form is the same three bytes behind a letter or two, so one declaration
        // answers for 'LR"(', 'uR"(' and 'u8R"(' as well
        assert_eq!(TextInfo::from_slice_w_literal("auto s = u8;"),
                bounds_multi(r#"auto s = u8R"(text)";"#, &cpp, None, &None));

        // the closer is the bracket every call in the language ends with, and standing in code with
        // nothing open it is text: this is one ordinary string and not the end of anything
        assert_eq!(TextInfo::from_slice_w_literal("printf();"),
                bounds_multi(r#"printf(")");"#, &cpp, None, &None));

        // left open it reports its own symbol, an ordinary quote cannot close it on the next line,
        // and its own closer can
        assert_eq!(TextInfo::new(Some("auto s = ".to_owned()), true, None, Some(1)),
                bounds_multi(r#"auto s = R"(open"#, &cpp, None, &None));
        assert_eq!(TextInfo::new(None, true, None, Some(1)),
                bounds_multi(r#"still text "quoted" // not a comment"#, &cpp, None, &Some(1)));
        assert_eq!(TextInfo::new(Some(";".to_owned()), true, None, None),
                bounds_multi(r#"done)";"#, &cpp, None, &Some(1)));
    }

    // Rust's shortest raw form is one letter and a quote, so every string ending in 'r' carries
    // what looks like an opener: '"abcr"' must still be one plain string and not an opener inside
    // one. What saves it is that the pair cannot open while another string is open.
    #[test]
    fn a_raw_opener_that_appears_inside_an_ordinary_string_is_text() {
        let rust = Language::new("rust-like", ["rs"], [""; 0], ["//"], &[("/*", "*/")], [])
                .with_multiline_strings(&["\""])
                .with_string_pairs(&[("r\"", "\""), ("r#\"", "\"#")]);

        assert_eq!(TextInfo::from_slice_w_literal("let s = ;"),
                bounds_multi(r#"let s = "abcr";"#, &rust, None, &None));
        // and the raw form itself still opens and closes
        assert_eq!(TextInfo::from_slice_w_literal("let p = ;"),
                bounds_multi(r#"let p = r"C:\temp\";"#, &rust, None, &None));
        // a quote inside the one-letter form does end it, which is what that form means in Rust
        assert_eq!(TextInfo::from_slice_w_literal("let q = ab;"),
                bounds_multi(r#"let q = r"a"ab"b";"#, &rust, None, &None));
    }

    #[test]
    fn a_string_that_opens_with_one_symbol_closes_only_with_its_other_half() {
        // the measured poisoning case: the quotes inside the raw string are text, and the file
        // used to read everything after this line as string content
        assert_eq!(TextInfo::from_slice_w_literal("let a = ; done"),
                bounds_multi(r##"let a = r#"say "hi"#; done"##, &RUST_RAW, None, &None));

        // a raw string left open reports its own symbol, an ordinary quote cannot close it on the
        // next line, and its own closer can
        assert_eq!(TextInfo::new(Some("x = ".to_owned()), true, None, Some(1)),
                bounds_multi(r##"x = r#"open"##, &RUST_RAW, None, &None));
        assert_eq!(TextInfo::new(None, true, None, Some(1)),
                bounds_multi(r#"say "quoted" more"#, &RUST_RAW, None, &Some(1)));
        assert_eq!(TextInfo::none_all(true), bounds_multi(r##"done"#"##, &RUST_RAW, None, &Some(1)));

        // a closer with nothing open is not a delimiter: the quote of '"#"' opens an ordinary
        // string holding a '#', which is what that line means in Rust
        assert_eq!(TextInfo::from_slice_w_literal("let s = ;"),
                bounds_multi(r##"let s = "#";"##, &RUST_RAW, None, &None));
    }

    #[test]
    fn inside_a_two_sided_pair_the_backslash_does_not_escape() {
        // a raw string body ending in a backslash still closes, which the escape rule used to eat
        assert_eq!(TextInfo::from_slice_w_literal("let p = ;"),
                bounds_multi(r##"let p = r#"C:\path\"#;"##, &RUST_RAW, None, &None));
        // while the ordinary quote keeps the escape rule, and the residue stays documented: an
        // escaped quote leaves the plain string open
        assert_eq!(TextInfo::new(Some("let q = ".to_owned()), true, None, Some(0)),
                bounds_multi(r#"let q = "C:\path\";"#, &RUST_RAW, None, &None));

        // C#'s verbatim string closes at the plain quote its pair declares, backslash and all
        assert_eq!(TextInfo::from_slice_w_literal("var s =  + x;"),
                bounds_multi(r#"var s = @"C:\temp\" + x;"#, &CSHARP_VERBATIM, None, &None));
    }

    // One symbol at both ends says nothing about whether a backslash cancels it, and reading it off
    // that shape left every 'C:\' open to the end of the file in Go, Odin and D. The backtick
    // escapes nothing in those three and does escape in a JavaScript template literal, so the two
    // halves of this test are the same line in two languages with opposite right answers.
    #[test]
    fn a_one_sided_form_escapes_or_not_as_the_language_declares_and_not_as_its_shape_suggests() {
        let go = Language::new("go-like", ["go"], ["\""], ["//"], &[("/*", "*/")], [])
                .with_raw_multiline_strings(&["`"]);
        let js = Language::new("js-like", ["js"], ["\""], ["//"], &[("/*", "*/")], [])
                .with_multiline_strings(&["`"]);

        assert_eq!(TextInfo::from_slice_w_literal("var sep = ;"),
                bounds_multi(r"var sep = `C:\`;", &go, None, &None));
        assert_eq!(TextInfo::new(Some("var sep = ".to_owned()), true, None, Some(1)),
                bounds_multi(r"var sep = `C:\`;", &js, None, &None));

        // and the raw form is a string in every other way: its own symbol closes it, an ordinary
        // quote inside it is text, and a comment opener inside it opens nothing
        assert_eq!(TextInfo::new(Some("var s = ".to_owned()), true, None, Some(1)),
                bounds_multi("var s = `open", &go, None, &None));
        assert_eq!(TextInfo::none_all(true), bounds_multi("still \" /* text `", &go, None, &Some(1)));
    }

    // The languages a section can resolve to, keyed the way the real run keys them: definitions by
    // name, and the attribute values by extension.
    fn section_fixture() -> (HashMap<String, Language>, HashMap<String, Arc<str>>) {
        let js = Language::new("JS", ["js"], ["\""], ["//"], &[("/*", "*/")],
                [Keyword { descriptive_name: "functions".to_owned(), aliases: vec!["function".to_owned()] }]);
        let css = Language::new("CSS", ["css"], [""; 0], [""; 0], &[("/*", "*/")], []);
        let languages = crate::languages::keyed_by_name(vec![js, css]);
        let extensions = HashMap::from([("js".to_owned(), Arc::from("JS")), ("css".to_owned(), Arc::from("CSS"))]);
        (languages, extensions)
    }

    fn web_shell() -> Language {
        Language::new("web", ["wbl"], [""; 0], [""; 0], &[("<!--", "-->")], [])
                .with_nested_languages(&[NestedLanguage::of("<script", "</script>", "js"),
                        NestedLanguage::of("<style", "</style>", "css")])
    }

    fn parse_with_sections(contents: &str, shell: &Language,
        languages: &HashMap<String, Language>, extensions: &HashMap<String, Arc<str>>) -> FileReport
    {
        let lookup = NestedLanguageLookup { languages, extension_to_name: extensions, set_aside: &NO_SET_ASIDE };
        parse_lines(contents, shell, &lookup, &mut KeywordMatchers::default(),
                &EngineConfig::default(), &mut ParseBuffers::default())
    }

    #[test]
    fn a_section_is_counted_with_its_own_language_and_the_tag_lines_stay_with_the_shell() {
        let (languages, extensions) = section_fixture();
        let contents = "<p>hello</p>\n<script>\n// a js comment\nvar s = \"x\"; function f() {}\n</script>\n\
<style>\n/* css comment */\n</style>\n<p>bye</p>\n";

        let report = parse_with_sections(contents, &web_shell(), &languages, &extensions);
        assert_eq!((6, 6, 0), (report.shell.lines, report.shell.code_lines, report.shell.comment_lines),
                "the tag lines and the html around them belong to the shell");

        let js = &report.sections[0];
        assert_eq!(("JS", 2, 1, 1), (js.language.as_str(), js.stats.lines, js.stats.code_lines, js.stats.comment_lines));
        assert_eq!(vec![1], js.stats.keyword_occurences, "the js keywords count inside the js section");
        let css = &report.sections[1];
        assert_eq!(("CSS", 1, 0, 1), (css.language.as_str(), css.stats.lines, css.stats.code_lines, css.stats.comment_lines));

        // the bytes of a section are exactly the bytes between its tag lines
        let js_bytes = contents.find("</script>").unwrap() - (contents.find("<script>").unwrap() + "<script>\n".len());
        assert_eq!(js_bytes, js.bytes);
        assert_eq!(contents.lines().count(), report.total_lines(), "a line of the file is counted exactly once");
    }

    // The opener only counts where the shell read it as code: inside a comment or a string of the
    // shell it is text, which is what tokei gets only half right
    #[test]
    fn an_opener_inside_a_comment_or_a_string_of_the_shell_opens_nothing() {
        let (languages, extensions) = section_fixture();
        let report = parse_with_sections("<!-- <script> -->\n<p>x</p>\n", &web_shell(), &languages, &extensions);
        assert!(report.sections.is_empty(), "a tag inside a comment opened a section");
        assert_eq!((2, 1, 1), (report.shell.lines, report.shell.code_lines, report.shell.comment_lines));

        let stringy = Language::new("webstr", ["wbs"], ["\""], [""; 0], &[], [])
                .with_nested_languages(&[NestedLanguage::of("<script", "</script>", "js")]);
        let report = parse_with_sections("x = \"<script>\"\n", &stringy, &languages, &extensions);
        assert!(report.sections.is_empty(), "a tag inside a string opened a section");
    }

    #[test]
    fn the_tag_names_its_language_and_falls_to_the_declared_default_when_it_does_not() {
        let (languages, extensions) = section_fixture();
        let shell = web_shell();

        // 'lang' wins over the region's default, however the value is quoted
        for tag in ["<script lang=\"css\">", "<script lang='css'>", "<script lang=css>"] {
            let contents = format!("{tag}\n/* x */\n</script>\n");
            let report = parse_with_sections(&contents, &shell, &languages, &extensions);
            assert_eq!("CSS", report.sections[0].language, "{tag} did not resolve its language");
        }
        // a mime 'type' names its language after the slash, by extension or by the language's own
        // name, since people write both and only one of the two is an extension
        let report = parse_with_sections("<script type=\"text/js\">\nvar x = 1;\n</script>\n", &shell, &languages, &extensions);
        assert_eq!("JS", report.sections[0].language);
        let report = parse_with_sections("<style lang=\"CSS\">\n.a { color: red; }\n</style>\n", &shell, &languages, &extensions);
        assert_eq!("CSS", report.sections[0].language, "a language's own name was not recognised");
        // a value nobody recognises falls to the default rather than losing the section
        let report = parse_with_sections("<script lang=\"zz\">\nvar x = 1;\n</script>\n", &shell, &languages, &extensions);
        assert_eq!("JS", report.sections[0].language);
        // and 'slang=' is not 'lang='
        let report = parse_with_sections("<script slang=\"css\">\nvar x = 1;\n</script>\n", &shell, &languages, &extensions);
        assert_eq!("JS", report.sections[0].language);
    }

    // HTML reads its tags without regard to case, and a closer written in another case still ends
    // the section it opened
    #[test]
    fn tags_match_in_any_case() {
        let (languages, extensions) = section_fixture();
        let report = parse_with_sections("<SCRIPT>\n// x\n</SCRIPT>\n<p>y</p>\n", &web_shell(), &languages, &extensions);
        assert_eq!(1, report.sections.len(), "an upper case tag was not read as a tag");
        assert_eq!((1, 0, 1), (report.sections[0].stats.lines, report.sections[0].stats.code_lines,
                report.sections[0].stats.comment_lines));

        let contents = "<script>\n// x\n</SCRIPT>\n";
        let report = parse_with_sections(contents, &web_shell(), &languages, &extensions);
        assert_eq!(1, report.sections.len(), "a closer in another case did not end the section");
        let section_from = contents.find("// x").unwrap();
        assert_eq!(contents.find("</SCRIPT>").unwrap() - section_from, report.sections[0].bytes);
    }

    // Nothing forces an opener to be a tag rather than the same word written as text, so a section
    // that never closes is text: without this, one '<script' in a paragraph hands every line under
    // it to another language, and the file it costs is the whole of it
    #[test]
    fn a_section_that_never_closes_stays_with_the_shell() {
        let (languages, extensions) = section_fixture();
        let report = parse_with_sections("<p>x</p>\n<script>\n// one\n// two\n", &web_shell(), &languages, &extensions);
        assert!(report.sections.is_empty(), "an unclosed opener took the rest of the file");
        assert_eq!(4, report.shell.lines);

        // The word has to end where a tag name ends, so a longer word beginning with it is text
        let report = parse_with_sections("<scriptures>\n// one\n</scriptures>\n", &web_shell(), &languages, &extensions);
        assert!(report.sections.is_empty(), "a longer word beginning with the tag opened a section");

        // And the shell keeps reading the lines it kept, with its own symbols
        let report = parse_with_sections("<p>x</p>\n<script>\n<!-- a note -->\n", &web_shell(), &languages, &extensions);
        assert_eq!((3, 2, 1), (report.shell.lines, report.shell.code_lines, report.shell.comment_lines));
    }

    // The three shapes that stay shell whole: a tag split over two lines, a section that opens and
    // closes on one line, and a language the maps cannot answer for
    #[test]
    fn what_cannot_be_a_section_counts_as_the_shell_it_always_was() {
        let (languages, extensions) = section_fixture();
        let report = parse_with_sections("<script\nlang=\"js\">\nvar x = 1;\n</script>\n", &web_shell(), &languages, &extensions);
        assert!(report.sections.is_empty(), "a tag split over two lines opened a section");

        let report = parse_with_sections("<script>var x = 1;</script>\n<p>y</p>\n", &web_shell(), &languages, &extensions);
        assert!(report.sections.is_empty(), "a one line section left the line");
        assert_eq!((2, 2), (report.shell.lines, report.shell.code_lines));

        // A default nothing can answer for, by extension or by name, leaves the section as shell
        // rather than counting it under a language that does not exist
        let unknown = Language::new("web", ["wbl"], [""; 0], [""; 0], &[("<!--", "-->")], [])
                .with_nested_languages(&[NestedLanguage::of("<script", "</script>", "nosuchthing")]);
        let report = parse_with_sections("<script>\nvar x = 1;\n</script>\n", &unknown, &languages, &extensions);
        assert!(report.sections.is_empty(), "a section resolved to a language nothing declares");
        assert_eq!((3, 3), (report.shell.lines, report.shell.code_lines));
    }

    // Two sections of one language add up in one entry, the way the report will show them
    #[test]
    fn two_sections_of_the_same_language_are_one_entry_of_the_report() {
        let (languages, extensions) = section_fixture();
        let contents = "<script>\n// one\n</script>\n<script>\n// two\nvar x = 1;\n</script>\n";
        let report = parse_with_sections(contents, &web_shell(), &languages, &extensions);
        assert_eq!(1, report.sections.len());
        assert_eq!((3, 1, 2), (report.sections[0].stats.lines, report.sections[0].stats.code_lines,
                report.sections[0].stats.comment_lines));
    }

    // A string ends with its line unless its symbol was declared to cross lines, so an unbalanced
    // quote costs one line while a docstring still spans as many as it likes.
    #[test]
    fn an_unbalanced_quote_costs_its_line_and_not_the_rest_of_the_file() {
        let plain = Language::new("py-like", ["py"], ["\"", "'"], ["#"], &[], [])
                .with_multiline_strings(&["\"\"\""]);
        let crossing = Language::new("py-like", ["py"], [""; 0], ["#"], &[], [])
                .with_multiline_strings(&["\"\"\"", "\"", "'"]);
        let contents = "a = \"unbalanced\nb = 1\nc = 2\n# comment\n";

        let stats = parse_lines_whole(contents, &plain);
        assert_eq!((4, 3, 1), (stats.lines, stats.code_lines, stats.comment_lines));
        // declared crossing, everything after the quote is string content and code to the end
        let stats = parse_lines_whole(contents, &crossing);
        assert_eq!((4, 4, 0), (stats.lines, stats.code_lines, stats.comment_lines));

        // and the docstring symbol, which is declared crossing in both, still spans lines
        let doc = "d = \"\"\"docstring\n# still string\n\"\"\"\ne = 1\n# comment\n";
        let stats = parse_lines_whole(doc, &plain);
        assert_eq!((5, 4, 1), (stats.lines, stats.code_lines, stats.comment_lines));
    }

    // Closing used to advance one byte whatever the symbol, so the tail of a closing '"""' leaked
    // into the code text. Nearly invisible while the line counted as code anyway; load bearing the
    // moment a closer has a length of its own.
    #[test]
    fn closing_a_string_advances_past_the_whole_closing_symbol() {
        assert_eq!(TextInfo::from_slice_w_literal("var d =  y"),
                bounds_multi(r#"var d = """doc""" y"#, &CSHARP_VERBATIM, None, &None));
        assert_eq!(TextInfo::from_slice_w_literal("x =  y"),
                bounds_multi(r#"x = """doc""" y"#, &PYTHON_FULL, None, &None));
    }

    static DEFN : LazyLock<Keyword> = LazyLock::new(|| Keyword {
        descriptive_name : "functions".to_owned(),
        aliases : vec!["(defn".to_owned(), "defn".to_owned()]
    });

    static CLOJURE : LazyLock<Language> = LazyLock::new(|| Language {
        name : "clojure".to_owned(),
        extensions : vec!["clj".to_owned()],
        filenames : vec![],
        string_symbols : vec!["\"".to_owned()],
        char_literal_symbols : vec![],
        line_continuation : None,
        nested_languages : vec![],
        multiline_strings : vec![],
        comment_symbols : vec![";".to_owned()],
        multiline_comments : vec![],
        nesting_comments : vec![],
        leveled_comments : vec![],
        keywords : vec![DEFN.clone()],
        scan_plan : std::sync::OnceLock::new()
    });

    // '(' is not an accepted boundary, so the Lisp family's '(defn' counts zero through a bare alias.
    // The bracket therefore belongs to the alias, and the two forms are declared together. They can
    // never double count: wherever '(defn' matches, the bare 'defn' sits one byte later with the
    // bracket before it and is rejected, so exactly one of the pair fires.
    #[test]
    fn a_bracketed_alias_counts_once_and_never_twice() {
        let matcher = KeywordMatcher::build(&CLOJURE).unwrap();
        let count_of = |line: &str| {
            let mut file_stats = FileStats::with_keywords(std::slice::from_ref(&DEFN));
            keywords_of(line, &matcher, &mut file_stats);
            file_stats.keyword_occurences[0]
        };

        assert_eq!(1, count_of("(defn foo [x] x)"));
        assert_eq!(1, count_of("  (defn foo [x] x)"));
        assert_eq!(1, count_of("(do (defn foo))"));
        // the bare form still counts where nothing precedes it
        assert_eq!(1, count_of("defn"));
        assert_eq!(1, count_of("defn foo"));
        // and neither form fires on a longer word
        assert_eq!(0, count_of("(defnx foo)"));
        assert_eq!(0, count_of("(mydefn foo)"));
    }

    // Every symbol of a kind has to be searched in the same pass, otherwise its positions would not
    // come out in the order they appear on the line and the merge below would read them out of turn.
    #[test]
    fn every_kind_is_searched_whole_in_a_single_pass() {
        for language in [&*JAVA, &*RUST, &*PHP, &*PYTHON, &*PYTHON_FULL, &*PASCAL, &*D_LANG, &*LUA, &*POWERSHELL] {
            let plan = ScanPlan::build(language);
            let searched = |byte: u8| plan.chunks.iter().filter(|c| c.bytes[..c.len as usize].contains(&byte)).count();
            // The byte a symbol is found by, which is its first only until it is anchored on another
            let bytes_of = |kind: u8| plan.slots.iter().enumerate().filter(|(_, slot)| slot.kind == kind)
                    .map(|(at, slot)| plan.symbols[at][slot.anchor as usize]).collect::<Vec<u8>>();

            for kind in [STRINGS, COMMENTS, COM_STARTS, COM_ENDS] {
                let bytes = bytes_of(kind);
                if bytes.is_empty() { continue; }
                let holding = plan.chunks.iter()
                        .filter(|c| bytes.iter().any(|b| c.bytes[..c.len as usize].contains(b)))
                        .count();
                assert_eq!(1, holding, "{} splits a kind across passes", language.name);
                // and no byte is looked for twice, which would report the same symbol from two passes
                for byte in bytes {
                    assert_eq!(1, searched(byte), "{} searches a byte twice", language.name);
                }
            }
        }
    }

    #[test]
    fn a_language_is_scanned_in_as_few_passes_as_its_first_bytes_allow() {
        // '"', '/' and '*' cover the string, the comment and both multiline symbols
        assert_eq!(1, ScanPlan::build(&JAVA).chunks.len());
        // '"', '\'' and '#' cover everything python declares
        assert_eq!(1, ScanPlan::build(&PYTHON).chunks.len());
        // php needs '"', '\'', '#', '/' and '*', which is two passes and not five
        assert_eq!(2, ScanPlan::build(&PHP).chunks.len());
    }

    // One pass per symbol never let a symbol overlap itself, and the scan has to behave the same:
    // the candidate positions of '/' in "///" are 0, 1 and 2, but only 0 begins a comment.
    #[test]
    fn a_symbol_does_not_overlap_itself() {
        assert_eq!(vec![0], comment_delimiters("///", &JAVA));
        assert_eq!(vec![0], comment_delimiters("//", &JAVA));
        assert_eq!(vec![0, 2], comment_delimiters("////", &JAVA));
        assert_eq!(vec![1], comment_delimiters("a///", &JAVA));

        // the same for a string symbol longer than one byte: six quotes are two '"""' and not four,
        // and five are one '"""' that never closes
        assert_eq!(vec![0, 3], str_delimiters(&"\"".repeat(6), &PYTHON_FULL, &None).0);
        assert_eq!(vec![0], str_delimiters(&"\"".repeat(5), &PYTHON_FULL, &None).0);
    }

    // The whole point of replacing str::lines() is that nothing about the lines changes, so the
    // standard library is the oracle: every shape that behaves differently at the end of a file.
    #[test]
    fn the_line_iterator_agrees_with_the_standard_library() {
        let cases = ["", "\n", "\n\n", "a", "a\n", "a\nb", "a\nb\n", "a\r\nb", "a\r\n",
                     "a\r\r\nb", "a\rb", "\r\n", "  \n\t\n", "one\ntwo\nthree",
                     "fn main() {\n    println!(\"hi\");\n}\n", "αβ\nγ"];
        for case in cases {
            let expected = case.lines().collect::<Vec<&str>>();
            let actual = get_lines_of(case).map(|(_, line)| line).collect::<Vec<&str>>();
            assert_eq!(expected, actual, "disagreed on {case:?}");
        }
    }

    // The resolution reads a symbol identity beside every position; the cases here are all the one
    // '/*' '*/' pair, so the helper pins the identity to 0 and the assertions stay bare positions.
    fn resolved_double_counting(start_indices: Vec<usize>, end_indices: Vec<usize>, is_comment_open: bool)
    -> (Vec<usize>, Vec<usize>) {
        let language = Language::new("one-pair", ["x"], ["\""], ["//"], &[("/*", "*/")], []);
        let mut starts = start_indices.into_iter().map(|x| (x, 0u8, 0u8)).collect::<Vec<_>>();
        let mut ends = end_indices.into_iter().map(|x| (x, 0u8, 0u8)).collect::<Vec<_>>();
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut starts, &mut ends, is_comment_open, &language);
        (starts.into_iter().map(|(x, _, _)| x).collect(), ends.into_iter().map(|(x, _, _)| x).collect())
    }

    #[test]
    fn double_counting_resolution() {
        // /*Hello*//* world*//*
        assert_eq!((vec![0,9,19],vec![7,17]), resolved_double_counting(vec![0,9,19], vec![7,17], false));
        // /**//**/
        assert_eq!((vec![0,4],vec![2,6]), resolved_double_counting(vec![0,4], vec![2,6], false));
        // /*/**/*/
        assert_eq!((vec![0,2],vec![4,6]), resolved_double_counting(vec![0,2], vec![4,6], false));

        // /* */*
        assert_eq!((vec![0],vec![3]), resolved_double_counting(vec![0,4], vec![3], false));

        // */* /*/
        assert_eq!((vec![1],vec![5]), resolved_double_counting(vec![1,4], vec![0,5], false));
        assert_eq!((vec![4],vec![0]), resolved_double_counting(vec![1,4], vec![0,5], true));

        // /*/*/ */*/ /* */
        assert_eq!((vec![0,7,11],vec![3,14]), resolved_double_counting(vec![0,2,7,11], vec![1,3,6,8,14], false));
        assert_eq!((vec![7,11],vec![1,3,14]), resolved_double_counting(vec![0,2,7,11], vec![1,3,6,8,14], true));

        // /*/*/ */*/
        assert_eq!((vec![0,7],vec![3]), resolved_double_counting(vec![0,2,7], vec![1,3,6,8], false));
        assert_eq!((vec![7],vec![1,3]), resolved_double_counting(vec![0,2,7], vec![1,3,6,8], true));

        // '*/ */*' with a comment open from the line before, which is the case that decides the two
        // conditions in the loop below 'resolve_collision'. They are not mirror images of each other,
        // and the one that looks like a typo is the one that is right: the end symbol at 0 closes the
        // comment, so the '*/' at 3 is a stray in code and the '/*' at 4 is a real opener. Reading the
        // second condition as the mirror of the first discards the opener instead of the stray, and
        // the whole rest of the file is then counted as code.
        assert_eq!((vec![4],vec![0]), resolved_double_counting(vec![4], vec![0,3], true));

        // /* */*/*//*
        assert_eq!((vec![0,6,9],vec![3]), resolved_double_counting(vec![0,4,6,9], vec![3,5,7], false));
        assert_eq!((vec![0,6,9],vec![3]), resolved_double_counting(vec![0,4,6,9], vec![3,5,7], true));
    }

    // The collision window around each symbol is that symbol's own span. Lua's pair is 4 bytes on
    // one side and 2 on the other, and one shared window saw a collision in ']]--[[' where the two
    // symbols merely touch: the reopening start was discarded and the rest of the file counted as
    // code. Measured on the v3.0.0 binary: a 7-line file whose honest counts are code=1 comments=6
    // reported code=3 comments=2.
    #[test]
    fn a_close_that_touches_a_reopen_is_not_a_collision_when_the_lengths_differ() {
        let lua_like = Language::new("lua-like", ["x"], ["\""], ["--"], &[("--[[", "]]")], []);
        // ]]--[[ with the block open from the line before: both symbols are real
        let (mut starts, mut ends) = (vec![(2usize, 0u8, 0u8)], vec![(0usize, 0u8, 0u8)]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut starts, &mut ends, true, &lua_like);
        assert_eq!((vec![(2, 0, 0)], vec![(0, 0, 0)]), (starts, ends));

        // and the whole shape through the walk: the line closes and reopens, so it ends still open,
        // and it is a comment line whether or not whitespace separates the two symbols, since the
        // space between them is nothing a reader would call code
        assert_eq!(TextInfo::with_open_comment(0), bounds_multi("]]--[[", &LUA, Some(0), &None));
        assert_eq!(TextInfo::with_open_comment(0), bounds_multi("]]  --[[ reopened", &LUA, Some(0), &None));
        // an HTML-shaped pair, 4 against 3, through a language declaring no line comments
        let html : Language = Language::new("html-like", ["html"], ["\""], [""; 0], &[("<!--", "-->")], []);
        assert_eq!(TextInfo::with_open_comment(0), bounds_multi("--><!--", &html, Some(0), &None));
        assert_eq!(TextInfo::with_open_comment(0), bounds_multi("--> <!-- reopened", &html, Some(0), &None));
    }

    #[test]
    fn test_find_comment_indicies() {
        let line = "";
        assert_eq!(Vec::<usize>::new(), comment_delimiters(line, &PHP));
        let line = "Hello world!";
        assert_eq!(Vec::<usize>::new(), comment_delimiters(line, &PHP));
        let line = "//Hello world!";
        assert_eq!(vec![0], comment_delimiters(line, &PHP));
        let line = "////Hello world!";
        assert_eq!(vec![0,2], comment_delimiters(line, &PHP));
        let line = "//#//#Hello world!";
        assert_eq!(vec![0,2,3,5], comment_delimiters(line, &PHP));
        let line = "//Hello# world!";
        assert_eq!(vec![0,7], comment_delimiters(line, &PHP));

        let line = "Hello world!";
        assert_eq!(Vec::<usize>::new(), comment_delimiters_w_multiline(line, &PHP, &[]));
        let line = "//Hello*/ world!";
        assert_eq!(vec![0], comment_delimiters_w_multiline(line, &PHP, &[7]));
        let line = "///*Hello world!";
        assert_eq!(vec![0], comment_delimiters_w_multiline(line, &PHP, &[]));
        let line = "//*//Hello world!";
        assert_eq!(vec![0], comment_delimiters_w_multiline(line, &PHP, &[2]));
        let line = "//*/#Hello world!";
        assert_eq!(vec![0,4], comment_delimiters_w_multiline(line, &PHP, &[2]));
    }
    
    #[test]
    fn gets_bounds_PYTHON() {
        let line = String::from("[\"\\\"\\\"\\\"\",\"'''\",\"\\\"\",\"'\",]");
        assert_eq!(TextInfo::new(Some("[,,,,]".to_owned()),true,None,None),bounds_multi(&line, &PYTHON, None,&None));
        let line = String::from("\\''\''");
        assert_eq!(TextInfo::new(Some("\\\'".to_owned()),true,None,Some(1u8)), bounds_multi(&line, &PYTHON, None,&None));
        assert_eq!(TextInfo::none_all(true), bounds_multi(&line, &PYTHON, None,&Some(1u8)));
        let line = String::from("\'\\'\\'\\\''"); 
        assert_eq!(TextInfo::new(None,true,None,None), bounds_multi(&line, &PYTHON, None,&None));
        
        let single_str_opt = &Some(1u8);
        let double_str_opt = &Some(0u8);
        let single_str_li = TextInfo::with_open_symbol(1);
        let double_str_li = TextInfo::with_open_symbol(0);
    
        let line = String::from("Hello world!");
        assert_eq!(TextInfo::from_slice("Hello world!"),bounds_multi(&line, &PYTHON, None,&None));
        assert_eq!(single_str_li,bounds_multi(&line, &PYTHON, None,single_str_opt));
        
        //testing comments
        let line = String::from("#Hello world!");
        assert_eq!(single_str_li,bounds_multi(&line, &PYTHON, None,single_str_opt));
        let line = String::from("Hello world!#");
        assert_eq!(TextInfo::from_slice("Hello world!"),bounds_multi(&line, &PYTHON, None,&None));
        let line = String::from("Hello# world!");
        assert_eq!(TextInfo::from_slice("Hello"),bounds_multi(&line, &PYTHON, None,&None));
        assert_eq!(single_str_li,bounds_multi(&line, &PYTHON, None,single_str_opt));
        let line = String::from("Hello## world!");
        assert_eq!(TextInfo::from_slice("Hello"),bounds_multi(&line, &PYTHON, None,&None));
        let line = String::from("#Hello# world!");
        assert_eq!(single_str_li,bounds_multi(&line, &PYTHON, None,single_str_opt));
        
        //testing strings 
        let line = String::from("\"Hello world!#");
        assert_eq!(double_str_li,bounds_multi(&line, &PYTHON, None,&None));
        let line = String::from("\"Hello\" world!");
        assert_eq!(TextInfo::from_slice_w_literal(" world!"),bounds_multi(&line, &PYTHON, None,&None));
        assert_eq!(TextInfo::new(Some("Hello".to_owned()), true, None, Some(0u8)),bounds_multi(&line, &PYTHON, None,double_str_opt));
        let line = String::from("Hello world!\"");
        assert_eq!(TextInfo::new(Some("Hello world!".to_owned()), true, None, Some(0u8)),bounds_multi(&line, &PYTHON, None,&None));
        let line = String::from("\"'Hello'\" world!");
        assert_eq!(TextInfo::from_slice_w_literal(" world!"),bounds_multi(&line, &PYTHON, None,&None));
        let line = String::from("'Hello' world!");
        assert_eq!(TextInfo::from_slice_w_literal(" world!"),bounds_multi(&line, &PYTHON, None,&None));
        let line = String::from("'\"He'llo'\" world!'");
        assert_eq!(TextInfo::from_slice_w_literal("llo"),bounds_multi(&line, &PYTHON, None,&None));
        assert_eq!(TextInfo::new(Some("He".to_owned()), true, None, Some(0u8)),bounds_multi(&line, &PYTHON, None,double_str_opt));
        let line = String::from(r#""""Hello""#);
        assert_eq!(TextInfo::new(None, true, None, None), bounds_multi(&line, &PYTHON, None,&None));
        assert_eq!(TextInfo::new(Some("Hello".to_owned()), true, None, Some(0u8)), bounds_multi(&line, &PYTHON, None,double_str_opt));
        let line = String::from(r#"['⣯', '⣟"#); 
        assert_eq!(TextInfo::new(Some("[, ".to_owned()),true,None,Some(1u8)), bounds_multi(&line, &PYTHON, None,&None));
        
        //test mixed
        let line = String::from("'Hello#' world!'");
        assert_eq!(TextInfo::new(Some(" world!".to_owned()), true, None, Some(1u8)),bounds_multi(&line, &PYTHON, None,&None));
        assert_eq!(TextInfo::from_slice_w_literal("Hello"),bounds_multi(&line, &PYTHON, None,single_str_opt));
        let line = String::from("'Hello'# world!'");
        assert_eq!(TextInfo::none_all(true),bounds_multi(&line, &PYTHON, None,&None));
        assert_eq!(TextInfo::from_slice_w_literal("Hello"),bounds_multi(&line, &PYTHON, None,single_str_opt));
        let line = String::from("''#Hello");
        assert_eq!(TextInfo::none_all(true),bounds_multi(&line, &PYTHON, None,&None));
        let line = String::from("'''#'''Hello world!'");
        assert_eq!(TextInfo::new(Some("Hello world!".to_owned()), true, None, Some(1u8)),bounds_multi(&line, &PYTHON, None,&None));
        assert_eq!(TextInfo::none_all(true),bounds_multi(&line, &PYTHON, None,single_str_opt));
        assert_eq!(TextInfo::with_open_symbol(0),bounds_multi(&line, &PYTHON, None,double_str_opt));
        let line = String::from("Hello'###'\"world!\"");
        assert_eq!(TextInfo::from_slice_w_literal("Hello"),bounds_multi(&line, &PYTHON, None,&None));
        assert_eq!(TextInfo::none_all(true),bounds_multi(&line, &PYTHON, None,single_str_opt));
        assert_eq!(TextInfo::new(Some("world!".to_owned()), true, None, Some(0u8)),bounds_multi(&line, &PYTHON, None,double_str_opt));
        let line = String::from("\"//'''\"Hello'\"world!");
        assert_eq!(TextInfo::new(Some("Hello".to_owned()), true, None, Some(1u8)),bounds_multi(&line, &PYTHON, None,&None));
        assert_eq!(TextInfo::from_slice_w_literal("world!"),bounds_multi(&line, &PYTHON, None,single_str_opt));
        assert_eq!(TextInfo::new(Some("//".to_owned()), true, None, Some(0u8)),bounds_multi(&line, &PYTHON, None,double_str_opt));
    }
    
    #[test]
    fn gets_bounds_JAVA() {
        let double_str_opt = &Some(0u8);

        let line = String::from("Hello world!");
        assert_eq!(TextInfo::with_open_comment(0),bounds_multi(&line, &JAVA, Some(0), &None));
        assert_eq!(TextInfo::with_open_symbol(0),bounds_multi(&line, &JAVA, None, double_str_opt));
        assert_eq!(TextInfo::from_slice("Hello world!"),bounds_multi(&line, &JAVA, None, &None));
        
        //testing only multiline comment combinations
        let line = String::from("*/Hello world!");
        assert_eq!(TextInfo::from_slice("Hello world!"),bounds_multi(&line, &JAVA, Some(0), &None));
        assert_eq!(TextInfo::from_slice("*/Hello world!"),bounds_multi(&line, &JAVA, None, &None));
        let line = String::from("Hello/* ffd /**//*erer */ world!");
        assert_eq!(TextInfo::from_slice(" world!"),bounds_multi(&line, &JAVA, Some(0), &None));
        assert_eq!(TextInfo::from_slice("Hello world!"),bounds_multi(&line, &JAVA, None, &None));
        let line = String::from("Hello*//**//**/ world!");
        assert_eq!(TextInfo::from_slice(" world!"),bounds_multi(&line, &JAVA, Some(0), &None));
        assert_eq!(TextInfo::from_slice("Hello*/ world!"),bounds_multi(&line, &JAVA, None, &None));
        let line = String::from("*//*Hello/**/ world!");
        assert_eq!(TextInfo::from_slice(" world!"),bounds_multi(&line, &JAVA, Some(0), &None));
        assert_eq!(TextInfo::from_slice("*/ world!"),bounds_multi(&line, &JAVA, None, &None));
        let line = String::from("Hello world*/");
        assert_eq!(TextInfo::none_all(false), bounds_multi(&line, &JAVA, Some(0), &None));
        let line = String::from("*/Hello world!/**/");
        assert_eq!(TextInfo::from_slice("Hello world!"), bounds_multi(&line, &JAVA, Some(0), &None));
        let line = String::from("Hello world*//**/");
        assert_eq!(TextInfo::none_all(false), bounds_multi(&line, &JAVA, Some(0), &None));
        let line = String::from("*/He/**//*llo world*/!/**/");
        assert_eq!(TextInfo::from_slice("He!"), bounds_multi(&line, &JAVA, Some(0), &None));
        let line = String::from("Hello world*/!");
        assert_eq!(TextInfo::from_slice("!"), bounds_multi(&line, &JAVA, Some(0), &None));
        let line = String::from("/*H*/ello world/*!");
        assert_eq!(TextInfo::new(Some("ello world".to_string()), false, Some((0, 1)), None), bounds_multi(&line, &JAVA, Some(0), &None));
        assert_eq!(TextInfo::new(Some("ello world".to_string()), false, Some((0, 1)), None), bounds_multi(&line, &JAVA, None, &None));
        let line = String::from("/*H*/e/*llo world!");
        assert_eq!(TextInfo::new(Some("e".to_string()), false, Some((0, 1)), None), bounds_multi(&line, &JAVA, Some(0), &None));
        
        //testing only string symbols
        let line = String::from("\"");
        assert_eq!(TextInfo::with_open_symbol(0), bounds_multi(&line, &JAVA, None, &None));
        let line = String::from("\"Hello\"");
        assert_eq!(TextInfo::new(Some("Hello".to_string()), true, None, Some(0u8)), bounds_multi(&line, &JAVA, None, double_str_opt));
        assert_eq!(TextInfo::none_all(true), bounds_multi(&line, &JAVA, None, &None));
        let line = String::from("\"\"Hello");
        assert_eq!(TextInfo::with_open_symbol(0), bounds_multi(&line, &JAVA, None, double_str_opt));
        assert_eq!(TextInfo::from_slice_w_literal("Hello"), bounds_multi(&line, &JAVA, None, &None));
        let line = String::from("\"\"");
        assert_eq!(TextInfo::with_open_symbol(0), bounds_multi(&line, &JAVA, None, double_str_opt));
        assert_eq!(TextInfo::none_all(true), bounds_multi(&line, &JAVA, None, &None));
        let line = String::from("\"\"Hello");
        assert_eq!(TextInfo::from_slice_w_literal("Hello"), bounds_multi(&line, &JAVA, None, &None));
        let line  = String::from("Hel\"\"lo");
        assert_eq!(TextInfo::from_slice_w_literal("Hello"), bounds_multi(&line, &JAVA, None, &None));
        let line = String::from("\"\"He\"\"\"ll\"o");
        assert_eq!(TextInfo::from_slice_w_literal("Heo"), bounds_multi(&line, &JAVA, None, &None));
        let line = String::from(r#""""Hello""#);
        assert_eq!(TextInfo::new(None, true, None, None), bounds_multi(&line, &JAVA, None, &None));
        assert_eq!(TextInfo::new(Some("Hello".to_owned()), true, None, Some(0u8)), bounds_multi(&line, &JAVA, None, double_str_opt));
        
        //testing only comments
        let line = String::from("//");
        assert_eq!(TextInfo::none_all(false), bounds_multi(&line, &JAVA, None, &None));
        let line = String::from("Hello//");
        assert_eq!(TextInfo::from_slice("Hello"), bounds_multi(&line, &JAVA, None, &None));
        assert_eq!(TextInfo::with_open_comment(0), bounds_multi(&line, &JAVA, Some(0), &None));
        assert_eq!(TextInfo::with_open_symbol(0), bounds_multi(&line, &JAVA, None, double_str_opt));
        let line = String::from("//Hello");
        assert_eq!(TextInfo::none_all(false), bounds_multi(&line, &JAVA, None, &None));
        let line = String::from("////Hello");
        assert_eq!(TextInfo::none_all(false), bounds_multi(&line, &JAVA, None, &None));
        let line = String::from("He//llo//");
        assert_eq!(TextInfo::from_slice("He"), bounds_multi(&line, &JAVA, None, &None));
        
        //testing mixed
        let line = String::from("\"\"\"//\"\"\"Hello world!");
        assert_eq!(TextInfo::from_slice_w_literal("Hello world!"),bounds_multi(&line, &JAVA, None, &None));
        assert_eq!(TextInfo::none_all(true),bounds_multi(&line, &JAVA, None, double_str_opt));
        let line = String::from("\"\"one\"//\"\"\"Hello world!");
        assert_eq!(TextInfo::from_slice_w_literal("oneHello world!"),bounds_multi(&line, &JAVA, None, &None));
        let line = String::from("\"He\"/*l*/lo//fd");
        assert_eq!(TextInfo::from_slice_w_literal("lo"), bounds_multi(&line, &JAVA, None, &None));
        assert_eq!(TextInfo::new(Some("He".to_string()), true, None, Some(0u8)), bounds_multi(&line, &JAVA, None, double_str_opt));
        assert_eq!(TextInfo::from_slice("lo"), bounds_multi(&line, &JAVA, Some(0), &None));
        let line = String::from("//\"/**/dfd\"");
        assert_eq!(TextInfo::none_all(false), bounds_multi(&line, &JAVA, None, &None));
        assert_eq!(TextInfo::new(Some("dfd".to_string()), true, None, Some(0u8)), bounds_multi(&line, &JAVA, Some(0), &None));
        assert_eq!(TextInfo::new(Some("dfd".to_string()), true, None, Some(0u8)), bounds_multi(&line, &JAVA, None, double_str_opt));
        
        let line  = String::from(
            "Hello /* \
            mefm \" */ \" \
            //*/world!"
        );
        assert_eq!(TextInfo::new(Some("Hello  ".to_string()), true, None, Some(0u8)), bounds_multi(&line, &JAVA, None, &None));
        assert_eq!(TextInfo::new(Some(" ".to_string()), true, None, Some(0u8)), bounds_multi(&line, &JAVA, Some(0), &None));
        assert_eq!(TextInfo::new(Some(" */ ".to_string()), true, None, Some(0u8)), bounds_multi(&line, &JAVA, None, double_str_opt));
    }

    const MARKER: &str = "mezura-expect";
    const LANGUAGE_FIELD: &str = "language=";

    fn fixtures_dir() -> std::path::PathBuf {
        Path::new(FIXTURES_DIR).join("lang")
    }

    // Each fixture declares, on its first line and in its own comment syntax, the counts mezura must
    // produce for it. The counts are hand-verified, so a mismatch means either the parser regressed
    // or the fixture is wrong; both are worth stopping for. The header line itself is a comment, so
    // it is included in 'lines' and excluded from 'code'.
    // The counts, and the language the fixture says it is. Naming the language is only for an
    // extension two of them claim, as MATLAB and Objective-C both claim '.m': the lookup would
    // answer with the tie-break rule and the counts would be that rule's rather than the parser's.
    // It comes last on the line because a language name can hold a space.
    fn parse_expectations(first_line: &str) -> Option<(Option<String>, HashMap<String, usize>)> {
        let after_marker = first_line.split_once(MARKER)?.1;
        // A fixture in a language whose comments are blocks carries the closer on the header line,
        // and the closer is where the declarations end rather than a malformed one. Anything else
        // that is not a 'name=count' is a typo and refuses the header, which is the point.
        let after_marker = ["-->", "*/", "*)", "-}", "]]", "}"].iter()
                .fold(after_marker, |text, closer| text.split(closer).next().unwrap_or(text));
        let (counts, language) = match after_marker.split_once(LANGUAGE_FIELD) {
            Some((before, name)) => (before, Some(name.trim().to_owned())),
            None => (after_marker, None)
        };
        let mut expectations = HashMap::new();
        for entry in counts.split_whitespace() {
            let (key, value) = entry.split_once('=')?;
            expectations.insert(key.to_owned(), value.parse::<usize>().ok()?);
        }

        if expectations.is_empty() { None } else { Some((language, expectations)) }
    }

    fn fixture_paths(root: &Path) -> Vec<std::path::PathBuf> {
        let mut paths = std::fs::read_dir(root)
            .unwrap_or_else(|x| panic!("cannot read the fixture directory {}: {x}", root.display()))
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        paths.sort();
        paths
    }

    #[test]
    fn language_fixtures_match_their_declared_counts() {
        let root = fixtures_dir();
        // The same lookup a run uses, name before extension, so a fixture called 'Makefile' is
        // resolved the way the program resolves it and not by a rule of this test's own
        let lookup = fixture_lookup();
        // Built-in defaults only, so that a preference in the machine's own config file cannot
        // change the counts
        let config = EngineConfig::default();

        let mut failures = Vec::new();
        let mut checked = 0;

        for path in fixture_paths(&root) {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();

            let contents = std::fs::read_to_string(&path).unwrap();
            let Some((declared, expected)) = parse_expectations(contents.lines().next().unwrap_or_default()) else {
                failures.push(format!("{name}: the first line must contain a '{MARKER} lines=N code=N ...' header"));
                continue;
            };

            let lang_name = match declared {
                Some(declared) => std::sync::Arc::from(declared.as_str()),
                None => match lookup.of_path(&path) {
                    Some(found) => found,
                    None => {
                        failures.push(format!("{name}: no supported language claims this name or its extension"));
                        continue;
                    }
                }
            };
            if !LANGUAGE_MAP_REF.contains_key(lang_name.as_ref()) {
                failures.push(format!("{name}: no language is called '{lang_name}'"));
                continue;
            }

            let language = LANGUAGE_MAP_REF.get(lang_name.as_ref()).unwrap();
            let mut buf = String::new();
            let stats = match parse_file_whole(&path, lang_name.as_ref(), &mut buf, &config) {
                Ok(stats) => stats,
                Err(x) => {
                    failures.push(format!("{name}: could not be parsed: {x}"));
                    continue;
                }
            };

            let mut actual = HashMap::from([
                ("lines".to_owned(), stats.lines),
                ("code".to_owned(), stats.code_lines),
                ("comments".to_owned(), stats.comment_lines),
                ("extra".to_owned(), stats.lines - stats.code_lines - stats.comment_lines),
            ]);
            for (index, keyword) in language.keywords.iter().enumerate() {
                actual.insert(keyword.descriptive_name.clone(), stats.keyword_occurences[index]);
            }

            for (key, expected_value) in &expected {
                match actual.get(key) {
                    Some(actual_value) if actual_value == expected_value => (),
                    Some(actual_value) => failures.push(format!("{name} ({lang_name}): {key} expected {expected_value}, got {actual_value}")),
                    None => {
                        let mut known = actual.keys().cloned().collect::<Vec<_>>();
                        known.sort();
                        failures.push(format!("{name} ({lang_name}): '{key}' is not a countable field. Available: {}", known.join(", ")));
                    }
                }
            }

            // A keyword the fixture does not mention must be absent, otherwise a fixture could
            // quietly stop covering a keyword the moment someone forgets to declare it
            for (index, keyword) in language.keywords.iter().enumerate() {
                let occurrences = stats.keyword_occurences[index];
                if occurrences > 0 && !expected.contains_key(&keyword.descriptive_name) {
                    failures.push(format!("{name} ({lang_name}): found {occurrences} '{}' but the header does not declare them",
                            keyword.descriptive_name));
                }
            }

            checked += 1;
        }

        assert!(checked > 0, "no fixtures were checked, is {} populated?", root.display());
        assert!(failures.is_empty(), "\n{} fixture check(s) failed:\n  {}\n", failures.len(), failures.join("\n  "));
    }

    // The stress corpus is not a fixture of this crate: its files carry a pair of counts per
    // counting tool and are meant to be run by any of them, so it lives at the top of the
    // repository and outside both packages. That also means it is absent from a crate downloaded
    // off crates.io, which is why this returns nothing rather than failing there.
    fn stress_corpus_dir() -> Option<std::path::PathBuf> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("stress-corpus");
        root.is_dir().then_some(root)
    }

    // '7 lines 1 code 6 comment', the shape every counter writes its own numbers in, read by the
    // word that follows each number so the order in the line does not matter
    fn parse_stress_counts(line: &str, marker: &str) -> Option<(usize, usize, usize)> {
        let words = line.split_once(marker)?.1.split_whitespace().collect::<Vec<_>>();
        let value_before = |name: &str| words.iter().position(|word| *word == name)
                .and_then(|at| at.checked_sub(1))
                .and_then(|at| words[at].parse::<usize>().ok());
        Some((value_before("lines")?, value_before("code")?, value_before("comment")?))
    }

    // The sections a line declares, as 'mezura-section TS 2 lines 1 code 1 comment', one per line.
    // A case in a container language needs them: its three totals are the same whether the sections
    // were found at all, so without these lines the file that proves the feature asserts nothing
    // about it. The language is the first word after the marker.
    //
    // 'real-section' carries no tool's name because a section is not a matter of opinion: whether a
    // '<script lang="ts">' block is TypeScript is a fact about the file, and a tool calling it
    // JavaScript because it cannot tell the two apart is wrong rather than differently defined.
    // Which lines are code and which are comment is the part that is genuinely per tool, and that is
    // what the two totals are for.
    fn parse_stress_sections(header: &str, marker: &str) -> Vec<(String, (usize, usize, usize))> {
        header.lines().filter_map(|line| {
            let rest = line.split_once(marker)?.1;
            let language = rest.split_whitespace().next()?;
            Some((language.to_owned(), parse_stress_counts(rest, language)?))
        }).collect()
    }

    fn sorted_sections(header: &str, marker: &str) -> Vec<(String, (usize, usize, usize))> {
        let mut sections = parse_stress_sections(header, marker);
        sections.sort();
        sections
    }

    // Whether a tool's answer is the right one, in both halves: the totals it declares against the
    // totals it wants, and the sections it declares against the sections the file has.
    fn tool_is_right_in(header: &str, tool: &str) -> bool {
        parse_stress_counts(header, &format!("{tool}-real")) == parse_stress_counts(header, &format!("{tool}-count"))
                && sorted_sections(header, &format!("{tool}-section")) == sorted_sections(header, "real-section")
    }

    // A tool's note explains why its answer is not the right one, so it belongs to exactly the cases
    // where the two disagree. Both directions and every tool by the same rule: a case that gets
    // something wrong and says nothing reads as a passing case, and a note left behind after the
    // wrong answer was fixed reads as a fault that is no longer there.
    fn check_the_note_of(header: &str, tool: &str, name: &str) -> Option<String> {
        // A case is free to say nothing about a tool, and most say nothing about any but ours
        parse_stress_counts(header, &format!("{tool}-count"))?;

        match (tool_is_right_in(header, tool), header.contains(&format!("{tool}:"))) {
            (false, false) => Some(format!("{name}: '{tool}' does not give the right answer here and no \
                    '{tool}:' line says what it gets wrong")),
            (true, false) => None,
            (true, true) => Some(format!("{name}: has a '{tool}:' line while '{tool}' gives the right \
                    answer, so either the note is stale or the numbers are")),
            (false, true) => None
        }
    }

    // Each file of the corpus declares the honest answer and the answer mezura gives today. The
    // assertion is on the second, so a case mezura gets wrong keeps the suite green while saying
    // so out loud, and the moment the answer changes at all somebody has to look: a fix has to
    // promote the file in the same commit, and a wrong answer that changed shape is not a fix.
    #[test]
    fn the_stress_corpus_answers_are_the_ones_declared() {
        let Some(root) = stress_corpus_dir() else { return };
        let lookup = fixture_lookup();
        let config = EngineConfig::default();

        let (mut failures, mut known_wrong, mut checked) = (Vec::new(), 0, 0);
        for path in fixture_paths(&root) {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            // The folder describes and licenses itself beside the cases; only a file with an
            // extension is one, since every case is source in some language
            if !name.contains('.') || name.ends_with(".md") { continue; }

            let contents = std::fs::read_to_string(&path).unwrap();
            // Generous, since a case that carries another counter's numbers as well as ours, and a
            // line per section of each, has a header far longer than the three lines the plainest
            // one needs
            let header = contents.lines().take(20).collect::<Vec<_>>().join("\n");
            let (Some(real), Some(declared)) = (parse_stress_counts(&header, "mezura-real"),
                    parse_stress_counts(&header, "mezura-count")) else {
                failures.push(format!("{name}: needs a 'mezura-real' and a 'mezura-count' line, each \
                        written as 'N lines N code N comment'"));
                continue;
            };
            let Some(lang_name) = lookup.of_path(&path) else {
                failures.push(format!("{name}: no supported language claims this name or its extension"));
                continue;
            };

            let mut buf = String::new();
            let report = parse_file_report(&path, lang_name.as_ref(), &mut buf, &config)
                    .unwrap_or_else(|x| panic!("{name} could not be parsed: {x}"));
            let mut found = report.sections.iter().map(|section| (section.language.clone(),
                    (section.stats.lines, section.stats.code_lines, section.stats.comment_lines)))
                    .collect::<Vec<_>>();
            let stats = report.into_whole();
            let counted = (stats.lines, stats.code_lines, stats.comment_lines);

            // Declared and found are compared as sets, since the order sections appear in is the
            // file's business and not the declaration's
            let sections = sorted_sections(&header, "mezura-section");
            found.sort();
            if sections != found {
                failures.push(format!("{name} ({lang_name}): declares the sections {sections:?} \
                        and found {found:?}"));
            }

            if counted != declared {
                let verdict = if counted == real && found == sorted_sections(&header, "real-section") {
                    "it is now right, so promote the file"
                } else {
                    "it is wrong in a new way"
                };
                failures.push(format!("{name} ({lang_name}): declared {declared:?}, counted {counted:?}, \
                        honest {real:?}. {verdict}"));
            } else if !tool_is_right_in(&header, "mezura") {
                known_wrong += 1;
            }
            for tool in ["mezura", "tokei"] {
                failures.extend(check_the_note_of(&header, tool, &name));
            }
            checked += 1;
        }

        println!("stress corpus: {checked} cases, {known_wrong} of them known wrong");
        assert!(checked > 0, "no stress cases were found in {}", root.display());
        assert!(failures.is_empty(), "\n{} stress case(s) moved:\n  {}\n", failures.len(), failures.join("\n  "));
    }

    // With the priority rules a real run has, so that a contested extension resolves here to the
    // language it resolves to on somebody's machine. Without them the tiebreak is alphabetical, and
    // a '.pas' file would be counted as Delphi in the corpus and as Pascal everywhere else.
    fn fixture_lookup() -> LanguageLookup {
        let priority = crate::languages::parse_shipped_extension_priority();
        LanguageLookup {
            by_extension: build_language_map_by(IdentifiedBy::Extension, &LANGUAGE_MAP_REF,
                    &priority.by_extension, &HashMap::new()).0,
            by_filename: build_language_map_by(IdentifiedBy::Filename, &LANGUAGE_MAP_REF,
                    &priority.by_filename, &HashMap::new()).0
        }
    }

    #[test]
    fn every_fixture_extension_resolves_to_exactly_one_language() {
        let mut claimants_of = HashMap::<String, Vec<String>>::new();
        for language in LANGUAGE_MAP_REF.values() {
            for extension in &language.extensions {
                claimants_of.entry(extension.clone()).or_default().push(language.name.clone());
            }
            // A fixture named after a whole filename is resolved by that name, so what has to be
            // uncontested is the name and not the extension its spelling happens to end in
            for filename in &language.filenames {
                claimants_of.entry(filename.clone()).or_default().push(language.name.clone());
            }
        }

        for path in fixture_paths(&fixtures_dir()) {
            // One that says what it is has already answered the question this asks
            let contents = std::fs::read_to_string(&path).unwrap_or_default();
            if parse_expectations(contents.lines().next().unwrap_or_default())
                    .is_some_and(|(declared, _)| declared.is_some()) {
                continue;
            }

            let name = path.file_name().and_then(|x| x.to_str()).unwrap_or_default().to_owned();
            let extension = match claimants_of.contains_key(&name) {
                true => name,
                false => path.extension().and_then(|x| x.to_str()).unwrap_or_default().to_owned()
            };
            let claimants = claimants_of.get(&extension).cloned().unwrap_or_default();
            assert!(claimants.len() == 1, "the fixture extension '{extension}' is claimed by {} languages ({}), so its counts depend on the tie-break rule",
                    claimants.len(), claimants.join(", "));
        }
    }
}

