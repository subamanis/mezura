// One pass over a 64-byte block answers two questions for every byte at once: is it a newline, and
// is it one of the bytes the scan plan searches. The answers come back as bitmasks, bit i for byte
// i, so the line splitting and the candidate question are read off them without a search per line.

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::{__m256i, _mm256_and_si256, _mm256_cmpeq_epi8, _mm256_movemask_epi8, _mm256_or_si256,
        _mm256_set1_epi8, _mm256_set_epi64x, _mm256_shuffle_epi8, _mm256_srli_epi16};
#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::{uint8x16_t, vandq_u8, vceqq_u8, vcombine_u8, vcreate_u8, vdupq_n_u8, vgetq_lane_u64,
        vmaxvq_u8, vorrq_u8, vpaddq_u8, vqtbl1q_u8, vreinterpretq_u64_u8, vshrq_n_u8, vtstq_u8};

pub(crate) const BLOCK_BYTES : usize = 64;

// A byte's class is the 'and' of what its low nibble and its high nibble allow, so two table lookups
// classify sixteen or thirty-two bytes at once. Digits, the letters of the two halves of each case and
// every byte above ASCII are words; space, tab, line feed, form feed and carriage return are the
// whitespace 'trim_ascii' takes off.
const DIGIT : u8 = 0x01;
const LETTER_FROM_ONE : u8 = 0x02;
const LETTER_UP_TO_TEN : u8 = 0x04;
const ABOVE_ASCII : u8 = 0x08;
const SPACE : u8 = 0x10;
const CONTROL_SPACE : u8 = 0x20;
const WORD_CLASSES : u8 = DIGIT | LETTER_FROM_ONE | LETTER_UP_TO_TEN | ABOVE_ASCII;
const SPACE_CLASSES : u8 = SPACE | CONTROL_SPACE;
// Written twice over, since the 32-byte lookup reads each half of its lanes from its own sixteen
const LOW_NIBBLE_CLASSES : [u8; 32] = build_low_nibble_classes();
const HIGH_NIBBLE_CLASSES : [u8; 32] = build_high_nibble_classes();

const fn build_high_nibble_classes() -> [u8; 32] {
    let by_nibble = [CONTROL_SPACE, 0, SPACE, DIGIT, LETTER_FROM_ONE, LETTER_UP_TO_TEN, LETTER_FROM_ONE, LETTER_UP_TO_TEN];
    let mut table = [ABOVE_ASCII; 32];
    let mut high = 0;
    while high < 8 {
        table[high] = by_nibble[high];
        table[high + 16] = by_nibble[high];
        high += 1;
    }
    table
}

