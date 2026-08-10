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

use crate::{EngineConfig, Language, phase_timing};
use crate::domain::{CommentPair, FileStats};

pub const MAX_RETAINED_FILE_BUFFER_BYTES: usize = 4_194_304;

const NO_SLOT : u16 = u16::MAX;

// The four kinds of declared symbol, as indices into the per-kind arrays of a scan
const STRINGS    : u8 = 0;
const COMMENTS   : u8 = 1;
const COM_STARTS : u8 = 2;
const COM_ENDS   : u8 = 3;

// What one side of a string pair may do. An ordinary quote is both sides in one symbol; a pair
// whose halves differ gets one slot per half. Only the two-sided form obeys the backslash: a
// distinct opener means a raw form, and inside those nothing escapes.
const ROLE_EITHER : u8 = 0;
const ROLE_OPEN   : u8 = 1;
const ROLE_CLOSE  : u8 = 2;

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
}

impl ScanPlan {
    pub fn build(language: &Language) -> ScanPlan {
        // The single line symbols first and the crossing ones after them, which is the numbering
        // 'Language::get_string_pair_of' answers to
        let mut entries : Vec<PlanEntry> = Vec::new();
        for (i, symbol) in language.string_symbols.iter().enumerate() {
            entries.push(PlanEntry::of(STRINGS, i as u8, ROLE_EITHER, symbol.as_bytes()));
        }
        for (i, (open, close)) in language.multiline_strings.iter().enumerate() {
            let index = (language.string_symbols.len() + i) as u8;
            if open == close {
                entries.push(PlanEntry::of(STRINGS, index, ROLE_EITHER, open.as_bytes()));
            } else {
                entries.push(PlanEntry::of(STRINGS, index, ROLE_OPEN, open.as_bytes()));
                entries.push(PlanEntry::of(STRINGS, index, ROLE_CLOSE, close.as_bytes()));
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
        // An anchored match begins behind the byte that found it, so its position can come out
        // behind one already recorded; the kind is sorted afterwards to put it back in line order
        for (entry, anchor) in entries.iter().zip(&anchors) {
            if *anchor != 0 { sorted_kinds[entry.kind as usize] = true }
        }
        ScanPlan { chunks, first, slots, symbols, sorted_kinds }
    }
}

// Where in each symbol the byte that finds it sits: at the front, except for a symbol beginning
// with an ordinary letter or digit, which is searched by a byte further in. See 'Slot'.
//
// Which byte matters as much as that it is not the letter. The choice is a byte the scan looks
// for anyway, which is why declaring 'r#"' or 'R"(' adds nothing to the passes: both are found by
// the quote the language already declares. Anchoring 'R"(' on its bracket instead would put '(' in
// front of every call in a C++ file.
fn anchors_of(entries: &[PlanEntry]) -> Vec<u8> {
    let searched_anyway = entries.iter().map(|entry| entry.bytes[0])
            .filter(|byte| !byte.is_ascii_alphanumeric()).collect::<Vec<u8>>();

    entries.iter().map(|entry| {
        if !entry.bytes[0].is_ascii_alphanumeric() {
            return 0;
        }
        entry.bytes.iter().position(|byte| searched_anyway.contains(byte))
                .or_else(|| entry.bytes.iter().rposition(|byte| !byte.is_ascii_alphanumeric()))
                .unwrap_or(0) as u8
    }).collect()
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
        // An escape cancels a string symbol and nothing else, and never the half of a two-sided
        // pair: a distinct opener means a raw form, and inside those the backslash is a byte
        if slot.kind == STRINGS && slot.role == ROLE_EITHER && start != 0 && !is_not_escaped(start, line_bytes) { continue }

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

pub fn parse_file(path: &Path, lang_name: &str, buf: &mut String, buffers: &mut ParseBuffers,
    language_map: &HashMap<String,Language>, keyword_matcher: Option<&KeywordMatcher>, config: &EngineConfig)
-> Result<FileStats,String>
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

    let file_stats = parse_lines(buf, language_map.get(lang_name).unwrap(), keyword_matcher, config, buffers);
    if let Some(t) = at { buffers.timing.parse_nanos += phase_timing::nanos_since(t); }

    Ok(file_stats)
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

fn parse_lines(contents: &str, language: &Language, keyword_matcher: Option<&KeywordMatcher>, config: &EngineConfig,
    buffers: &mut ParseBuffers) -> FileStats
{
    let ParseBuffers { scan, alias_indices, code_spans, .. } = buffers;
    let mut file_stats = match config.count_keywords {
        false => FileStats::default(),
        true => FileStats::with_keywords(&language.keywords)
    };
    let counting_keywords = config.count_keywords && keyword_matcher.is_some();
    code_spans.clear();

    let mut open_comment = None;
    let mut open_str_symbol = None;
    for (line_start, raw_line) in get_lines_of(contents) {
        file_stats.lines += 1;

        // Ascii-only trimming, since the unicode whitespace classification of trim() costs
        // a significant part of the total run time, for lines that are code either way
        let line = raw_line.trim_ascii();
        if line.is_empty() { continue; }
        let base = line_start + (raw_line.len() - raw_line.trim_ascii_start().len());

        // Two functions rather than one with a branch, so a language without multiline comments never
        // pays for the checks that only they need
        let line_info =
        if language.supports_multiline_comments() {
            get_bounds_w_multiline_comments(line, language, open_comment, &open_str_symbol, scan)
        } else {
            get_bounds_only_single_line_comments(line, language, &open_str_symbol, scan)
        };

        open_comment = line_info.open_comment_after;
        // Only a symbol declared to cross lines carries its string to the next one, so the damage
        // of an unbalanced quote is this line and not the rest of the file
        open_str_symbol = line_info.open_str_sybol_after
                .filter(|symbol| language.string_crosses_lines(*symbol));

        if line_info.code.is_some() {
            // With the strings and comments stripped, a line holding no letter and no digit is
            // punctuation the language required rather than anything the programmer said: '}',
            // '});', '],', ')'. Bytes above 0x7f count as content, so an identifier in a non-latin
            // alphabet reads as code and not as punctuation.
            let is_no_content = !line_info.has_string_literal
                    && !scan.code_ranges.iter().any(|(from, to)|
                            line.as_bytes()[*from..*to].iter().any(|b| b.is_ascii_alphanumeric() || *b >= 0x80));
            if config.braces_as_code || !is_no_content {
                file_stats.code_lines += 1;
                if counting_keywords {
                    push_trimmed_spans(code_spans, &scan.code_ranges, line, base);
                }
            }
        } else if line_info.has_string_literal {
            file_stats.code_lines += 1;
        } else {
            file_stats.comment_lines += 1;
        }
    }

    if let Some(matcher) = keyword_matcher && counting_keywords {
        count_keywords(contents, code_spans, matcher, &mut file_stats, alias_indices);
    }

    file_stats
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
fn push_code(ranges: &mut Vec<(usize, usize)>, from: usize, to: usize) {
    if to > from {
        ranges.push((from, to));
    }
}

fn line_info_with_str_symbol(ranges: usize, str_symbol: u8) -> LineInfo {
    if ranges == 0 {
        LineInfo::with_open_symbol(str_symbol)
    } else {
        LineInfo::code_span_with((0, ranges), true, None, Some(str_symbol))
    }
}

fn get_bounds_only_single_line_comments(line: &str, language: &Language, open_str_symbol: &Option<u8>,
    buffers: &mut ScanBuffers) -> LineInfo
{
    scan_line(line, language, buffers);
    resolve_string_delimiters(language, open_str_symbol, buffers);
    let ScanBuffers { strings: str_indices, string_symbols: str_symbols, comments: comment_indices, code_ranges, .. } = buffers;

    if open_str_symbol.is_some() && str_indices.is_empty() {
        return LineInfo::none_str(None, true, *open_str_symbol);
    }

    if str_indices.is_empty() && comment_indices.is_empty() {
        push_code(code_ranges, 0, line.len());
        return LineInfo::code_span((0, code_ranges.len()), false);
    }
    
    let has_more_strs = |counter| counter < str_indices.len();
    let has_more_comments = |counter| counter < comment_indices.len(); 
    let next_symbol_is_comment = |comment_counter: usize, str_counter: usize| {
        if !has_more_comments(comment_counter) {return false;}
        if has_more_strs(str_counter) && comment_indices[comment_counter] > str_indices[str_counter] {
            return false;
        }
        true
    };
    let next_symbol_is_string = |comment_counter: usize, str_counter: usize| {
        if !has_more_strs(str_counter) {return false;}
        if has_more_comments(comment_counter)  && str_indices[str_counter] > comment_indices[comment_counter] {
            return false;
        }
        true
    };
    let advance_comment_counter_until = |index, comment_counter: &mut usize| {
        while *comment_counter < comment_indices.len() && comment_indices[*comment_counter] < index {
            *comment_counter += 1;
        }
    };

    let mut has_string_literal = false;
    let mut slice_start_index = 0;
    let mut is_str_open_m = open_str_symbol.is_some();
    let (mut str_counter, mut comment_counter) = (0,0);
    loop {
        if is_str_open_m {
            let index_after = str_indices[str_counter]
                    + language.get_string_pair_of(str_symbols[str_counter]).1.len();

            if index_after >= line.len() {
                if code_ranges.is_empty() {return LineInfo::none_all(true);}
                else {return LineInfo::code_span((0, code_ranges.len()), true);}
            }

            is_str_open_m = false;
            str_counter += 1;
            advance_comment_counter_until(index_after, &mut comment_counter);
            slice_start_index = index_after;
            has_string_literal = true;
        } else {
            if next_symbol_is_string(comment_counter, str_counter) {
                let this_index = str_indices[str_counter];
                push_code(code_ranges, slice_start_index, this_index);
                str_counter += 1;
                if !has_more_strs(str_counter) {
                    return line_info_with_str_symbol(code_ranges.len(), str_symbols[str_counter-1]);
                }

                is_str_open_m = true;
                has_string_literal = true;
            } else if next_symbol_is_comment(comment_counter, str_counter) {
                push_code(code_ranges, slice_start_index, comment_indices[comment_counter]);

                if code_ranges.is_empty() {return LineInfo::none_str(None, has_string_literal, None);}
                else {return LineInfo::code_span_with((0, code_ranges.len()), has_string_literal, None, None);}
            } else {
                push_code(code_ranges, slice_start_index, line.len());
                return LineInfo::code_span((0, code_ranges.len()), has_string_literal);
            }
        }
    }
}

fn get_bounds_w_multiline_comments(line: &str, language: &Language, open_comment: Option<(u8, u32)>,
    open_str_symbol: &Option<u8>, buffers: &mut ScanBuffers) -> LineInfo
{
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
                // A nesting or leveled block that stays open is comment through and through; the
                // empty-span shape below is kept only for the plain pairs that always had it
                if (nests || leveled) && code_ranges.is_empty() {
                    return LineInfo::open_comment_at(open_pair, carry);
                }
                return LineInfo::code_span_with((0, code_ranges.len()), has_string_literal, Some((open_pair, carry)), None);
            };
            last_symbol_index = closed_at;
            let end_level = if leveled { carried as u8 } else { 0 };
            let index_after = last_symbol_index + language.comment_end_len(open_pair, end_level);
            if index_after >= line.len() {
                if code_ranges.is_empty() {return LineInfo::none_all(has_string_literal);}
                else {return LineInfo::code_span((0, code_ranges.len()), has_string_literal);}
            }

            open_com_m = None;
            progress_counters_after(last_symbol_index, &mut comment_counter, &mut str_counter,
                    &mut start_com_counter, &mut end_com_counter);
            end_com_counter += 1;

            if has_more_strs(str_counter) && str_indices[str_counter] == index_after {
                is_str_open_m = true;
            } else if has_more_starts(start_com_counter) && com_start_indices[start_com_counter].0 == index_after {
                let (_, symbol, level) = com_start_indices[start_com_counter];
                open_com_m = Some((symbol, if language.comment_is_leveled(symbol) { level as u32 } else { 1 }));
            } else {
                slice_start_index = index_after;
            }
        } else {
            if next_symbol_is_comment(comment_counter, str_counter, start_com_counter) {
                push_code(code_ranges, slice_start_index, comment_indices[comment_counter]);
                if code_ranges.is_empty() {return LineInfo::none_all(has_string_literal);}
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
                    if code_ranges.is_empty() {return LineInfo::with_open_comment(this_symbol);}
                    else {return LineInfo::code_span_with((0, code_ranges.len()), has_string_literal, Some((this_symbol, 1)), None);}
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
    use crate::{Keyword, Stats};
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
        let info = get_bounds_w_multiline_comments(line, language, open_comment, open_str_symbol, &mut buffers);
        text_of(line, info, &buffers)
    }

    fn bounds_single(line: &str, language: &Language, open_str_symbol: &Option<u8>) -> TextInfo {
        let mut buffers = ScanBuffers::default();
        let info = get_bounds_only_single_line_comments(line, language, open_str_symbol, &mut buffers);
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
        multiline_strings : vec![("\"\"\"".to_owned(), "\"\"\"".to_owned()), ("'''".to_owned(), "'''".to_owned())],
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

    fn matcher_for(lang_name: &str) -> Option<KeywordMatcher> {
        KeywordMatcher::build(LANGUAGE_MAP_REF.get(lang_name).unwrap())
    }

    // Seeded from the language and then given the one file, which is what a real run does: the seed
    // is what puts a slot in for every keyword the language declares, so one that never occurs still
    // reports its zero instead of being missing.
    fn content_info_of(file: FileStats, lang_name: &str) -> Stats {
        let language = LANGUAGE_MAP_REF.get(lang_name).unwrap();
        let mut stats = Stats::from(language);
        stats.add_file(file, 0, &language.keywords);
        stats
    }

    #[test]
    fn test_correct_parsing_of_the_sample_files() {
        let mut buf = String::with_capacity(150);

        let mut config = EngineConfig::default();
        let result = parse_file(&sample_file("a.txt"), "Java", &mut buf, &mut ParseBuffers::default(), &LANGUAGE_MAP_REF, matcher_for("Java").as_ref(), &config);
        let result = content_info_of(result.unwrap(), "Java");
        assert_eq!(Stats::new(1, 0, 44, 13, 15, hashmap!("classes".to_owned()=>3,"interfaces".to_owned()=>0)), result);
        buf.clear();
        // The keywords keep their slots and stay at zero, which is what a run produces: the seed
        // comes from the language and not from the file, so hiding them stops the counting and not
        // the language's own list of what it would have counted.
        config.count_keywords = false;
        let result = parse_file(&sample_file("a.txt"), "Java", &mut buf, &mut ParseBuffers::default(), &LANGUAGE_MAP_REF, matcher_for("Java").as_ref(), &config);
        let result = content_info_of(result.unwrap(), "Java");
        assert_eq!(Stats::new(1, 0, 44, 13, 15, hashmap!("classes".to_owned()=>0,"interfaces".to_owned()=>0)), result);
        buf.clear();
        config.count_keywords = true;
        let result = parse_file(&sample_file("a.txt"), "C#", &mut buf, &mut ParseBuffers::default(), &LANGUAGE_MAP_REF, matcher_for("C#").as_ref(), &EngineConfig::default());
        let result = content_info_of(result.unwrap(), "C#");
        assert_eq!(Stats::new(1, 0, 44, 13, 15, hashmap!("structs".to_owned()=>0,"classes".to_owned()=>3,"interfaces".to_owned()=>0)), result);
        buf.clear();
        
        let result = parse_file(&sample_file("d.txt"), "C#", &mut buf, &mut ParseBuffers::default(), &LANGUAGE_MAP_REF, matcher_for("C#").as_ref(), &EngineConfig::default());
        let result = content_info_of(result.unwrap(), "C#");
        assert_eq!(Stats::new(1, 0, 19, 7, 10, hashmap!("structs".to_owned()=>0,"classes".to_owned()=>5,"interfaces".to_owned()=>0)), result);
        buf.clear();
        let result = parse_file(&sample_file("d.txt"), "Java", &mut buf, &mut ParseBuffers::default(), &LANGUAGE_MAP_REF, matcher_for("Java").as_ref(), &EngineConfig::default());
        let result = content_info_of(result.unwrap(), "Java");
        assert_eq!(Stats::new(1, 0, 19, 7, 10, hashmap!("classes".to_owned()=>5,"interfaces".to_owned()=>0)), result);
        buf.clear();

        let result = parse_file(&sample_file("b.txt"), "Java", &mut buf, &mut ParseBuffers::default(), &LANGUAGE_MAP_REF, matcher_for("Java").as_ref(), &EngineConfig::default());
        let result = content_info_of(result.unwrap(), "Java");
        assert_eq!(Stats::new(1, 0, 19, 11, 5, hashmap!("classes".to_owned()=>7,"interfaces".to_owned()=>0)), result);
        buf.clear();

        // The 'class' on the line between two lone apostrophes counts: Python declares its plain
        // quotes single-line, so the quote above it dies at its own line instead of swallowing it
        let result = parse_file(&sample_file("c.txt"), "Python", &mut buf, &mut ParseBuffers::default(), &LANGUAGE_MAP_REF, matcher_for("Python").as_ref(), &EngineConfig::default());
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
            let stats = parse_file(&path, "Java", buf, &mut ParseBuffers::default(), &LANGUAGE_MAP_REF, matcher_for("Java").as_ref(), &config).unwrap();
            (stats.lines, stats.code_lines, stats.comment_lines)
        };

        // a.txt has 10 lines that are nothing but a brace, and 6 blank ones. The comments never
        // move, whatever the flag says, and the three categories always add up to the total.
        assert_eq!((44, 13, 15), count_with(false, &mut buf));
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
        let info = get_bounds_w_multiline_comments(line, &RUST, None, &None, &mut buffers);
        let mut spans = Vec::new();
        assert!(info.code.is_some());
        push_trimmed_spans(&mut spans, &buffers.code_ranges, line, 0);
        count_keywords(line, &spans, &matcher, &mut file_stats, &mut Vec::new());
        assert_eq!(0, file_stats.keyword_occurences[0]);

        // and the same word, whole, still counts
        let line = "struct a;";
        let mut file_stats = FileStats::with_keywords(&[STRUCT.clone(),ENUM.clone(),TRAIT.clone()]);
        let mut buffers = ScanBuffers::default();
        let info = get_bounds_w_multiline_comments(line, &RUST, None, &None, &mut buffers);
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
        // '*/' does not close a '/+' block, '+/' does not close a '/*' block. The nesting pair
        // gets the honest comment label; the plain one keeps the empty-code-span shape that
        // '*//*' at the end of a line always had. Either way the open pair survives the line.
        assert_eq!(TextInfo::with_open_comment(1), bounds_multi("/+ a */ still open", &D_LANG, None, &None));
        assert_eq!(TextInfo::new(Some(String::new()), false, Some((0, 1)), None),
                bounds_multi("/* a +/ still open", &D_LANG, None, &None));
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

    // A string ends with its line unless its symbol was declared to cross lines, so an unbalanced
    // quote costs one line while a docstring still spans as many as it likes.
    #[test]
    fn an_unbalanced_quote_costs_its_line_and_not_the_rest_of_the_file() {
        let plain = Language::new("py-like", ["py"], ["\"", "'"], ["#"], &[], [])
                .with_multiline_strings(&["\"\"\""]);
        let crossing = Language::new("py-like", ["py"], [""; 0], ["#"], &[], [])
                .with_multiline_strings(&["\"\"\"", "\"", "'"]);
        let contents = "a = \"unbalanced\nb = 1\nc = 2\n# comment\n";

        let stats = parse_lines(contents, &plain, None, &EngineConfig::default(), &mut ParseBuffers::default());
        assert_eq!((4, 3, 1), (stats.lines, stats.code_lines, stats.comment_lines));
        // declared crossing, everything after the quote is string content and code to the end
        let stats = parse_lines(contents, &crossing, None, &EngineConfig::default(), &mut ParseBuffers::default());
        assert_eq!((4, 4, 0), (stats.lines, stats.code_lines, stats.comment_lines));

        // and the docstring symbol, which is declared crossing in both, still spans lines
        let doc = "d = \"\"\"docstring\n# still string\n\"\"\"\ne = 1\n# comment\n";
        let stats = parse_lines(doc, &plain, None, &EngineConfig::default(), &mut ParseBuffers::default());
        assert_eq!((5, 4, 1), (stats.lines, stats.code_lines, stats.comment_lines));
    }

    // Closing used to advance one byte whatever the symbol, so the tail of a closing '"""' leaked
    // into the code text. Nearly invisible while the line counted as code anyway; load bearing the
    // moment a closer has a length of its own.
    #[test]
    fn closing_a_string_advances_past_the_whole_closing_symbol() {
        assert_eq!(TextInfo::from_slice_w_literal("var d =  y"),
                bounds_multi(r#"var d = """doc""" y"#, &CSHARP_VERBATIM, None, &None));
        let mut buffers = ScanBuffers::default();
        let info = get_bounds_only_single_line_comments(r#"x = """doc""" y"#, &PYTHON_FULL, &None, &mut buffers);
        assert_eq!(TextInfo::from_slice_w_literal("x =  y"), text_of(r#"x = """doc""" y"#, info, &buffers));
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

            let string_halves = language.string_symbols.iter().cloned()
                    .chain(language.multiline_strings.iter()
                            .flat_map(|(open, close)| [open.clone(), close.clone()]))
                    .collect::<Vec<_>>();
            let com_starts = language.multiline_comments.iter().map(|(start, _)| start.clone()).collect::<Vec<_>>();
            let com_ends = language.multiline_comments.iter().map(|(_, end)| end.clone()).collect::<Vec<_>>();
            for symbols in [&string_halves, &language.comment_symbols, &com_starts, &com_ends] {
                if symbols.is_empty() { continue; }
                let first_bytes = symbols.iter().map(|s| s.as_bytes()[0]).collect::<Vec<u8>>();
                let holding = plan.chunks.iter()
                        .filter(|c| first_bytes.iter().any(|b| c.bytes[..c.len as usize].contains(b)))
                        .count();
                assert_eq!(1, holding, "{} splits a kind across passes", language.name);
            }
            // and no byte is looked for twice, which would report the same symbol from two passes
            for symbol in string_halves.iter().chain(language.comment_symbols.iter())
                    .chain(com_starts.iter()).chain(com_ends.iter()) {
                assert_eq!(1, searched(symbol.as_bytes()[0]), "{} searches a byte twice", language.name);
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

        // and the whole shape through the walk: the line closes and reopens, so it ends still open.
        // The whitespace between the two symbols is a code stretch with nothing in it, the same as
        // '*/ /*' behaves today, and parse_lines counts such a line as extra.
        assert_eq!(TextInfo::new(Some(String::new()), false, Some((0, 1)), None),
                bounds_multi("]]--[[", &LUA, Some(0), &None));
        assert_eq!(TextInfo::new(Some("  ".to_owned()), false, Some((0, 1)), None),
                bounds_multi("]]  --[[ reopened", &LUA, Some(0), &None));
        // an HTML-shaped pair, 4 against 3, through a language declaring no line comments
        let html : Language = Language::new("html-like", ["html"], ["\""], [""; 0], &[("<!--", "-->")], []);
        assert_eq!(TextInfo::new(Some(String::new()), false, Some((0, 1)), None),
                bounds_multi("--><!--", &html, Some(0), &None));
        assert_eq!(TextInfo::new(Some(" ".to_owned()), false, Some((0, 1)), None),
                bounds_multi("--> <!-- reopened", &html, Some(0), &None));
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
        assert_eq!(TextInfo::new(Some("[,,,,]".to_owned()),true,None,None),bounds_single(&line, &PYTHON, &None));
        let line = String::from("\\''\''");
        assert_eq!(TextInfo::new(Some("\\\'".to_owned()),true,None,Some(1u8)), bounds_single(&line, &PYTHON, &None));
        assert_eq!(TextInfo::none_all(true), bounds_single(&line, &PYTHON, &Some(1u8)));
        let line = String::from("\'\\'\\'\\\''"); 
        assert_eq!(TextInfo::new(None,true,None,None), bounds_single(&line, &PYTHON, &None));
        
        let single_str_opt = &Some(1u8);
        let double_str_opt = &Some(0u8);
        let single_str_li = TextInfo::with_open_symbol(1);
        let double_str_li = TextInfo::with_open_symbol(0);
    
        let line = String::from("Hello world!");
        assert_eq!(TextInfo::from_slice("Hello world!"),bounds_single(&line, &PYTHON, &None));
        assert_eq!(single_str_li,bounds_single(&line, &PYTHON, single_str_opt));
        
        //testing comments
        let line = String::from("#Hello world!");
        assert_eq!(single_str_li,bounds_single(&line, &PYTHON, single_str_opt));
        let line = String::from("Hello world!#");
        assert_eq!(TextInfo::from_slice("Hello world!"),bounds_single(&line, &PYTHON, &None));
        let line = String::from("Hello# world!");
        assert_eq!(TextInfo::from_slice("Hello"),bounds_single(&line, &PYTHON, &None));
        assert_eq!(single_str_li,bounds_single(&line, &PYTHON, single_str_opt));
        let line = String::from("Hello## world!");
        assert_eq!(TextInfo::from_slice("Hello"),bounds_single(&line, &PYTHON, &None));
        let line = String::from("#Hello# world!");
        assert_eq!(single_str_li,bounds_single(&line, &PYTHON, single_str_opt));
        
        //testing strings 
        let line = String::from("\"Hello world!#");
        assert_eq!(double_str_li,bounds_single(&line, &PYTHON, &None));
        let line = String::from("\"Hello\" world!");
        assert_eq!(TextInfo::from_slice_w_literal(" world!"),bounds_single(&line, &PYTHON, &None));
        assert_eq!(TextInfo::new(Some("Hello".to_owned()), true, None, Some(0u8)),bounds_single(&line, &PYTHON, double_str_opt));
        let line = String::from("Hello world!\"");
        assert_eq!(TextInfo::new(Some("Hello world!".to_owned()), true, None, Some(0u8)),bounds_single(&line, &PYTHON, &None));
        let line = String::from("\"'Hello'\" world!");
        assert_eq!(TextInfo::from_slice_w_literal(" world!"),bounds_single(&line, &PYTHON, &None));
        let line = String::from("'Hello' world!");
        assert_eq!(TextInfo::from_slice_w_literal(" world!"),bounds_single(&line, &PYTHON, &None));
        let line = String::from("'\"He'llo'\" world!'");
        assert_eq!(TextInfo::from_slice_w_literal("llo"),bounds_single(&line, &PYTHON, &None));
        assert_eq!(TextInfo::new(Some("He".to_owned()), true, None, Some(0u8)),bounds_single(&line, &PYTHON, double_str_opt));
        let line = String::from(r#""""Hello""#);
        assert_eq!(TextInfo::new(None, true, None, None), bounds_single(&line, &PYTHON, &None));
        assert_eq!(TextInfo::new(Some("Hello".to_owned()), true, None, Some(0u8)), bounds_single(&line, &PYTHON, double_str_opt));
        let line = String::from(r#"['⣯', '⣟"#); 
        assert_eq!(TextInfo::new(Some("[, ".to_owned()),true,None,Some(1u8)), bounds_single(&line, &PYTHON, &None));
        
        //test mixed
        let line = String::from("'Hello#' world!'");
        assert_eq!(TextInfo::new(Some(" world!".to_owned()), true, None, Some(1u8)),bounds_single(&line, &PYTHON, &None));
        assert_eq!(TextInfo::from_slice_w_literal("Hello"),bounds_single(&line, &PYTHON, single_str_opt));
        let line = String::from("'Hello'# world!'");
        assert_eq!(TextInfo::none_all(true),bounds_single(&line, &PYTHON, &None));
        assert_eq!(TextInfo::from_slice_w_literal("Hello"),bounds_single(&line, &PYTHON, single_str_opt));
        let line = String::from("''#Hello");
        assert_eq!(TextInfo::none_all(true),bounds_single(&line, &PYTHON, &None));
        let line = String::from("'''#'''Hello world!'");
        assert_eq!(TextInfo::new(Some("Hello world!".to_owned()), true, None, Some(1u8)),bounds_single(&line, &PYTHON, &None));
        assert_eq!(TextInfo::none_all(true),bounds_single(&line, &PYTHON, single_str_opt));
        assert_eq!(TextInfo::with_open_symbol(0),bounds_single(&line, &PYTHON, double_str_opt));
        let line = String::from("Hello'###'\"world!\"");
        assert_eq!(TextInfo::from_slice_w_literal("Hello"),bounds_single(&line, &PYTHON, &None));
        assert_eq!(TextInfo::none_all(true),bounds_single(&line, &PYTHON, single_str_opt));
        assert_eq!(TextInfo::new(Some("world!".to_owned()), true, None, Some(0u8)),bounds_single(&line, &PYTHON, double_str_opt));
        let line = String::from("\"//'''\"Hello'\"world!");
        assert_eq!(TextInfo::new(Some("Hello".to_owned()), true, None, Some(1u8)),bounds_single(&line, &PYTHON, &None));
        assert_eq!(TextInfo::from_slice_w_literal("world!"),bounds_single(&line, &PYTHON, single_str_opt));
        assert_eq!(TextInfo::new(Some("//".to_owned()), true, None, Some(0u8)),bounds_single(&line, &PYTHON, double_str_opt));
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

    fn fixtures_dir() -> std::path::PathBuf {
        Path::new(FIXTURES_DIR).join("lang")
    }

    // Each fixture declares, on its first line and in its own comment syntax, the counts mezura must
    // produce for it. The counts are hand-verified, so a mismatch means either the parser regressed
    // or the fixture is wrong; both are worth stopping for. The header line itself is a comment, so
    // it is included in 'lines' and excluded from 'code'.
    fn parse_expectations(first_line: &str) -> Option<HashMap<String, usize>> {
        let after_marker = first_line.split_once(MARKER)?.1;
        let mut expectations = HashMap::new();
        for entry in after_marker.split_whitespace() {
            let (key, value) = entry.split_once('=')?;
            expectations.insert(key.to_owned(), value.parse::<usize>().ok()?);
        }

        if expectations.is_empty() { None } else { Some(expectations) }
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

            let Some(lang_name) = lookup.of_path(&path) else {
                failures.push(format!("{name}: no supported language claims this name or its extension"));
                continue;
            };

            let contents = std::fs::read_to_string(&path).unwrap();
            let Some(expected) = parse_expectations(contents.lines().next().unwrap_or_default()) else {
                failures.push(format!("{name}: the first line must contain a '{MARKER} lines=N code=N ...' header"));
                continue;
            };

            let language = LANGUAGE_MAP_REF.get(lang_name.as_ref()).unwrap();
            let keyword_matcher = KeywordMatcher::build(language);
            let mut buf = String::new();
            let stats = match parse_file(&path, lang_name.as_ref(), &mut buf, &mut ParseBuffers::default(),
                    &LANGUAGE_MAP_REF, keyword_matcher.as_ref(), &config) {
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

    fn fixture_lookup() -> LanguageLookup {
        LanguageLookup {
            by_extension: build_language_map_by(IdentifiedBy::Extension, &LANGUAGE_MAP_REF, &HashMap::new(), &HashMap::new()).0,
            by_filename: build_language_map_by(IdentifiedBy::Filename, &LANGUAGE_MAP_REF, &HashMap::new(), &HashMap::new()).0
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

