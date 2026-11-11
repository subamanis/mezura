// One pass over a 64-byte block answers two questions for every byte at once: is it a newline, and
// is it one of the bytes the scan plan searches. The answers come back as bitmasks, bit i for byte
// i, so the line splitting and the candidate question are read off them without a search per line.
// A second pass answers what a line is made of, the word bytes and the bytes that are not blank,
// so that a run of plain lines is classified off the masks without being read.

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::{__m256i, __m512i, _mm256_and_si256, _mm256_cmpeq_epi8, _mm256_movemask_epi8, _mm256_or_si256,
        _mm256_set1_epi8, _mm256_set_epi64x, _mm256_shuffle_epi8, _mm256_srli_epi16, _mm512_and_si512,
        _mm512_cmpeq_epi8_mask, _mm512_set1_epi8, _mm512_set_epi64, _mm512_shuffle_epi8, _mm512_srli_epi16,
        _mm512_test_epi8_mask};
#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::{uint8x16_t, vandq_u8, vceqq_u8, vcombine_u8, vcreate_u8, vdupq_n_u8, vgetq_lane_u64,
        vmaxvq_u8, vorrq_u8, vpaddq_u8, vreinterpretq_u64_u8};

pub(crate) const BLOCK_BYTES : usize = 64;
// The class of a byte is the AND of what its high nibble allows and what its low nibble allows,
// one table lookup each: a digit is high nibble 3 with a low one up to 9, a letter is high nibble
// 4 or 6 with a low one from 1, or 5 or 7 with a low one up to 10, a space is 0x20, a control
// space is 0x09, 0x0A, 0x0C or 0x0D, and anything from 0x80 up is above ASCII
#[cfg(target_arch = "x86_64")]
const DIGIT : u8 = 0x01;
#[cfg(target_arch = "x86_64")]
const LETTER_FROM_ONE : u8 = 0x02;
#[cfg(target_arch = "x86_64")]
const LETTER_UP_TO_TEN : u8 = 0x04;
#[cfg(target_arch = "x86_64")]
const ABOVE_ASCII : u8 = 0x08;
#[cfg(target_arch = "x86_64")]
const SPACE : u8 = 0x10;
#[cfg(target_arch = "x86_64")]
const CONTROL_SPACE : u8 = 0x20;
#[cfg(target_arch = "x86_64")]
const WORD_CLASSES : u8 = DIGIT | LETTER_FROM_ONE | LETTER_UP_TO_TEN | ABOVE_ASCII;
#[cfg(target_arch = "x86_64")]
const SPACE_CLASSES : u8 = SPACE | CONTROL_SPACE;
// Each table holds its sixteen entries once per 128-bit lane of the shuffle that reads it
#[cfg(target_arch = "x86_64")]
const LOW_NIBBLE_CLASSES_32 : [u8; 32] = repeat_table(build_low_nibble_classes());
#[cfg(target_arch = "x86_64")]
const HIGH_NIBBLE_CLASSES_32 : [u8; 32] = repeat_table(build_high_nibble_classes());
#[cfg(target_arch = "x86_64")]
const LOW_NIBBLE_CLASSES_64 : [u8; 64] = repeat_table(build_low_nibble_classes());
#[cfg(target_arch = "x86_64")]
const HIGH_NIBBLE_CLASSES_64 : [u8; 64] = repeat_table(build_high_nibble_classes());

#[derive(Debug, Clone, Copy)]
pub(crate) struct Masks {
    pub newlines: u64,
    pub candidates: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Classes {
    pub words: u64,
    pub content: u64,
}

// Which bytes are searched, and whether this machine can do it 64 or 32 bytes at a time
#[derive(Debug, Clone)]
pub(crate) struct MaskFinder {
    is_searched: [bool; 256],
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    searched: Box<[u8]>,
    #[cfg(target_arch = "x86_64")]
    avx512: bool,
    #[cfg(target_arch = "x86_64")]
    avx2: bool,
    #[cfg(target_arch = "aarch64")]
    neon: bool,
}

impl MaskFinder {
    pub(crate) fn of(searched: &[u8]) -> MaskFinder {
        let mut is_searched = [false; 256];
        for &byte in searched { is_searched[byte as usize] = true; }
        MaskFinder {
            is_searched,
            #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
            searched: searched.into(),
            #[cfg(target_arch = "x86_64")]
            avx512: std::is_x86_feature_detected!("avx512f") && std::is_x86_feature_detected!("avx512bw"),
            #[cfg(target_arch = "x86_64")]
            avx2: std::is_x86_feature_detected!("avx2"),
            #[cfg(target_arch = "aarch64")]
            neon: std::arch::is_aarch64_feature_detected!("neon"),
        }
    }

