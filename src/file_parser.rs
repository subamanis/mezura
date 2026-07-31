use std::borrow::Cow;
use std::{io::Read as IoRead, str};

use memchr::memmem;

use crate::*;

const MAX_RETAINED_FILE_BUFFER_BYTES: usize = 4_194_304;

const NO_SLOT : u16 = u16::MAX;

const STRINGS    : u8 = 0;
const COMMENTS   : u8 = 1;
const COM_STARTS : u8 = 2;
const COM_ENDS   : u8 = 3;

// One declared symbol. 'next' chains every symbol that begins with the same byte, longest first,
// so that a '"""' is recognised before the '"' that starts it.
#[derive(Debug, Clone, Copy)]
struct Slot {
    symbol: u8,
    kind: u8,
    len: u8,
    second: u8,
    next: u16,
}

#[derive(Debug, Clone, Copy)]
struct Chunk {
    bytes: [u8; 3],
    len: u8,
}

// A line used to be scanned once per symbol. Every symbol begins with one byte, and memchr searches
// up to three bytes in a single SIMD pass, so the symbols are grouped by their first byte and the
// groups are packed into as few passes as the language allows: one for most, two for the handful
// that declare more than three distinct first bytes. The pass gives candidate positions, and only
// there is the rest of a symbol compared.
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
        let mut entries : Vec<(u8, u8, Box<[u8]>)> = Vec::new();
        for (i, symbol) in language.string_symbols.iter().enumerate() {
            entries.push((STRINGS, i as u8, symbol.as_bytes().into()));
        }
        for (i, symbol) in language.comment_symbols.iter().enumerate() {
            entries.push((COMMENTS, i as u8, symbol.as_bytes().into()));
        }
        if let Some(symbol) = &language.multiline_comment_start_symbol {
            entries.push((COM_STARTS, 0, symbol.as_bytes().into()));
        }
        if let Some(symbol) = &language.multiline_comment_end_symbol {
            entries.push((COM_ENDS, 0, symbol.as_bytes().into()));
        }
        entries.retain(|(_, _, bytes)| !bytes.is_empty());
        entries.sort_by_key(|(_, _, bytes)| std::cmp::Reverse(bytes.len()));

        let mut first = [NO_SLOT; 256];
        let (mut slots, mut symbols) = (Vec::with_capacity(entries.len()), Vec::with_capacity(entries.len()));
        for (kind, symbol, bytes) in &entries {
            let index = slots.len() as u16;
            slots.push(Slot {
                symbol: *symbol,
                kind: *kind,
                len: bytes.len() as u8,
                second: if bytes.len() > 1 { bytes[1] } else { 0 },
                next: NO_SLOT,
            });
            symbols.push(bytes.clone());
            let head = &mut first[bytes[0] as usize];
            if *head == NO_SLOT {
                *head = index;
            } else {
                let mut cursor = *head as usize;
                while slots[cursor].next != NO_SLOT { cursor = slots[cursor].next as usize }
                slots[cursor].next = index;
            }
        }

        let (chunks, sorted_kinds) = pack_into_chunks(&entries);
        ScanPlan { chunks, first, slots, symbols, sorted_kinds }
    }
}

// Two kinds that share a first byte have to be searched in the same pass, otherwise that byte would
// be visited twice. Kinds are merged into groups by that overlap, and the groups are then packed
// whole, which is what keeps every output vector in the order the positions appear on the line and
// lets the sorting go away. A group of more than three distinct bytes cannot be one pass, so it is
// split and the kinds it holds are marked as needing a sort after all.
fn pack_into_chunks(entries: &[(u8, u8, Box<[u8]>)]) -> (Vec<Chunk>, [bool; 4]) {
    let mut bytes_of_kind : [Vec<u8>; 4] = Default::default();
    for (kind, _, bytes) in entries {
        let set = &mut bytes_of_kind[*kind as usize];
        if !set.contains(&bytes[0]) { set.push(bytes[0]) }
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

fn plan_of(language: &Language) -> &ScanPlan {
    language.scan_plan.get_or_init(|| ScanPlan::build(language))
}

// The per line working memory, owned by the consumer thread and cleared rather than reallocated.
#[derive(Debug, Default)]
pub struct ScanBuffers {
    raw_strings: Vec<(usize, u8)>,
    strings: Vec<usize>,
    string_symbols: Vec<u8>,
    comments: Vec<usize>,
    com_starts: Vec<usize>,
    com_ends: Vec<usize>,
    consumed: Vec<usize>,
    relevant: String,
}

// The keyword scratch cannot live in ScanBuffers: while a LineInfo borrows the cleansed line out of
// it, the whole struct is borrowed, and counting the keywords of that very line needs a free one.
#[derive(Debug, Default)]
pub struct ParseBuffers {
    scan: ScanBuffers,
    alias_indices: Vec<usize>,
    pub timing: phase_timing::Totals,
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
        self.relevant.clear();
    }
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
    let plan = plan_of(language);
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
        let length_of = |symbol: u8| language.string_symbols[symbol as usize].len();
        buffers.raw_strings.sort_unstable_by(|(a_at, a_symbol), (b_at, b_symbol)|
                a_at.cmp(b_at).then_with(|| length_of(*b_symbol).cmp(&length_of(*a_symbol))));
    }
    if plan.sorted_kinds[COMMENTS as usize] { buffers.comments.sort_unstable() }
}

fn take_symbols_at(at: usize, line_bytes: &[u8], plan: &ScanPlan, buffers: &mut ScanBuffers) {
    let mut cursor = plan.first[line_bytes[at] as usize];
    while cursor != NO_SLOT {
        let index = cursor as usize;
        let slot = plan.slots[index];
        cursor = slot.next;

        // Each symbol is searched without overlapping itself, the way one pass per symbol used to
        // behave: "///" holds one "//" and not two
        if at < buffers.consumed[index] { continue }
        let matched = match slot.len {
            1 => true,
            2 => line_bytes.get(at + 1) == Some(&slot.second),
            _ => line_bytes[at..].starts_with(&plan.symbols[index])
        };
        if !matched { continue }
        // An escape cancels a string symbol and nothing else
        if slot.kind == STRINGS && at != 0 && !is_not_escaped(at, line_bytes) { continue }

        buffers.consumed[index] = at + slot.len as usize;
        match slot.kind {
            STRINGS => buffers.raw_strings.push((at, slot.symbol)),
            COMMENTS => buffers.comments.push(at),
            COM_STARTS => buffers.com_starts.push(at),
            _ => buffers.com_ends.push(at)
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
    language_map: &HashMap<String,Language>, keyword_matcher: Option<&KeywordMatcher>, config: &Configuration)
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

    if buf.capacity() > MAX_RETAINED_FILE_BUFFER_BYTES {
        *buf = String::new();
    }

    Ok(file_stats)
}

// str::lines() splits on a char pattern, which reaches the standard library's own byte search: a
// SWAR loop over two usize words at a time. The memchr crate is already a dependency and searches
// the same byte with SIMD. Same lines out, including the trailing '\r' that lines() drops.
struct LineIter<'a> {
    contents: &'a str,
    newlines: memchr::Memchr<'a>,
    start: usize,
}

