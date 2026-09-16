// One pass over a 64-byte block answers two questions for every byte at once: is it a newline, and
// is it one of the bytes the scan plan searches. The answers come back as bitmasks, bit i for byte
// i, so the line splitting and the candidate question are read off them without a search per line.

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::{__m256i, _mm256_cmpeq_epi8, _mm256_movemask_epi8, _mm256_or_si256,
        _mm256_set1_epi8, _mm256_set_epi64x};

pub(crate) const BLOCK_BYTES : usize = 64;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Masks {
    pub newlines: u64,
    pub candidates: u64,
}

// Which bytes are searched, and whether this machine can do it 32 bytes at a time
#[derive(Debug, Clone)]
pub(crate) struct MaskFinder {
    is_searched: [bool; 256],
    #[cfg(target_arch = "x86_64")]
    searched: Box<[u8]>,
    #[cfg(target_arch = "x86_64")]
    avx2: bool,
}

impl MaskFinder {
    pub(crate) fn of(searched: &[u8]) -> MaskFinder {
        let mut is_searched = [false; 256];
        for &byte in searched { is_searched[byte as usize] = true; }
        MaskFinder {
            is_searched,
            #[cfg(target_arch = "x86_64")]
            searched: searched.into(),
            #[cfg(target_arch = "x86_64")]
            avx2: std::is_x86_feature_detected!("avx2"),
        }
    }

    // A block shorter than 64 bytes, the last of a file, answers for the bytes it has and zero for
    // the rest.
    #[allow(unsafe_code)]
    pub(crate) fn scan_block(&self, block: &[u8]) -> Masks {
        #[cfg(target_arch = "x86_64")]
        if self.avx2 && block.len() == BLOCK_BYTES {
            // The feature was checked once when the finder was built, which is the whole
            // requirement of calling a function compiled for it
            return unsafe { scan_block_avx2(block, &self.searched) };
        }
        scan_block_scalar(block, &self.is_searched)
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
    bytes.is_ascii()
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
fn is_ascii_avx2(bytes: &[u8]) -> bool {
    let mut high_bits = _mm256_set1_epi8(0);
    let mut blocks = bytes.chunks_exact(32);
    for block in &mut blocks {
        high_bits = _mm256_or_si256(high_bits, load_32(block));
    }
    _mm256_movemask_epi8(high_bits) == 0 && blocks.remainder().is_ascii()
}

fn scan_block_scalar(block: &[u8], is_searched: &[bool; 256]) -> Masks {
    let (mut newlines, mut candidates) = (0u64, 0u64);
    for (at, &byte) in block.iter().enumerate() {
        if byte == b'\n' { newlines |= 1 << at; }
        if is_searched[byte as usize] { candidates |= 1 << at; }
    }
    Masks { newlines, candidates }
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
        assert_eq!(finder.scan_block(&block).candidates, 0);
        assert_eq!(finder.scan_block(&block).newlines, 0);
    }
}