const fn build_low_nibble_classes() -> [u8; 32] {
    let mut table = [0u8; 32];
    let mut low = 0;
    while low < 32 {
        let mut classes = ABOVE_ASCII;
        let nibble = low % 16;
        if nibble <= 9 { classes |= DIGIT; }
        if nibble >= 1 { classes |= LETTER_FROM_ONE; }
        if nibble <= 10 { classes |= LETTER_UP_TO_TEN; }
        if nibble == 0 { classes |= SPACE; }
        if matches!(nibble, 9 | 10 | 12 | 13) { classes |= CONTROL_SPACE; }
        table[low] = classes;
        low += 1;
    }
    table
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Masks {
    pub newlines: u64,
    pub candidates: u64,
    pub non_spaces: u64,
    pub words: u64,
}

// Which bytes are searched, and whether this machine can do it 32 bytes at a time
#[derive(Debug, Clone)]
pub(crate) struct MaskFinder {
    is_searched: [bool; 256],
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    searched: Box<[u8]>,
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
            avx2: std::is_x86_feature_detected!("avx2"),
            #[cfg(target_arch = "aarch64")]
            neon: std::arch::is_aarch64_feature_detected!("neon"),
        }
    }

    // A block shorter than 64 bytes, the last of a file, answers for the bytes it has and zero for
    // the rest.
    #[allow(unsafe_code)]
    // The two class masks only where the caller reads them, and zero elsewhere
    pub(crate) fn scan_block<const CLASSES: bool>(&self, block: &[u8]) -> Masks {
        #[cfg(target_arch = "x86_64")]
        if self.avx2 && block.len() == BLOCK_BYTES {
            // The feature was checked once when the finder was built, which is the whole
            // requirement of calling a function compiled for it
            return unsafe { scan_block_avx2::<CLASSES>(block, &self.searched) };
        }
        #[cfg(target_arch = "aarch64")]
        if self.neon && block.len() == BLOCK_BYTES {
            return unsafe { scan_block_neon::<CLASSES>(block, &self.searched) };
        }
        scan_block_scalar::<CLASSES>(block, &self.is_searched)
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

fn scan_block_scalar<const CLASSES: bool>(block: &[u8], is_searched: &[bool; 256]) -> Masks {
    let (mut newlines, mut candidates, mut non_spaces, mut words) = (0u64, 0u64, 0u64, 0u64);
    for (at, &byte) in block.iter().enumerate() {
        if byte == b'\n' { newlines |= 1 << at; }
        if is_searched[byte as usize] { candidates |= 1 << at; }
        if CLASSES && !byte.is_ascii_whitespace() { non_spaces |= 1 << at; }
        if CLASSES && (byte.is_ascii_alphanumeric() || byte >= 0x80) { words |= 1 << at; }
    }
    Masks { newlines, candidates, non_spaces, words }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn scan_block_avx2<const CLASSES: bool>(block: &[u8], searched: &[u8]) -> Masks {
    let (low, high) = (load_32(&block[..32]), load_32(&block[32..64]));
    let newline = _mm256_set1_epi8(b'\n' as i8);
    let (low_newlines, high_newlines) = (_mm256_cmpeq_epi8(low, newline), _mm256_cmpeq_epi8(high, newline));
    let (mut low_hits, mut high_hits) = (_mm256_set1_epi8(0), _mm256_set1_epi8(0));
    for &byte in searched {
        let wanted = _mm256_set1_epi8(byte as i8);
        low_hits = _mm256_or_si256(low_hits, _mm256_cmpeq_epi8(low, wanted));
        high_hits = _mm256_or_si256(high_hits, _mm256_cmpeq_epi8(high, wanted));
    }
    if !CLASSES {
        return Masks { newlines: gather_bits(low_newlines, high_newlines), candidates: gather_bits(low_hits, high_hits),
                non_spaces: 0, words: 0 };
    }
    let (low_classes, high_classes) = (find_classes(low), find_classes(high));
    let (space_bits, word_bits, zero) = (_mm256_set1_epi8(SPACE_CLASSES as i8), _mm256_set1_epi8(WORD_CLASSES as i8),
            _mm256_set1_epi8(0));
    let lacks = |classes: __m256i, bits: __m256i| _mm256_cmpeq_epi8(_mm256_and_si256(classes, bits), zero);
    Masks {
        newlines: gather_bits(low_newlines, high_newlines),
        candidates: gather_bits(low_hits, high_hits),
        non_spaces: gather_bits(lacks(low_classes, space_bits), lacks(high_classes, space_bits)),
        words: !gather_bits(lacks(low_classes, word_bits), lacks(high_classes, word_bits)),
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn find_classes(half: __m256i) -> __m256i {
    let nibble = _mm256_set1_epi8(0x0F);
    let low = _mm256_shuffle_epi8(load_32(&LOW_NIBBLE_CLASSES), _mm256_and_si256(half, nibble));
    let high = _mm256_shuffle_epi8(load_32(&HIGH_NIBBLE_CLASSES), _mm256_and_si256(_mm256_srli_epi16::<4>(half), nibble));
    _mm256_and_si256(low, high)
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
fn scan_block_neon<const CLASSES: bool>(block: &[u8], searched: &[u8]) -> Masks {
    let quarters = [load_16(&block[..16]), load_16(&block[16..32]), load_16(&block[32..48]), load_16(&block[48..64])];
    let newline = vdupq_n_u8(b'\n');
    let newlines = gather_bits(quarters.map(|quarter| vceqq_u8(quarter, newline)));
    let mut hits = [vdupq_n_u8(0); 4];
    let (mut spaces, mut words) = ([vdupq_n_u8(0); 4], [vdupq_n_u8(0); 4]);
    for &byte in searched {
        let wanted = vdupq_n_u8(byte);
        for (hit, quarter) in hits.iter_mut().zip(quarters) {
            *hit = vorrq_u8(*hit, vceqq_u8(quarter, wanted));
        }
    }
    if !CLASSES {
        return Masks { newlines, candidates: gather_bits(hits), non_spaces: 0, words: 0 };
    }
    for (at, quarter) in quarters.into_iter().enumerate() {
        let classes = find_classes(quarter);
        spaces[at] = vtstq_u8(classes, vdupq_n_u8(SPACE_CLASSES));
        words[at] = vtstq_u8(classes, vdupq_n_u8(WORD_CLASSES));
    }
    Masks { newlines, candidates: gather_bits(hits), non_spaces: !gather_bits(spaces), words: gather_bits(words) }
}

#[cfg(target_arch = "aarch64")]
#[target_feature(enable = "neon")]
fn find_classes(quarter: uint8x16_t) -> uint8x16_t {
    let low = vqtbl1q_u8(load_16(&LOW_NIBBLE_CLASSES[..16]), vandq_u8(quarter, vdupq_n_u8(0x0F)));
    let high = vqtbl1q_u8(load_16(&HIGH_NIBBLE_CLASSES[..16]), vshrq_n_u8::<4>(quarter));
    vandq_u8(low, high)
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
        blocks.push(std::array::from_fn(|i| [b' ', b'\t', b'\r', 0x0C, b'a', b'Z', b'0', 0x80, b'@', b'[', b'`', b'{'][i % 12]));
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
            let expected = scan_block_scalar::<true>(&block, &is_searched);
            let actual = finder.scan_block::<true>(&block);
            assert_eq!(expected.newlines, actual.newlines, "newlines of {block:?}");
            assert_eq!(expected.candidates, actual.candidates, "candidates of {block:?}");
            assert_eq!(expected.non_spaces, actual.non_spaces, "non spaces of {block:?}");
            assert_eq!(expected.words, actual.words, "words of {block:?}");
        }
    }

    #[test]
    fn a_short_block_answers_for_its_bytes_only() {
        let finder = MaskFinder::of(b"/");
        let masks = finder.scan_block::<true>(b"ab\n/");
        assert_eq!(masks.newlines, 0b0100);
        assert_eq!(masks.candidates, 0b1000);
        assert_eq!(masks.non_spaces, 0b1011);
        assert_eq!(masks.words, 0b0011);
        assert_eq!(finder.scan_block::<true>(b"").newlines, 0);
    }

    #[test]
    fn a_buffer_of_ascii_of_any_length_is_ascii() {
        assert!(is_ascii(b""));
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
        assert_eq!(finder.scan_block::<true>(&block).candidates, 0);
        assert_eq!(finder.scan_block::<true>(&block).newlines, 0);
    }
}