impl<'a> Iterator for LineIter<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        match self.newlines.next() {
            Some(at) => {
                let mut end = at;
                if end > self.start && self.contents.as_bytes()[end - 1] == b'\r' {
                    end -= 1;
                }
                let line = &self.contents[self.start..end];
                self.start = at + 1;
                Some(line)
            },
            None => {
                if self.start >= self.contents.len() {
                    return None;
                }
                let line = &self.contents[self.start..];
                self.start = self.contents.len();
                Some(line)
            }
        }
    }
}

fn lines_of(contents: &str) -> LineIter<'_> {
    LineIter { contents, newlines: memchr::memchr_iter(b'\n', contents.as_bytes()), start: 0 }
}

fn parse_lines(contents: &str, language: &Language, keyword_matcher: Option<&KeywordMatcher>, config: &Configuration,
    buffers: &mut ParseBuffers) -> FileStats
{
    let ParseBuffers { scan, alias_indices, .. } = buffers;
    let mut file_stats = match config.hidden.keywords {
        true => FileStats::default(),
        false => FileStats::with_keywords(&language.keywords)
    };
    let mut is_comment_closed = true;
    let mut open_str_symbol = None;
    for raw_line in lines_of(contents) {
        file_stats.incr_lines();

        // Ascii-only trimming, since the unicode whitespace classification of trim() costs
        // a significant part of the total run time, for lines that are code either way
        let line = raw_line.trim_ascii();
        if line.is_empty() { continue; }

        // Two different parsing functions to skip the unnecessary checks for langs that don't support multiline comments
        // for performance reasons
        let line_info = 
        if language.supports_multiline_comments() {
            get_bounds_w_multiline_comments(line, language, is_comment_closed, &open_str_symbol, scan)
        } else {
            get_bounds_only_single_line_comments(line, language, &open_str_symbol, scan)
        };

        is_comment_closed = !line_info.is_comment_open_after;
        open_str_symbol = line_info.open_str_sybol_after;

        if let Some(x) = line_info.cleansed_string {
            let cleansed = x.trim_ascii();
            // A line with no letter and no digit left after the strings and the comments were
            // stripped is punctuation that the language required, not something the programmer
            // said: '}', '});', '],', ')'. Bytes above 0x7f count as content, so that an identifier
            // written in a non-latin alphabet reads as code instead of looking like punctuation.
            let is_no_content = !line_info.has_string_literal
                    && !cleansed.bytes().any(|b| b.is_ascii_alphanumeric() || b >= 0x80);
            if config.braces_as_code || !is_no_content {
                file_stats.incr_code_lines();
                if !config.hidden.keywords && let Some(matcher) = keyword_matcher {
                    add_keywords_if_any(cleansed, matcher, &mut file_stats, alias_indices);
                }
            }
        } else if line_info.has_string_literal {
            file_stats.incr_code_lines();
        } else {
            file_stats.incr_comment_lines();
        }
    }

    file_stats
}


// cleansed_string can contain normal code string or curly braces or strings
#[derive(Debug, PartialEq)]
struct LineInfo<'a> {
    cleansed_string: Option<Cow<'a, str>>,
    has_string_literal: bool,
    is_comment_open_after: bool,
    open_str_sybol_after: Option<u8>
}


fn get_bounds_only_single_line_comments<'a>(line: &'a str, language: &Language, open_str_symbol: &Option<u8>,
    buffers: &'a mut ScanBuffers) -> LineInfo<'a>
{
    scan_line(line, language, buffers);
    resolve_string_delimiters(language, open_str_symbol, buffers);
    let ScanBuffers { strings: str_indices, string_symbols: str_symbols, comments: comment_indices, relevant, .. } = buffers;

    if open_str_symbol.is_some() && str_indices.is_empty() {
        return LineInfo::none_str(false, true, *open_str_symbol);
    }

    if str_indices.is_empty() && comment_indices.is_empty() {
        return LineInfo::whole_line(line, false);
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
            let index_after = str_indices[str_counter] + 1;
            
            if index_after >= line.len() {
                if relevant.is_empty() {return LineInfo::none_all(true);}
                else {return LineInfo::with_buffer(relevant,true);}
            } 
            
            is_str_open_m = false;
            str_counter += 1;
            if !has_more_strs(str_counter) && is_str_open_m {
                return get_LineInfo_with_str_symbol(relevant, str_symbols[str_counter-1]);
            }
            
            advance_comment_counter_until(index_after, &mut comment_counter);
            slice_start_index = index_after;
            has_string_literal = true;
        } else {
            if next_symbol_is_string(comment_counter, str_counter) {
                let this_index = str_indices[str_counter];
                relevant.push_str(&line[slice_start_index..this_index]);
                str_counter += 1;
                if !has_more_strs(str_counter) {
                    return get_LineInfo_with_str_symbol(relevant, str_symbols[str_counter-1]);
                }
                
                is_str_open_m = true;
                has_string_literal = true;
            } else if next_symbol_is_comment(comment_counter, str_counter) {
                relevant.push_str(&line[slice_start_index..comment_indices[comment_counter]]);
                
                if relevant.is_empty() {return LineInfo::none_str(false, has_string_literal, None);}
                else {return LineInfo::buffered(relevant, has_string_literal, false, None);}
            } else {
                relevant.push_str(&line[slice_start_index..line.len()]);
                return LineInfo::with_buffer(relevant, has_string_literal);
            }
        }
    }
}