    // A block shorter than 64 bytes, the last of a file, answers for the bytes it has and zero for
    // the rest.
    #[allow(unsafe_code)]
    pub(crate) fn scan_block(&self, block: &[u8]) -> Masks {
        #[cfg(target_arch = "x86_64")]
        if block.len() == BLOCK_BYTES {
            // The features were checked once when the finder was built, which is the whole
            // requirement of calling a function compiled for them
            if self.avx512 { return unsafe { scan_block_avx512(block, &self.searched) }; }
            if self.avx2 { return unsafe { scan_block_avx2(block, &self.searched) }; }
        }
        #[cfg(target_arch = "aarch64")]
        if self.neon && block.len() == BLOCK_BYTES {
            return unsafe { scan_block_neon(block, &self.searched) };
        }
        scan_block_scalar(block, &self.is_searched)
    }

    // The word bytes and the bytes that are not blank, as 'has_word_byte' and 'trim_ascii' read them
    #[allow(unsafe_code)]
    pub(crate) fn classify_block(&self, block: &[u8]) -> Classes {
        #[cfg(target_arch = "x86_64")]
        if block.len() == BLOCK_BYTES {
            // The features were checked once when the finder was built, which is the whole
            // requirement of calling a function compiled for them
            if self.avx512 { return unsafe { classify_block_avx512(block) }; }
            if self.avx2 { return unsafe { classify_block_avx2(block) }; }
        }
        classify_block_scalar(block)
    }
}

#[allow(unsafe_code)]
pub(crate) fn is_ascii(bytes: &[u8]) -> bool {
    #[cfg(target_arch = "x86_64")]
    if std::is_x86_feature_detected!("avx2") {
        // Checked on the line above, which is the whole requirement of calling a function
        // compiled for the feature
        return unsafe { is_ascii_avx2(bytes) };
    }
    #[cfg(target_arch = "aarch64")]
    if std::arch::is_aarch64_feature_detected!("neon") {
        return unsafe { is_ascii_neon(bytes) };
    }
    bytes.is_ascii()
}

// The features of the AVX2 tier of the parser: the vectors, and the bit instructions every x86-64
// of the last fifteen years has
#[cfg(target_arch = "x86_64")]
pub(crate) fn has_avx2_tier() -> bool {
    std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("bmi1") && std::is_x86_feature_detected!("bmi2")
            && std::is_x86_feature_detected!("lzcnt") && std::is_x86_feature_detected!("popcnt")
}

// The AVX-512 tier adds the 64-byte compares and shuffles of the masks, which need no VBMI
#[cfg(target_arch = "x86_64")]
pub(crate) fn has_avx512_tier() -> bool {
    has_avx2_tier() && std::is_x86_feature_detected!("avx512f") && std::is_x86_feature_detected!("avx512bw")
}

// The newlines of a block that close a line holding a word byte, and those that close a line
// holding any byte that is not blank. Added to the mask of every byte that is not a newline, a
// class bit carries up to its line's newline and stops there, which sets that newline's bit. The
// line still open at the end of the block overflows, and that carry goes into the next block
#[inline(always)]
pub(crate) fn find_lines_holding_words_and_content(newlines: u64, classes: Classes, from_start: u64,
        carried: (bool, bool)) -> (u64, u64, (bool, bool)) {
    let not_newlines = !newlines;
    let (words, words_out) = add_with_carry(not_newlines, classes.words & from_start, carried.0);
    let (content, content_out) = add_with_carry(not_newlines, classes.content & from_start, carried.1);
    (words & newlines, content & newlines, (words_out, content_out))
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn is_ascii_avx2(bytes: &[u8]) -> bool {
    let mut high_bits = _mm256_set1_epi8(0);
    let (blocks, remainder) = bytes.as_chunks::<32>();
    for block in blocks {
        high_bits = _mm256_or_si256(high_bits, load_32(block));
    }
    _mm256_movemask_epi8(high_bits) == 0 && remainder.is_ascii()
}

fn scan_block_scalar(block: &[u8], is_searched: &[bool; 256]) -> Masks {
    let (mut newlines, mut candidates) = (0u64, 0u64);
    for (at, &byte) in block.iter().enumerate() {
        if byte == b'\n' { newlines |= 1 << at; }
        if is_searched[byte as usize] { candidates |= 1 << at; }
    }
    Masks { newlines, candidates }
}

fn classify_block_scalar(block: &[u8]) -> Classes {
    let (mut words, mut content) = (0u64, 0u64);
    for (at, &byte) in block.iter().enumerate() {
        if byte.is_ascii_alphanumeric() || byte >= 0x80 { words |= 1 << at; }
        if !byte.is_ascii_whitespace() { content |= 1 << at; }
    }
    Classes { words, content }
}

#[inline(always)]
fn add_with_carry(a: u64, b: u64, carry: bool) -> (u64, bool) {
    let (sum, first) = a.overflowing_add(b);
    let (sum, second) = sum.overflowing_add(carry as u64);
    (sum, first || second)
}

#[cfg(target_arch = "x86_64")]
const fn build_high_nibble_classes() -> [u8; 16] {
    let by_nibble = [CONTROL_SPACE, 0, SPACE, DIGIT, LETTER_FROM_ONE, LETTER_UP_TO_TEN, LETTER_FROM_ONE, LETTER_UP_TO_TEN];
    let mut table = [ABOVE_ASCII; 16];
    let mut high = 0;
    while high < 8 {
        table[high] = by_nibble[high];
        high += 1;
    }
    table
}

#[cfg(target_arch = "x86_64")]
const fn build_low_nibble_classes() -> [u8; 16] {
    let mut table = [0u8; 16];
    let mut nibble = 0;
    while nibble < 16 {
        let mut classes = ABOVE_ASCII;
        if nibble <= 9 { classes |= DIGIT; }
        if nibble >= 1 { classes |= LETTER_FROM_ONE; }
        if nibble <= 10 { classes |= LETTER_UP_TO_TEN; }
        if nibble == 0 { classes |= SPACE; }
        if matches!(nibble, 9 | 10 | 12 | 13) { classes |= CONTROL_SPACE; }
        table[nibble] = classes;
        nibble += 1;
    }
    table
}

#[cfg(target_arch = "x86_64")]
const fn repeat_table<const N: usize>(table: [u8; 16]) -> [u8; N] {
    let mut repeated = [0u8; N];
    let mut at = 0;
    while at < N {
        repeated[at] = table[at % 16];
        at += 1;
    }
    repeated
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw")]
fn scan_block_avx512(block: &[u8], searched: &[u8]) -> Masks {
    let bytes = load_64(block);
    let newlines = _mm512_cmpeq_epi8_mask(bytes, _mm512_set1_epi8(b'\n' as i8));
    let mut candidates = 0;
    for &byte in searched {
        candidates |= _mm512_cmpeq_epi8_mask(bytes, _mm512_set1_epi8(byte as i8));
    }
    Masks { newlines, candidates }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f,avx512bw")]
fn classify_block_avx512(block: &[u8]) -> Classes {
    let bytes = load_64(block);
    let nibble = _mm512_set1_epi8(0x0F);
    let low = _mm512_shuffle_epi8(load_64(&LOW_NIBBLE_CLASSES_64), _mm512_and_si512(bytes, nibble));
    let high = _mm512_shuffle_epi8(load_64(&HIGH_NIBBLE_CLASSES_64), _mm512_and_si512(_mm512_srli_epi16::<4>(bytes), nibble));
    let classes = _mm512_and_si512(low, high);
    Classes {
        words: _mm512_test_epi8_mask(classes, _mm512_set1_epi8(WORD_CLASSES as i8)),
        content: !_mm512_test_epi8_mask(classes, _mm512_set1_epi8(SPACE_CLASSES as i8)),
    }
}

// Eight lanes from eight little-endian words, which the compiler turns into one unaligned load
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx512f")]
fn load_64(bytes: &[u8]) -> __m512i {
    let word = |at: usize| i64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
    _mm512_set_epi64(word(56), word(48), word(40), word(32), word(24), word(16), word(8), word(0))
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn classify_block_avx2(block: &[u8]) -> Classes {
    let (low, high) = (load_32(&block[..32]), load_32(&block[32..64]));
    let (low_classes, high_classes) = (classify_half(low), classify_half(high));
    let (word_bits, space_bits, zero) = (_mm256_set1_epi8(WORD_CLASSES as i8), _mm256_set1_epi8(SPACE_CLASSES as i8),
            _mm256_set1_epi8(0));
    let lacks = |classes: __m256i, bits: __m256i| _mm256_cmpeq_epi8(_mm256_and_si256(classes, bits), zero);
    Classes {
        words: !gather_bits(lacks(low_classes, word_bits), lacks(high_classes, word_bits)),
        content: gather_bits(lacks(low_classes, space_bits), lacks(high_classes, space_bits)),
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn classify_half(half: __m256i) -> __m256i {
    let nibble = _mm256_set1_epi8(0x0F);
    let low = _mm256_shuffle_epi8(load_32(&LOW_NIBBLE_CLASSES_32), _mm256_and_si256(half, nibble));
    let high = _mm256_shuffle_epi8(load_32(&HIGH_NIBBLE_CLASSES_32), _mm256_and_si256(_mm256_srli_epi16::<4>(half), nibble));
    _mm256_and_si256(low, high)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn scan_block_avx2(block: &[u8], searched: &[u8]) -> Masks {
    let (low, high) = (load_32(&block[..32]), load_32(&block[32..64]));
    let newline = _mm256_set1_epi8(b'\n' as i8);
    let newlines = gather_bits(_mm256_cmpeq_epi8(low, newline), _mm256_cmpeq_epi8(high, newline));
    let (mut low_hits, mut high_hits) = (_mm256_set1_epi8(0), _mm256_set1_epi8(0));
    for &byte in searched {
        let wanted = _mm256_set1_epi8(byte as i8);
        low_hits = _mm256_or_si256(low_hits, _mm256_cmpeq_epi8(low, wanted));
        high_hits = _mm256_or_si256(high_hits, _mm256_cmpeq_epi8(high, wanted));
    }
    Masks { newlines, candidates: gather_bits(low_hits, high_hits) }
}

// Four lanes from four little-endian words, which the compiler turns into one unaligned load
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn load_32(bytes: &[u8]) -> __m256i {
    let word = |at: usize| i64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
    _mm256_set_epi64x(word(24), word(16), word(8), word(0))
}

// The top bit of every lane of the two halves, bit i for byte i of the block
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn gather_bits(low: __m256i, high: __m256i) -> u64 {
    (_mm256_movemask_epi8(low) as u32 as u64) | ((_mm256_movemask_epi8(high) as u32 as u64) << 32)
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
fn is_ascii_neon(bytes: &[u8]) -> bool {
    let mut high_bits = vdupq_n_u8(0);
    let (blocks, remainder) = bytes.as_chunks::<16>();
    for block in blocks {
        high_bits = vorrq_u8(high_bits, load_16(block));
    }
    vmaxvq_u8(high_bits) < 0x80 && remainder.is_ascii()
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
fn scan_block_neon(block: &[u8], searched: &[u8]) -> Masks {
    let quarters = [load_16(&block[..16]), load_16(&block[16..32]), load_16(&block[32..48]), load_16(&block[48..64])];
    let newline = vdupq_n_u8(b'\n');
    let newlines = gather_bits(quarters.map(|quarter| vceqq_u8(quarter, newline)));
    let mut hits = [vdupq_n_u8(0); 4];
    for &byte in searched {
        let wanted = vdupq_n_u8(byte);
        for (hit, quarter) in hits.iter_mut().zip(quarters) {
            *hit = vorrq_u8(*hit, vceqq_u8(quarter, wanted));
        }
    }
    Masks { newlines, candidates: gather_bits(hits) }
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
fn load_16(bytes: &[u8]) -> uint8x16_t {
    let word = |at: usize| u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
    vcombine_u8(vcreate_u8(word(0)), vcreate_u8(word(8)))
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
fn gather_bits(lanes: [uint8x16_t; 4]) -> u64 {
    let bit_of_lane = vcombine_u8(vcreate_u8(0x8040_2010_0804_0201), vcreate_u8(0x8040_2010_0804_0201));
    let [a, b, c, d] = lanes.map(|lane| vandq_u8(lane, bit_of_lane));
    let halves = vpaddq_u8(vpaddq_u8(a, b), vpaddq_u8(c, d));
    vgetq_lane_u64::<0>(vreinterpretq_u64_u8(vpaddq_u8(halves, halves)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Whole blocks of every shape: the fixed ones, every byte value across four blocks, and two
    // hundred random ones dense in the searched bytes
    fn every_block_shape() -> Vec<[u8; BLOCK_BYTES]> {
        let mut blocks = Vec::new();
        blocks.push([b'\n'; BLOCK_BYTES]);
        blocks.push([b'/'; BLOCK_BYTES]);
        blocks.push(std::array::from_fn(|i| if i % 7 == 0 { b'\n' } else if i % 5 == 0 { b'"' } else { b'x' }));
        for quarter in 0..4u8 {
            blocks.push(std::array::from_fn(|i| quarter * 64 + i as u8));
        }
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..200 {
            blocks.push(std::array::from_fn(|_| {
                seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17;
                match seed % 6 { 0 => b'\n', 1 => b'"', 2 => b'/', 3 => b'\'', _ => (seed >> 8) as u8 }
            }));
        }
        blocks
    }

    #[test]
    fn the_vector_path_answers_exactly_as_the_scalar_one() {
        let finder = MaskFinder::of(b"\"/'");
        let mut is_searched = [false; 256];
        for &byte in b"\"/'" { is_searched[byte as usize] = true; }
        for block in every_block_shape() {
            let expected = scan_block_scalar(&block, &is_searched);
            let actual = finder.scan_block(&block);
            assert_eq!(expected.newlines, actual.newlines, "newlines of {block:?}");
            assert_eq!(expected.candidates, actual.candidates, "candidates of {block:?}");
        }
    }

    // Each vector arm on its own, so the machine with the widest one still checks the narrower
    #[test]
    #[allow(unsafe_code)]
    #[cfg(target_arch = "x86_64")]
    fn every_vector_arm_answers_exactly_as_the_scalar_one() {
        let mut is_searched = [false; 256];
        for &byte in b"\"/'" { is_searched[byte as usize] = true; }
        let avx2 = std::is_x86_feature_detected!("avx2");
        let avx512 = std::is_x86_feature_detected!("avx512f") && std::is_x86_feature_detected!("avx512bw");
        if !avx2 { eprintln!("no AVX2 on this machine, the vector arms are untested here"); return }
        for block in every_block_shape() {
            let (masks, classes) = (scan_block_scalar(&block, &is_searched), classify_block_scalar(&block));
            // Checked two lines above, which is the whole requirement of calling a function
            // compiled for the feature
            let (vector_masks, vector_classes) = unsafe { (scan_block_avx2(&block, b"\"/'"), classify_block_avx2(&block)) };
            assert_eq!((masks.newlines, masks.candidates, classes), (vector_masks.newlines, vector_masks.candidates, vector_classes),
                    "avx2 over {block:?}");
            if avx512 {
                let (wide_masks, wide_classes) = unsafe { (scan_block_avx512(&block, b"\"/'"), classify_block_avx512(&block)) };
                assert_eq!((masks.newlines, masks.candidates, classes), (wide_masks.newlines, wide_masks.candidates, wide_classes),
                        "avx512 over {block:?}");
            }
        }
        if !avx512 { eprintln!("no AVX-512 on this machine, its arm is untested here"); }
    }

    #[test]
    fn the_classifier_answers_exactly_as_the_scalar_one() {
        let finder = MaskFinder::of(b"\"/'");
        for block in every_block_shape() {
            assert_eq!(classify_block_scalar(&block), finder.classify_block(&block), "classes of {block:?}");
        }
        // every byte value in turn, against what the line rules say of it
        for byte in 0..=255u8 {
            let block = [byte; BLOCK_BYTES];
            let classes = finder.classify_block(&block);
            let is_word = byte.is_ascii_alphanumeric() || byte >= 0x80;
            assert_eq!(if is_word { u64::MAX } else { 0 }, classes.words, "word bits of {byte:#04x}");
            assert_eq!(if byte.is_ascii_whitespace() { 0 } else { u64::MAX }, classes.content, "content bits of {byte:#04x}");
        }
        assert_eq!(Classes { words: 0b001, content: 0b101 }, finder.classify_block(b"a\n."));
    }

    #[test]
    fn a_short_block_answers_for_its_bytes_only() {
        let finder = MaskFinder::of(b"/");
        let masks = finder.scan_block(b"ab\n/");
        assert_eq!(masks.newlines, 0b0100);
        assert_eq!(masks.candidates, 0b1000);
        assert_eq!(finder.scan_block(b"").newlines, 0);
    }

    #[test]
    fn a_buffer_of_ascii_of_any_length_is_ascii() {
        for length in 0..=200usize {
            let buffer = (0..length).map(|at| (at % 128) as u8).collect::<Vec<u8>>();
            assert_eq!(buffer.is_ascii(), is_ascii(&buffer), "pure ascii of {length} bytes");
        }
    }

    #[test]
    fn a_byte_above_ascii_is_found_at_every_position_of_a_buffer() {
        for byte in 0x80..=0xFFu8 {
            for at in 0..100usize {
                let mut buffer = vec![b'x'; 100];
                buffer[at] = byte;
                assert_eq!(buffer.is_ascii(), is_ascii(&buffer), "{byte:#04x} at {at}");
            }
        }
    }

    #[test]
    fn the_ascii_test_answers_exactly_as_the_standard_library_on_random_buffers() {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; seed };
        for _ in 0..500 {
            let length = (next() % 301) as usize;
            let buffer = (0..length).map(|_| (next() >> 8) as u8).collect::<Vec<u8>>();
            assert_eq!(buffer.is_ascii(), is_ascii(&buffer), "{buffer:?}");
        }
    }

    #[test]
    fn a_language_searching_nothing_finds_no_candidate() {
        let finder = MaskFinder::of(b"");
        let block = [b'/'; BLOCK_BYTES];
        assert_eq!(finder.scan_block(&block).candidates, 0);
        assert_eq!(finder.scan_block(&block).newlines, 0);
    }

    #[test]
    fn the_lines_holding_words_and_content_are_the_ones_a_byte_walk_finds() {
        for block in every_block_shape() {
            let newlines = scan_block_scalar(&block, &[false; 256]).newlines;
            let classes = classify_block_scalar(&block);
            for start in [0, 1, 7, 33, 63] {
                for carried in [(false, false), (true, false), (false, true), (true, true)] {
                    let (mut words, mut content, mut open) = (0u64, 0u64, carried);
                    for (at, &byte) in block.iter().enumerate() {
                        if byte == b'\n' {
                            if open.0 { words |= 1 << at; }
                            if open.1 { content |= 1 << at; }
                            open = (false, false);
                        } else if at >= start {
                            open.0 |= classes.words >> at & 1 == 1;
                            open.1 |= classes.content >> at & 1 == 1;
                        }
                    }
                    assert_eq!((words, content, open),
                            find_lines_holding_words_and_content(newlines, classes, u64::MAX << start, carried),
                            "{block:?} from {start} with {carried:?} carried in");
                }
            }
        }
    }
}