fn get_bounds_w_multiline_comments<'a>(line: &'a str, language: &Language, is_comment_closed: bool,
    open_str_symbol: &Option<u8>, buffers: &'a mut ScanBuffers) -> LineInfo<'a>
{
    scan_line(line, language, buffers);
    resolve_string_delimiters(language, open_str_symbol, buffers);
    let ScanBuffers { strings: str_indices, string_symbols: str_symbols, comments: comment_indices,
            com_starts: com_start_indices, com_ends: com_end_indices, relevant, .. } = buffers;

    if is_comment_closed {
        if open_str_symbol.is_some() && str_indices.is_empty() {
            return LineInfo::none_str(false, true, *open_str_symbol);
        }
    } else {
        if com_end_indices.is_empty() {
            return LineInfo::with_open_comment();
        }
    }

    // A '//' that sits inside a '*/' is part of it and not a comment of its own, and a '/*' that
    // sits inside a comment never opens one
    comment_indices.retain(|x| !is_intersecting_with_multi_line_end_symbol(*x, language.multiline_end_len(), com_end_indices));
    com_start_indices.retain(|x| !is_intersecting_with_comment_symbol(*x, comment_indices));

    if !com_end_indices.is_empty() && !com_start_indices.is_empty() {
        resolve_double_counting_of_adjacent_start_and_end_symbols(com_start_indices, com_end_indices,
            !is_comment_closed, language.multiline_start_len());
    }

    if str_indices.is_empty() && comment_indices.is_empty() && com_start_indices.is_empty() && com_end_indices.is_empty() {
        return LineInfo::whole_line(line, false);
    }

    let (mut start_com_counter, mut end_com_counter, mut str_counter, mut comment_counter) = (0,0,0,0); 
    let (mut is_com_open_m, mut is_str_open_m) = (!is_comment_closed, open_str_symbol.is_some());

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
        if has_more_starts(start_counter) && comment_indices[comment_counter] > com_start_indices[start_counter] {
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
        if has_more_starts(start_counter) && str_indices[str_counter] > com_start_indices[start_counter] {
            return false;
        }
        true
    };
    let next_symbol_is_com_start = |comment_counter: usize, str_counter: usize,
        start_counter: usize| {
        if !has_more_starts(start_counter) {return false;}
        if has_more_comments(comment_counter) && com_start_indices[start_counter] > comment_indices[comment_counter] {
            return false;
        }
        if has_more_strs(str_counter) && com_start_indices[start_counter] > str_indices[str_counter] {
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
        while *start_counter < com_start_indices.len() && com_start_indices[*start_counter] < index {
            *start_counter += 1;
        }
        while *end_counter < com_end_indices.len() && com_end_indices[*end_counter] < index {
            *end_counter += 1;
        }
    };
    let skipped_com_end_symbol = |last_symbol_index, end_com_counter, cur_index| {
        has_more_ends(end_com_counter) && com_end_indices[end_com_counter] < cur_index && com_end_indices[end_com_counter] >= last_symbol_index
    };

    let mut has_string_literal = false;
    let mut slice_start_index = 0;
    let mut last_symbol_index = 0;
    loop {
        if is_str_open_m {
            last_symbol_index = str_indices[str_counter];
            let index_after = last_symbol_index + 1;
            if index_after >= line.len() {
                if relevant.is_empty() {return LineInfo::none_all(true);}
                else {return LineInfo::with_buffer(relevant,true);}
            } 
            
            progress_counters_after(last_symbol_index, &mut comment_counter, &mut str_counter,
                    &mut start_com_counter, &mut end_com_counter);

            is_str_open_m = false;
            str_counter += 1;
            has_string_literal = true;
            slice_start_index = index_after;
        } else if is_com_open_m {
            if end_com_counter == com_end_indices.len() {
                return LineInfo::buffered(relevant, has_string_literal, true, None);
            }
            last_symbol_index = com_end_indices[end_com_counter];
            let index_after = last_symbol_index + language.multiline_end_len();
            if index_after >= line.len() {
                if relevant.is_empty() {return LineInfo::none_all(has_string_literal);}
                else {return LineInfo::with_buffer(relevant,has_string_literal);}
            } 

            is_com_open_m = false;
            progress_counters_after(last_symbol_index, &mut comment_counter, &mut str_counter,
                    &mut start_com_counter, &mut end_com_counter);
            end_com_counter += 1;

            if has_more_strs(str_counter) && str_indices[str_counter] == index_after {
                is_str_open_m = true;
            } else if has_more_starts(start_com_counter) && com_start_indices[start_com_counter] == index_after {
                is_com_open_m = true;
            } else {
                slice_start_index = index_after; 
            }
        } else {
            if next_symbol_is_comment(comment_counter, str_counter, start_com_counter) {
                relevant.push_str(&line[slice_start_index..comment_indices[comment_counter]]);
                if relevant.is_empty() {return LineInfo::none_all(has_string_literal);}
                else {return LineInfo::with_buffer(relevant,has_string_literal);}
            } else if next_symbol_is_string(comment_counter, str_counter, start_com_counter) {
                let this_index = str_indices[str_counter];
                if skipped_com_end_symbol(last_symbol_index, end_com_counter, this_index) {
                    end_com_counter += 1;
                }
                relevant.push_str(&line[slice_start_index..this_index]);
                str_counter += 1;
                if !has_more_strs(str_counter) {
                    return get_LineInfo_with_str_symbol(relevant, str_symbols[str_counter-1]);
                }
                
                is_str_open_m = true;
                has_string_literal = true;
                last_symbol_index = this_index;
            } else if next_symbol_is_com_start(comment_counter, str_counter, start_com_counter) {
                let this_index = com_start_indices[start_com_counter];
                if skipped_com_end_symbol(last_symbol_index, end_com_counter, this_index) {
                    end_com_counter += 1;
                }

                relevant.push_str(&line[slice_start_index..this_index]);
                if !has_more_ends(end_com_counter) {
                    if relevant.is_empty() {return LineInfo::with_open_comment();}
                    else {return LineInfo::buffered(relevant, has_string_literal, true, None);}
                }
                
                is_com_open_m = true;
                start_com_counter += 1;
                last_symbol_index = this_index;
            } else {
                relevant.push_str(&line[slice_start_index..line.len()]);
                return LineInfo::with_buffer(relevant, has_string_literal);
            }
        }
    }
}

fn get_LineInfo_with_str_symbol<'a>(relevant: &'a str, str_symbol: u8) -> LineInfo<'a> {
    if relevant.is_empty() {
        LineInfo::with_open_symbol(str_symbol)
    } else {
        LineInfo::buffered(relevant, true, false, Some(str_symbol))
    }
}

fn resolve_double_counting_of_adjacent_start_and_end_symbols(start_indices: &mut Vec<usize>,
    end_indices: &mut Vec<usize>, is_comment_open: bool, multiline_len: usize) 
{
   fn resolve_collision(start_indices: &mut Vec<usize>, end_indices: &mut Vec<usize>, start_counter: &mut usize, 
       end_counter: &mut usize, is_comment_open_m: &mut bool, multiline_len: usize)
   {
       if *is_comment_open_m {
           start_indices.remove(*start_counter);
           if *start_counter < start_indices.len() && start_indices[*start_counter] <
                   end_indices[*end_counter] + multiline_len {
               start_indices.remove(*start_counter);
           }
           *end_counter += 1;
       } else {
           end_indices.remove(*end_counter);
           if *end_counter < end_indices.len() && end_indices[*end_counter] <
                   start_indices[*start_counter] + multiline_len {
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

       let start_index = start_indices[start_counter];
       let end_index = end_indices[end_counter];

       if end_index > start_index && end_index < start_index + multiline_len ||
                start_index > end_index && start_index < end_index + multiline_len {
            resolve_collision(start_indices, end_indices, &mut start_counter, &mut end_counter, &mut is_comment_open_m, multiline_len);
       } else {
           if start_index < end_index {
               start_counter += 1;
               if start_counter < start_indices.len() {
                   if start_indices[start_counter] > end_index {
                       is_comment_open_m = true;
                   }
               } else {
                   break;
               }
           }
           else {
               end_counter += 1;
               if end_counter < end_indices.len() {
                   if end_indices[end_counter] > start_counter {
                       is_comment_open_m = false;
                   }
               } else {
                   break;
               }
           }
       }
   }
}


fn add_keywords_if_any(cleansed: &str, matcher: &KeywordMatcher, file_stats: &mut FileStats, indices: &mut Vec<usize>) {
    for (alias_finder, alias_len, keyword_index) in &matcher.aliases_with_indices {
        count_alias_occurrences(cleansed, alias_finder, *alias_len, *keyword_index, file_stats, indices);
    }
}

fn count_alias_occurrences(cleansed: &str, alias_finder: &memmem::Finder<'_>, alias_len: usize, keyword_index: usize,
    file_stats: &mut FileStats, indices: &mut Vec<usize>)
{
    fn is_acceptable_prefix(prefix: &str) -> bool {
        prefix.is_empty() || prefix.ends_with(' ') || prefix.ends_with('}') || prefix.ends_with('{') || prefix.ends_with(',')
    }

    fn is_acceptable_suffix(suffix: &str) -> bool {
        suffix.is_empty() || suffix.starts_with(' ') || suffix.starts_with('}') || suffix.starts_with('{') || suffix.starts_with(',')
    }

    indices.clear();
    indices.extend(alias_finder.find_iter(cleansed.as_bytes()));
    if indices.is_empty() {return;}

    //ignore indices that are directly next to each other
    let mut counter = 0;
    while !indices.is_empty() && counter < indices.len()-1 {
        if indices[counter] + alias_len == indices[counter+1] {
            indices.remove(counter);
            indices.remove(counter);
        }
        counter += 1;
    }
    if indices.is_empty() {return};

    // What sits before an occurrence reaches back to the end of the previous one, what sits after it
    // reaches the start of the next. Read where they are, instead of one slice per gap in a Vec.
    for i in 0..indices.len() {
        let previous_end = if i == 0 { 0 } else { indices[i-1] + alias_len };
        let next_start = if i + 1 == indices.len() { cleansed.len() } else { indices[i+1] };
        if is_acceptable_prefix(&cleansed[previous_end..indices[i]])
                && is_acceptable_suffix(&cleansed[indices[i]+alias_len..next_start]) {
            file_stats.incr_keyword(keyword_index);
        }
    }
}

// Every string symbol the scan found, reduced to the ones that actually open or close a string.
// The number of symbols a language declares is not fixed: the one rule is that only the symbol
// that opened a string can close it, so anything of another kind in between is text.
fn resolve_string_delimiters(language: &Language, open_str_symbol: &Option<u8>, buffers: &mut ScanBuffers) {
    let ScanBuffers { raw_strings, strings, string_symbols, .. } = buffers;
    let length_of = |symbol: u8| language.string_symbols[symbol as usize].len();

    let mut open = *open_str_symbol;
    let mut consumed_up_to = 0;

    for &(at, symbol) in raw_strings.iter() {
        // What sits inside a symbol that was already taken is part of it, not a symbol of its own
        if at < consumed_up_to {
            continue;
        }
        match open {
            Some(open_symbol) if open_symbol != symbol => continue,
            Some(_) => open = None,
            None => open = Some(symbol)
        }
        consumed_up_to = at + length_of(symbol);
        strings.push(at);
        string_symbols.push(symbol);
    }
}

fn is_intersecting_with_multi_line_end_symbol(index: usize, symbol_len: usize, end_vec: &[usize]) -> bool {
    for i in end_vec {
        if index < symbol_len {
            if *i == 0 {return true;}
        } else {
            if *i == index - symbol_len + 1 {return true;}    
        }
    }

    false
}

fn is_intersecting_with_comment_symbol(index: usize, comments_vec: &[usize]) -> bool {
    for i in comments_vec {
        if *i == index + 1 {return true;} 
    }

    false
}


impl<'a> LineInfo<'a> {
    pub fn none_str(is_comment_open_after: bool, has_string_literal: bool, open_str_sybol_after: Option<u8>) -> LineInfo<'a> {
        LineInfo {
            cleansed_string: None,
            has_string_literal,
            is_comment_open_after,
            open_str_sybol_after
        }
    }

    pub fn with_buffer(cleansed_string: &'a str, has_string_literal: bool) -> LineInfo<'a> {
        LineInfo {
            cleansed_string: Some(Cow::Borrowed(cleansed_string)),
            has_string_literal,
            is_comment_open_after : false,
            open_str_sybol_after : None
        }
    }

    pub fn buffered(cleansed_string: &'a str, has_string_literal: bool, is_comment_open_after: bool,
        open_str_sybol_after: Option<u8>) -> LineInfo<'a>
    {
        LineInfo {
            cleansed_string: Some(Cow::Borrowed(cleansed_string)),
            has_string_literal,
            is_comment_open_after,
            open_str_sybol_after
        }
    }

    pub fn whole_line(line: &'a str, has_string_literal: bool) -> LineInfo<'a> {
        LineInfo {
            cleansed_string: Some(Cow::Borrowed(line)),
            has_string_literal,
            is_comment_open_after : false,
            open_str_sybol_after : None
        }
    }

    pub fn with_open_comment() -> LineInfo<'a> {
        LineInfo {
            cleansed_string: None,
            has_string_literal: false,
            is_comment_open_after: true,
            open_str_sybol_after: None
        }
    }

    pub fn with_open_symbol(symbol: u8) -> LineInfo<'a> {
        LineInfo {
            cleansed_string: None,
            has_string_literal: true,
            is_comment_open_after: false,
            open_str_sybol_after : Some(symbol)
        }
    }

    #[cfg(test)]
    pub fn from_slice(slice: &'a str) -> LineInfo<'a> {
        LineInfo {
            cleansed_string: Some(Cow::Borrowed(slice)),
            has_string_literal: false,
            is_comment_open_after : false,
            open_str_sybol_after : None
        }
    }

    #[cfg(test)]
    pub fn from_slice_w_literal(slice: &'a str) -> LineInfo<'a> {
        LineInfo {
            cleansed_string: Some(Cow::Borrowed(slice)),
            has_string_literal: true,
            is_comment_open_after : false,
            open_str_sybol_after : None
        }
    }

    pub fn none_all(has_string_literal: bool) -> LineInfo<'a> {
        LineInfo {
            cleansed_string: None,
            has_string_literal,
            is_comment_open_after : false,
            open_str_sybol_after : None
        }
    }

    #[cfg(test)]
    pub fn new(cleansed_string: Option<String>, has_string_literal: bool, is_comment_open_after: bool, open_str_sybol_after: Option<u8>) -> LineInfo<'a> {
        LineInfo {
            cleansed_string: cleansed_string.map(Cow::Owned),
            has_string_literal,
            is_comment_open_after,
            open_str_sybol_after
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    // The parser is handed its working memory by the consumer thread that owns it. A test cares
    // about one line at a time, so it gets a fresh one and reads the result out.
    // The cleansed line lives in the buffers the consumer thread owns, so a LineInfo borrows them.
    // A test owns its buffers for one call only, and takes the string with it on the way out.
    fn detached<'a>(info: LineInfo<'_>) -> LineInfo<'a> {
        LineInfo {
            cleansed_string: info.cleansed_string.map(|x| Cow::Owned(x.into_owned())),
            has_string_literal: info.has_string_literal,
            is_comment_open_after: info.is_comment_open_after,
            open_str_sybol_after: info.open_str_sybol_after
        }
    }

    fn bounds_multi<'a>(line: &'a str, language: &Language, is_comment_closed: bool, open_str_symbol: &Option<u8>) -> LineInfo<'a> {
        let mut buffers = ScanBuffers::default();
        detached(get_bounds_w_multiline_comments(line, language, is_comment_closed, open_str_symbol, &mut buffers))
    }

    fn bounds_single<'a>(line: &'a str, language: &Language, open_str_symbol: &Option<u8>) -> LineInfo<'a> {
        let mut buffers = ScanBuffers::default();
        detached(get_bounds_only_single_line_comments(line, language, open_str_symbol, &mut buffers))
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
        let mut buffers = ScanBuffers::default();
        scan_line(line, language, &mut buffers);
        buffers.comments.retain(|x| !is_intersecting_with_multi_line_end_symbol(*x, language.multiline_end_len(), com_end_indices));
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
        string_symbols : vec!["\"".to_owned()],
        comment_symbols : vec!["//".to_owned()],
        multiline_comment_start_symbol : Some("/*".to_owned()),
        multiline_comment_end_symbol : Some("*/".to_owned()),
        keywords : vec![CLASS.clone(),INTERFACE.clone()],
        scan_plan : std::sync::OnceLock::new()
    });

    static PHP : LazyLock<Language> = LazyLock::new(|| Language {
        name : "PHP".to_owned(),
        extensions : vec!["php".to_owned()],
        string_symbols : vec!["\"".to_owned(),"'".to_owned()],
        comment_symbols : vec!["//".to_owned(),"#".to_owned()],
        multiline_comment_start_symbol : Some("/*".to_owned()),
        multiline_comment_end_symbol : Some("*/".to_owned()),
        keywords : vec![CLASS.clone()],
        scan_plan : std::sync::OnceLock::new()
    });

    static PYTHON : LazyLock<Language> = LazyLock::new(|| Language {
        name : "py".to_owned(),
        extensions : vec!["py".to_owned()],
        string_symbols : vec!["\"".to_owned(),"'".to_owned()],
        comment_symbols : vec!["#".to_owned()],
        multiline_comment_start_symbol : None,
        multiline_comment_end_symbol : None,
        keywords : vec![CLASS.clone()],
        scan_plan : std::sync::OnceLock::new()
    });

    static RUST : LazyLock<Language> = LazyLock::new(|| Language {
        name : "rust".to_owned(),
        extensions : vec!["rs".to_owned()],
        string_symbols : vec!["\"".to_owned()],
        comment_symbols : vec!["//".to_owned()],
        multiline_comment_start_symbol : Some("/*".to_owned()),
        multiline_comment_end_symbol : Some("*/".to_owned()),
        keywords : vec![STRUCT.clone(),ENUM.clone(),TRAIT.clone()],
        scan_plan : std::sync::OnceLock::new()
    });

    // Four string symbols and three comment ones, which no language file could express before: with
    // three the old merge silently scanned for the first alone, and with more than two comment
    // symbols it scanned for the first two.
    static PYTHON_FULL : LazyLock<Language> = LazyLock::new(|| Language {
        name : "py".to_owned(),
        extensions : vec!["py".to_owned()],
        string_symbols : vec!["\"\"\"".to_owned(), "'''".to_owned(), "\"".to_owned(), "'".to_owned()],
        comment_symbols : vec!["#".to_owned(), "//".to_owned(), "--".to_owned()],
        multiline_comment_start_symbol : None,
        multiline_comment_end_symbol : None,
        keywords : vec![CLASS.clone()],
        scan_plan : std::sync::OnceLock::new()
    });

    static LANGUAGE_MAP_REF : LazyLock<Arc<HashMap<String,Language>>> =
            LazyLock::new(|| Arc::new(io_handler::parse_supported_languages_to_map(&LOCAL_APP_PATHS.languages_dir).unwrap().0));

    static JAVA_MATCHER : LazyLock<KeywordMatcher> = LazyLock::new(|| KeywordMatcher::build(&JAVA).unwrap());

    fn matcher_for(lang_name: &str) -> Option<KeywordMatcher> {
        KeywordMatcher::build(LANGUAGE_MAP_REF.get(lang_name).unwrap())
    }

    fn content_info_of(stats: FileStats, lang_name: &str) -> LanguageContentInfo {
        LanguageContentInfo::from_file_stats(stats, &LANGUAGE_MAP_REF.get(lang_name).unwrap().keywords)
    }

    #[test]
    fn test_correct_parsing_of_test_dir() {
        let mut buf = String::with_capacity(150);

        let mut config = Configuration::new(vec!["a".to_owned()]);
        let result = parse_file(Path::new("test_dir/lang_files/a.txt"), "Java", &mut buf, &mut ParseBuffers::default(), &LANGUAGE_MAP_REF, matcher_for("Java").as_ref(), &config);
        let result = content_info_of(result.unwrap(), "Java");
        assert_eq!(LanguageContentInfo::new(44, 13, 15, hashmap!("classes".to_owned()=>3,"interfaces".to_owned()=>0)), result);
        buf.clear();
        config.set_hidden(config_manager::Hidden {keywords: true, ..Default::default()});
        let result = parse_file(Path::new("test_dir/lang_files/a.txt"), "Java", &mut buf, &mut ParseBuffers::default(), &LANGUAGE_MAP_REF, matcher_for("Java").as_ref(), &config);
        let result = content_info_of(result.unwrap(), "Java");
        assert_eq!(LanguageContentInfo::new(44, 13, 15, hashmap!()), result);
        buf.clear();
        config.set_hidden(config_manager::Hidden::default());
        let result = parse_file(Path::new("test_dir/lang_files/a.txt"), "C#", &mut buf, &mut ParseBuffers::default(), &LANGUAGE_MAP_REF, matcher_for("C#").as_ref(), &Configuration::new(vec!["a".to_owned()]));
        let result = content_info_of(result.unwrap(), "C#");
        assert_eq!(LanguageContentInfo::new(44, 13, 15, hashmap!("structs".to_owned()=>0,"classes".to_owned()=>3,"interfaces".to_owned()=>0)), result);
        buf.clear();
        
        let result = parse_file(Path::new("test_dir/lang_files/d.txt"), "C#", &mut buf, &mut ParseBuffers::default(), &LANGUAGE_MAP_REF, matcher_for("C#").as_ref(), &Configuration::new(vec!["a".to_owned()]));
        let result = content_info_of(result.unwrap(), "C#");
        assert_eq!(LanguageContentInfo::new(19, 7, 10, hashmap!("structs".to_owned()=>0,"classes".to_owned()=>5,"interfaces".to_owned()=>0)), result);
        buf.clear();
        let result = parse_file(Path::new("test_dir/lang_files/d.txt"), "Java", &mut buf, &mut ParseBuffers::default(), &LANGUAGE_MAP_REF, matcher_for("Java").as_ref(), &Configuration::new(vec!["a".to_owned()]));
        let result = content_info_of(result.unwrap(), "Java");
        assert_eq!(LanguageContentInfo::new(19, 7, 10, hashmap!("classes".to_owned()=>5,"interfaces".to_owned()=>0)), result);
        buf.clear();

        let result = parse_file(Path::new("test_dir/lang_files/b.txt"), "Java", &mut buf, &mut ParseBuffers::default(), &LANGUAGE_MAP_REF, matcher_for("Java").as_ref(), &Configuration::new(vec!["a".to_owned()]));
        let result = content_info_of(result.unwrap(), "Java");
        assert_eq!(LanguageContentInfo::new(19, 11, 5, hashmap!("classes".to_owned()=>7,"interfaces".to_owned()=>0)), result);
        buf.clear();

        let result = parse_file(Path::new("test_dir/lang_files/c.txt"), "Python", &mut buf, &mut ParseBuffers::default(), &LANGUAGE_MAP_REF, matcher_for("Python").as_ref(), &Configuration::new(vec!["a".to_owned()]));
        let result = content_info_of(result.unwrap(), "Python");
        assert_eq!(LanguageContentInfo::new(11, 6, 3, hashmap!("classes".to_owned()=>2)), result);
        buf.clear();
    }

    // The flag had no test at all: everything that mentioned it checked that it could be parsed
    // from the command line or written to a config file, and nothing checked that it counts
    // anything differently, so it could have been disconnected from the parser entirely.
    #[test]
    fn braces_as_code_moves_the_no_content_lines_into_code() {
        let mut buf = String::with_capacity(150);
        let path = Path::new("test_dir/lang_files/a.txt");
        let count_with = |flag: bool, buf: &mut String| {
            let mut config = Configuration::new(vec!["a".to_owned()]);
            config.set_braces_as_code(flag);
            let stats = parse_file(path, "Java", buf, &mut ParseBuffers::default(), &LANGUAGE_MAP_REF, matcher_for("Java").as_ref(), &config).unwrap();
            (stats.lines, stats.code_lines, stats.comment_lines)
        };

        // a.txt has 10 lines that are nothing but a brace, and 6 blank ones. The comments never
        // move, whatever the flag says, and the three categories always add up to the total.
        assert_eq!((44, 13, 15), count_with(false, &mut buf));
        buf.clear();
        assert_eq!((44, 23, 15), count_with(true, &mut buf));
    }

    #[test]
    fn finds_keywords_correctly() {
        let line = String::from("Hello world!");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA_MATCHER, &mut file_stats, &mut Vec::new());
        assert_eq!(make_file_stats(0,0), file_stats);

        let line = String::from("class");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA_MATCHER, &mut file_stats, &mut Vec::new());
        assert_eq!(make_file_stats(1,0), file_stats);

        let line = String::from("1class");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA_MATCHER, &mut file_stats, &mut Vec::new());
        assert_eq!(make_file_stats(0,0), file_stats);

        let line = String::from("hello class word!");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA_MATCHER, &mut file_stats, &mut Vec::new());
        assert_eq!(make_file_stats(1,0), file_stats);

        let line = String::from("class class class");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA_MATCHER, &mut file_stats, &mut Vec::new());
        assert_eq!(make_file_stats(3,0), file_stats);

        let line = String::from("classclass");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA_MATCHER, &mut file_stats, &mut Vec::new());
        assert_eq!(make_file_stats(0,0), file_stats);

        let line = String::from("hello,class{word!");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA_MATCHER, &mut file_stats, &mut Vec::new());
        assert_eq!(make_file_stats(1,0), file_stats);
        
        let line = String::from("classe,");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA_MATCHER, &mut file_stats, &mut Vec::new());
        assert_eq!(make_file_stats(0,0), file_stats);
        
        let line = String::from("class interfaceclass classinterface interface");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA_MATCHER, &mut file_stats, &mut Vec::new());
        assert_eq!(make_file_stats(1,1), file_stats);
        
        let line = String::from("{class,interface}");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA_MATCHER, &mut file_stats, &mut Vec::new());
        assert_eq!(make_file_stats(1,1), file_stats);
        
        let line = String::from("{class.interface}");
        let mut file_stats =  FileStats::with_keywords(&[CLASS.clone(),INTERFACE.clone()]);
        add_keywords_if_any(&line, &JAVA_MATCHER, &mut file_stats, &mut Vec::new());
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

        // '"""' is one symbol and not three '"', so the docstring opens once and closes once
        let (indices, symbols) = indices_of(r#""""a docstring""""#);
        assert_eq!(vec![0, 14], indices);
        assert_eq!(vec![0u8, 0u8], symbols);

        // Only the symbol that opened closes: the quote of an apostrophe inside a string is text,
        // and so is a '"""' that turns up inside a plain '"'
        assert_eq!(vec![0, 10], indices_of(r#""it's fine""#).0);
        assert_eq!(vec![0, 8], indices_of(r#"'a """ b'"#).0);

        // A line that leaves one open reports its symbol, and the next line closes with that one
        let (indices, symbols) = indices_of(r#"x = """ open"#);
        assert_eq!((vec![4], vec![0u8]), (indices, symbols));
        let open = Some(0u8);
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

    // Every symbol of a kind has to be searched in the same pass, otherwise its positions would not
    // come out in the order they appear on the line and the merge below would read them out of turn.
    #[test]
    fn every_kind_is_searched_whole_in_a_single_pass() {
        for language in [&*JAVA, &*RUST, &*PHP, &*PYTHON, &*PYTHON_FULL] {
            let plan = ScanPlan::build(language);
            let searched = |byte: u8| plan.chunks.iter().filter(|c| c.bytes[..c.len as usize].contains(&byte)).count();

            for symbols in [&language.string_symbols, &language.comment_symbols] {
                let first_bytes = symbols.iter().map(|s| s.as_bytes()[0]).collect::<Vec<u8>>();
                let holding = plan.chunks.iter()
                        .filter(|c| first_bytes.iter().any(|b| c.bytes[..c.len as usize].contains(b)))
                        .count();
                assert_eq!(1, holding, "{} splits a kind across passes", language.name);
            }
            // and no byte is looked for twice, which would report the same symbol from two passes
            for symbol in language.string_symbols.iter().chain(language.comment_symbols.iter()) {
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
            let actual = lines_of(case).collect::<Vec<&str>>();
            assert_eq!(expected, actual, "disagreed on {case:?}");
        }
    }

    #[test]
    fn double_counting_resolution() {
        // /*Hello*//* world*//*
        let (mut start_indices, mut end_indices) = (vec![0,9,19],vec![7,17]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, false, 2);
        assert_eq!((start_indices, end_indices), (vec![0,9,19],vec![7,17]));
        // /**//**/
        let (mut start_indices, mut end_indices) = (vec![0,4],vec![2,6]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, false, 2);
        assert_eq!((start_indices, end_indices), (vec![0,4],vec![2,6]));
        // /*/**/*/
        let (mut start_indices, mut end_indices) = (vec![0,2],vec![4,6]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, false, 2);
        assert_eq!((start_indices, end_indices), (vec![0,2],vec![4,6]));

        // /* */*
        let (mut start_indices, mut end_indices) = (vec![0,4],vec![3]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, false, 2);
        assert_eq!((start_indices, end_indices), (vec![0],vec![3]));

        // */* /*/
        let (mut start_indices, mut end_indices) = (vec![1,4],vec![0,5]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, false, 2);
        assert_eq!((start_indices, end_indices), (vec![1],vec![5]));
        let (mut start_indices, mut end_indices) = (vec![1,4],vec![0,5]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, true, 2);
        assert_eq!((start_indices, end_indices), (vec![4],vec![0]));

        // /*/*/ */*/ /* */
        let (mut start_indices, mut end_indices) = (vec![0,2,7,11],vec![1,3,6,8,14]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, false, 2);
        assert_eq!((start_indices, end_indices), (vec![0,7,11],vec![3,14])); 
        let (mut start_indices, mut end_indices) = (vec![0,2,7,11],vec![1,3,6,8,14]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, true, 2);
        assert_eq!((start_indices, end_indices), (vec![7,11],vec![1,3,14])); 
 
        // /*/*/ */*/
        let (mut start_indices, mut end_indices) = (vec![0,2,7],vec![1,3,6,8]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, false, 2);
        assert_eq!((start_indices, end_indices), (vec![0,7],vec![3])); 
        let (mut start_indices, mut end_indices) = (vec![0,2,7],vec![1,3,6,8]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, true, 2);
        assert_eq!((start_indices, end_indices), (vec![7],vec![1,3]));

        // /* */*/*//*
        let (mut start_indices, mut end_indices) = (vec![0,4,6,9],vec![3,5,7]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, false, 2);
        assert_eq!((start_indices, end_indices), (vec![0,6,9],vec![3]));
        let (mut start_indices, mut end_indices) = (vec![0,4,6,9],vec![3,5,7]);
        resolve_double_counting_of_adjacent_start_and_end_symbols(&mut start_indices, &mut end_indices, true, 2);
        assert_eq!((start_indices, end_indices), (vec![0,6,9],vec![3]));
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
        assert_eq!(LineInfo::new(Some("[,,,,]".to_owned()),true,false,None),bounds_single(&line, &PYTHON, &None));
        let line = String::from("\\''\''");
        assert_eq!(LineInfo::new(Some("\\\'".to_owned()),true,false,Some(1u8)), bounds_single(&line, &PYTHON, &None));
        assert_eq!(LineInfo::none_all(true), bounds_single(&line, &PYTHON, &Some(1u8)));
        let line = String::from("\'\\'\\'\\\''"); 
        assert_eq!(LineInfo::new(None,true,false,None), bounds_single(&line, &PYTHON, &None));
        
        let single_str_opt = &Some(1u8);
        let double_str_opt = &Some(0u8);
        let single_str_li = LineInfo::with_open_symbol(1);
        let double_str_li = LineInfo::with_open_symbol(0);
    
        let line = String::from("Hello world!");
        assert_eq!(LineInfo::from_slice("Hello world!"),bounds_single(&line, &PYTHON, &None));
        assert_eq!(single_str_li,bounds_single(&line, &PYTHON, single_str_opt));
        
        //testing comments
        let line = String::from("#Hello world!");
        assert_eq!(single_str_li,bounds_single(&line, &PYTHON, single_str_opt));
        let line = String::from("Hello world!#");
        assert_eq!(LineInfo::from_slice("Hello world!"),bounds_single(&line, &PYTHON, &None));
        let line = String::from("Hello# world!");
        assert_eq!(LineInfo::from_slice("Hello"),bounds_single(&line, &PYTHON, &None));
        assert_eq!(single_str_li,bounds_single(&line, &PYTHON, single_str_opt));
        let line = String::from("Hello## world!");
        assert_eq!(LineInfo::from_slice("Hello"),bounds_single(&line, &PYTHON, &None));
        let line = String::from("#Hello# world!");
        assert_eq!(single_str_li,bounds_single(&line, &PYTHON, single_str_opt));
        
        //testing strings 
        let line = String::from("\"Hello world!#");
        assert_eq!(double_str_li,bounds_single(&line, &PYTHON, &None));
        let line = String::from("\"Hello\" world!");
        assert_eq!(LineInfo::from_slice_w_literal(" world!"),bounds_single(&line, &PYTHON, &None));
        assert_eq!(LineInfo::new(Some("Hello".to_owned()), true, false, Some(0u8)),bounds_single(&line, &PYTHON, double_str_opt));
        let line = String::from("Hello world!\"");
        assert_eq!(LineInfo::new(Some("Hello world!".to_owned()), true, false, Some(0u8)),bounds_single(&line, &PYTHON, &None));
        let line = String::from("\"'Hello'\" world!");
        assert_eq!(LineInfo::from_slice_w_literal(" world!"),bounds_single(&line, &PYTHON, &None));
        let line = String::from("'Hello' world!");
        assert_eq!(LineInfo::from_slice_w_literal(" world!"),bounds_single(&line, &PYTHON, &None));
        let line = String::from("'\"He'llo'\" world!'");
        assert_eq!(LineInfo::from_slice_w_literal("llo"),bounds_single(&line, &PYTHON, &None));
        assert_eq!(LineInfo::new(Some("He".to_owned()), true, false, Some(0u8)),bounds_single(&line, &PYTHON, double_str_opt));
        let line = String::from(r#""""Hello""#);
        assert_eq!(LineInfo::new(None, true, false, None), bounds_single(&line, &PYTHON, &None));
        assert_eq!(LineInfo::new(Some("Hello".to_owned()), true, false, Some(0u8)), bounds_single(&line, &PYTHON, double_str_opt));
        let line = String::from(r#"['⣯', '⣟"#); 
        assert_eq!(LineInfo::new(Some("[, ".to_owned()),true,false,Some(1u8)), bounds_single(&line, &PYTHON, &None));
        
        //test mixed
        let line = String::from("'Hello#' world!'");
        assert_eq!(LineInfo::new(Some(" world!".to_owned()), true, false, Some(1u8)),bounds_single(&line, &PYTHON, &None));
        assert_eq!(LineInfo::from_slice_w_literal("Hello"),bounds_single(&line, &PYTHON, single_str_opt));
        let line = String::from("'Hello'# world!'");
        assert_eq!(LineInfo::none_all(true),bounds_single(&line, &PYTHON, &None));
        assert_eq!(LineInfo::from_slice_w_literal("Hello"),bounds_single(&line, &PYTHON, single_str_opt));
        let line = String::from("''#Hello");
        assert_eq!(LineInfo::none_all(true),bounds_single(&line, &PYTHON, &None));
        let line = String::from("'''#'''Hello world!'");
        assert_eq!(LineInfo::new(Some("Hello world!".to_owned()), true, false, Some(1u8)),bounds_single(&line, &PYTHON, &None));
        assert_eq!(LineInfo::none_all(true),bounds_single(&line, &PYTHON, single_str_opt));
        assert_eq!(LineInfo::with_open_symbol(0),bounds_single(&line, &PYTHON, double_str_opt));
        let line = String::from("Hello'###'\"world!\"");
        assert_eq!(LineInfo::from_slice_w_literal("Hello"),bounds_single(&line, &PYTHON, &None));
        assert_eq!(LineInfo::none_all(true),bounds_single(&line, &PYTHON, single_str_opt));
        assert_eq!(LineInfo::new(Some("world!".to_owned()), true, false, Some(0u8)),bounds_single(&line, &PYTHON, double_str_opt));
        let line = String::from("\"//'''\"Hello'\"world!");
        assert_eq!(LineInfo::new(Some("Hello".to_owned()), true, false, Some(1u8)),bounds_single(&line, &PYTHON, &None));
        assert_eq!(LineInfo::from_slice_w_literal("world!"),bounds_single(&line, &PYTHON, single_str_opt));
        assert_eq!(LineInfo::new(Some("//".to_owned()), true, false, Some(0u8)),bounds_single(&line, &PYTHON, double_str_opt));
    }
    
    #[test]
    fn gets_bounds_JAVA() {
        let double_str_opt = &Some(0u8);

        let line = String::from("Hello world!");
        assert_eq!(LineInfo::with_open_comment(),bounds_multi(&line, &JAVA, false, &None));
        assert_eq!(LineInfo::with_open_symbol(0),bounds_multi(&line, &JAVA, true, double_str_opt));
        assert_eq!(LineInfo::from_slice("Hello world!"),bounds_multi(&line, &JAVA, true, &None));
        
        //testing only multiline comment combinations
        let line = String::from("*/Hello world!");
        assert_eq!(LineInfo::from_slice("Hello world!"),bounds_multi(&line, &JAVA, false, &None));
        assert_eq!(LineInfo::from_slice("*/Hello world!"),bounds_multi(&line, &JAVA, true, &None));
        let line = String::from("Hello/* ffd /**//*erer */ world!");
        assert_eq!(LineInfo::from_slice(" world!"),bounds_multi(&line, &JAVA, false, &None));
        assert_eq!(LineInfo::from_slice("Hello world!"),bounds_multi(&line, &JAVA, true, &None));
        let line = String::from("Hello*//**//**/ world!");
        assert_eq!(LineInfo::from_slice(" world!"),bounds_multi(&line, &JAVA, false, &None));
        assert_eq!(LineInfo::from_slice("Hello*/ world!"),bounds_multi(&line, &JAVA, true, &None));
        let line = String::from("*//*Hello/**/ world!");
        assert_eq!(LineInfo::from_slice(" world!"),bounds_multi(&line, &JAVA, false, &None));
        assert_eq!(LineInfo::from_slice("*/ world!"),bounds_multi(&line, &JAVA, true, &None));
        let line = String::from("Hello world*/");
        assert_eq!(LineInfo::none_all(false), bounds_multi(&line, &JAVA, false, &None));
        let line = String::from("*/Hello world!/**/");
        assert_eq!(LineInfo::from_slice("Hello world!"), bounds_multi(&line, &JAVA, false, &None));
        let line = String::from("Hello world*//**/");
        assert_eq!(LineInfo::none_all(false), bounds_multi(&line, &JAVA, false, &None));
        let line = String::from("*/He/**//*llo world*/!/**/");
        assert_eq!(LineInfo::from_slice("He!"), bounds_multi(&line, &JAVA, false, &None));
        let line = String::from("Hello world*/!");
        assert_eq!(LineInfo::from_slice("!"), bounds_multi(&line, &JAVA, false, &None));
        let line = String::from("/*H*/ello world/*!");
        assert_eq!(LineInfo::new(Some("ello world".to_string()), false, true, None), bounds_multi(&line, &JAVA, false, &None));
        assert_eq!(LineInfo::new(Some("ello world".to_string()), false, true, None), bounds_multi(&line, &JAVA, true, &None));
        let line = String::from("/*H*/e/*llo world!");
        assert_eq!(LineInfo::new(Some("e".to_string()), false, true, None), bounds_multi(&line, &JAVA, false, &None));
        
        //testing only string symbols
        let line = String::from("\"");
        assert_eq!(LineInfo::with_open_symbol(0), bounds_multi(&line, &JAVA, true, &None));
        let line = String::from("\"Hello\"");
        assert_eq!(LineInfo::new(Some("Hello".to_string()), true, false, Some(0u8)), bounds_multi(&line, &JAVA, true, double_str_opt));
        assert_eq!(LineInfo::none_all(true), bounds_multi(&line, &JAVA, true, &None));
        let line = String::from("\"\"Hello");
        assert_eq!(LineInfo::with_open_symbol(0), bounds_multi(&line, &JAVA, true, double_str_opt));
        assert_eq!(LineInfo::from_slice_w_literal("Hello"), bounds_multi(&line, &JAVA, true, &None));
        let line = String::from("\"\"");
        assert_eq!(LineInfo::with_open_symbol(0), bounds_multi(&line, &JAVA, true, double_str_opt));
        assert_eq!(LineInfo::none_all(true), bounds_multi(&line, &JAVA, true, &None));
        let line = String::from("\"\"Hello");
        assert_eq!(LineInfo::from_slice_w_literal("Hello"), bounds_multi(&line, &JAVA, true, &None));
        let line  = String::from("Hel\"\"lo");
        assert_eq!(LineInfo::from_slice_w_literal("Hello"), bounds_multi(&line, &JAVA, true, &None));
        let line = String::from("\"\"He\"\"\"ll\"o");
        assert_eq!(LineInfo::from_slice_w_literal("Heo"), bounds_multi(&line, &JAVA, true, &None));
        let line = String::from(r#""""Hello""#);
        assert_eq!(LineInfo::new(None, true, false, None), bounds_multi(&line, &JAVA, true, &None));
        assert_eq!(LineInfo::new(Some("Hello".to_owned()), true, false, Some(0u8)), bounds_multi(&line, &JAVA, true, double_str_opt));
        
        //testing only comments
        let line = String::from("//");
        assert_eq!(LineInfo::none_all(false), bounds_multi(&line, &JAVA, true, &None));
        let line = String::from("Hello//");
        assert_eq!(LineInfo::from_slice("Hello"), bounds_multi(&line, &JAVA, true, &None));
        assert_eq!(LineInfo::with_open_comment(), bounds_multi(&line, &JAVA, false, &None));
        assert_eq!(LineInfo::with_open_symbol(0), bounds_multi(&line, &JAVA, true, double_str_opt));
        let line = String::from("//Hello");
        assert_eq!(LineInfo::none_all(false), bounds_multi(&line, &JAVA, true, &None));
        let line = String::from("////Hello");
        assert_eq!(LineInfo::none_all(false), bounds_multi(&line, &JAVA, true, &None));
        let line = String::from("He//llo//");
        assert_eq!(LineInfo::from_slice("He"), bounds_multi(&line, &JAVA, true, &None));
        
        //testing mixed
        let line = String::from("\"\"\"//\"\"\"Hello world!");
        assert_eq!(LineInfo::from_slice_w_literal("Hello world!"),bounds_multi(&line, &JAVA, true, &None));
        assert_eq!(LineInfo::none_all(true),bounds_multi(&line, &JAVA, true, double_str_opt));
        let line = String::from("\"\"one\"//\"\"\"Hello world!");
        assert_eq!(LineInfo::from_slice_w_literal("oneHello world!"),bounds_multi(&line, &JAVA, true, &None));
        let line = String::from("\"He\"/*l*/lo//fd");
        assert_eq!(LineInfo::from_slice_w_literal("lo"), bounds_multi(&line, &JAVA, true, &None));
        assert_eq!(LineInfo::new(Some("He".to_string()), true, false, Some(0u8)), bounds_multi(&line, &JAVA, true, double_str_opt));
        assert_eq!(LineInfo::from_slice("lo"), bounds_multi(&line, &JAVA, false, &None));
        let line = String::from("//\"/**/dfd\"");
        assert_eq!(LineInfo::none_all(false), bounds_multi(&line, &JAVA, true, &None));
        assert_eq!(LineInfo::new(Some("dfd".to_string()), true, false, Some(0u8)), bounds_multi(&line, &JAVA, false, &None));
        assert_eq!(LineInfo::new(Some("dfd".to_string()), true, false, Some(0u8)), bounds_multi(&line, &JAVA, true, double_str_opt));
        
        let line  = String::from(
            "Hello /* \
            mefm \" */ \" \
            //*/world!"
        );
        assert_eq!(LineInfo::new(Some("Hello  ".to_string()), true, false, Some(0u8)), bounds_multi(&line, &JAVA, true, &None));
        assert_eq!(LineInfo::new(Some(" ".to_string()), true, false, Some(0u8)), bounds_multi(&line, &JAVA, false, &None));
        assert_eq!(LineInfo::new(Some(" */ ".to_string()), true, false, Some(0u8)), bounds_multi(&line, &JAVA, true, double_str_opt));
    }
}

