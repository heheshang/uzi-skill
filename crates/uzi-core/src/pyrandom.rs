//! CPython's `random` module — MT19937 plus the `random` / `randint` / `choice`
//! helpers built on it.
//!
//! Ported so that seeded generators reproduce Python's stream *exactly*:
//! upstream `preview_with_mock.py` calls `random.seed(42)` and then draws from
//! it, so any deviation would change the generated mock report. The algorithm is
//! the reference MT19937 (`init_by_array` seeding) with CPython's
//! `genrand_res53` for `random()` and `_randbelow` for the bounded draws.
//!
//! Only the surface upstream needs is implemented: integer seeding, `random()`,
//! `randint`, `randrange`, and `choice`.

/// `MT19937` state size and period parameters.
const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908_b0df;
const UPPER_MASK: u32 = 0x8000_0000;
const LOWER_MASK: u32 = 0x7fff_ffff;

/// A seeded generator matching CPython's `random.Random`.
#[derive(Debug, Clone)]
pub struct PyRandom {
    mt: [u32; N],
    mti: usize,
}

impl PyRandom {
    /// `random.seed(n)` for a non-negative integer seed.
    ///
    /// CPython converts the integer to little-endian 32-bit words and feeds that
    /// array to `init_by_array`; a single word for any value below 2^32.
    pub fn seed_u64(n: u64) -> Self {
        let mut key: Vec<u32> = Vec::new();
        let mut v = n;
        loop {
            key.push((v & 0xffff_ffff) as u32);
            v >>= 32;
            if v == 0 {
                break;
            }
        }
        Self::seed_by_words(&key)
    }

    /// `random.seed(n)` for a signed integer, as Python accepts negative seeds.
    pub fn seed_i64(n: i64) -> Self {
        if n >= 0 {
            return Self::seed_u64(n as u64);
        }
        // CPython converts a negative int with `_PyLong_AsByteArray` in
        // two's-complement little-endian, so the word array is the two's
        // complement of the magnitude.
        let mag = (n as i128).unsigned_abs();
        let mut key: Vec<u32> = Vec::new();
        let mut v = mag;
        loop {
            key.push((v & 0xffff_ffff) as u32);
            v >>= 32;
            if v == 0 {
                break;
            }
        }
        // Two's complement across the same word count.
        let mut carry = true;
        for w in key.iter_mut() {
            let inverted = !*w;
            if carry {
                let (v, c) = inverted.overflowing_add(1);
                *w = v;
                carry = c;
            } else {
                *w = inverted;
            }
        }
        Self::seed_by_words(&key)
    }

    fn seed_by_words(key: &[u32]) -> Self {
        let mut r = PyRandom {
            mt: [0u32; N],
            mti: N + 1,
        };
        r.init_genrand(1965_0218);
        r.init_by_array(key);
        r
    }

    /// `init_genrand(s)`.
    fn init_genrand(&mut self, s: u32) {
        self.mt[0] = s;
        for i in 1..N {
            let prev = self.mt[i - 1];
            self.mt[i] = 1_812_433_253u32
                .wrapping_mul(prev ^ (prev >> 30))
                .wrapping_add(i as u32);
        }
        self.mti = N;
    }

    /// `init_by_array(key)`.
    fn init_by_array(&mut self, key: &[u32]) {
        if key.is_empty() {
            return;
        }
        let key_length = key.len();
        let mut i = 1usize;
        let mut j = 0usize;

        let mut k = if N > key_length { N } else { key_length };
        while k > 0 {
            let prev = self.mt[i - 1];
            self.mt[i] = (self.mt[i]
                ^ (prev ^ (prev >> 30)).wrapping_mul(1_664_525))
            .wrapping_add(key[j])
            .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                self.mt[0] = self.mt[N - 1];
                i = 1;
            }
            if j >= key_length {
                j = 0;
            }
            k -= 1;
        }

        let mut k = N - 1;
        while k > 0 {
            let prev = self.mt[i - 1];
            self.mt[i] = (self.mt[i]
                ^ (prev ^ (prev >> 30)).wrapping_mul(1_566_083_941))
            .wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                self.mt[0] = self.mt[N - 1];
                i = 1;
            }
            k -= 1;
        }

        self.mt[0] = 0x8000_0000;
        self.mti = N;
    }

    /// `genrand_uint32()`.
    fn genrand_uint32(&mut self) -> u32 {
        if self.mti >= N {
            for kk in 0..(N - M) {
                let y = (self.mt[kk] & UPPER_MASK) | (self.mt[kk + 1] & LOWER_MASK);
                self.mt[kk] = self.mt[kk + M] ^ (y >> 1) ^ if y & 1 == 1 { MATRIX_A } else { 0 };
            }
            for kk in (N - M)..(N - 1) {
                let y = (self.mt[kk] & UPPER_MASK) | (self.mt[kk + 1] & LOWER_MASK);
                self.mt[kk] = self.mt[kk + M - N] ^ (y >> 1) ^ if y & 1 == 1 { MATRIX_A } else { 0 };
            }
            let y = (self.mt[N - 1] & UPPER_MASK) | (self.mt[0] & LOWER_MASK);
            self.mt[N - 1] = self.mt[M - 1] ^ (y >> 1) ^ if y & 1 == 1 { MATRIX_A } else { 0 };
            self.mti = 0;
        }

        let mut y = self.mt[self.mti];
        self.mti += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }

    /// `random.getrandbits(k)` for `k <= 64`.
    ///
    /// CPython fills little-endian 32-bit words, right-shifting the final word
    /// when the request does not fill it.
    pub fn getrandbits(&mut self, k: u32) -> u64 {
        debug_assert!(k <= 64);
        if k == 0 {
            return 0;
        }
        let words = (k - 1) / 32 + 1;
        let mut out: u64 = 0;
        let mut remaining = k;
        for i in 0..words {
            let mut r = self.genrand_uint32() as u64;
            if remaining < 32 {
                r >>= 32 - remaining;
            }
            out |= r << (32 * i);
            remaining = remaining.saturating_sub(32);
        }
        out
    }

    /// `random.random()` — CPython's `genrand_res53`.
    pub fn random(&mut self) -> f64 {
        let a = (self.genrand_uint32() >> 5) as f64;
        let b = (self.genrand_uint32() >> 6) as f64;
        (a * 67_108_864.0 + b) * (1.0 / 9_007_199_254_740_992.0)
    }

    /// `random._randbelow_with_getrandbits(n)`.
    pub fn randbelow(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        let k = 64 - n.leading_zeros();
        let mut r = self.getrandbits(k);
        while r >= n {
            r = self.getrandbits(k);
        }
        r
    }

    /// `random.randrange(start, stop)`.
    pub fn randrange(&mut self, start: i64, stop: i64) -> i64 {
        let width = (stop - start) as u64;
        start + self.randbelow(width) as i64
    }

    /// `random.randint(a, b)` — inclusive on both ends.
    pub fn randint(&mut self, a: i64, b: i64) -> i64 {
        self.randrange(a, b + 1)
    }

    /// `random.choice(seq)` — panics on an empty sequence, like Python's
    /// `IndexError`.
    pub fn choice<'a, T>(&mut self, seq: &'a [T]) -> &'a T {
        assert!(!seq.is_empty(), "cannot choose from an empty sequence");
        &seq[self.randbelow(seq.len() as u64) as usize]
    }

    /// `random.choice(seq)` over a slice of string literals.
    pub fn choice_str<'a>(&mut self, seq: &[&'a str]) -> &'a str {
        assert!(!seq.is_empty(), "cannot choose from an empty sequence");
        seq[self.randbelow(seq.len() as u64) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `random.seed(42); [random.random() for _ in range(3)]`
    #[test]
    fn seed_42_random_stream_matches_cpython() {
        let mut r = PyRandom::seed_u64(42);
        let got: Vec<f64> = (0..3).map(|_| r.random()).collect();
        assert_eq!(
            got,
            vec![
                0.6394267984578837,
                0.025010755222666936,
                0.27502931836911926
            ]
        );
    }

    /// `random.seed(42); [random.randint(55, 95) for _ in range(5)]`
    #[test]
    fn seed_42_randint_matches_cpython() {
        let mut r = PyRandom::seed_u64(42);
        let got: Vec<i64> = (0..5).map(|_| r.randint(55, 95)).collect();
        assert_eq!(got, vec![95, 62, 56, 72, 70]);
    }

    /// `random.seed(42); [random.choice('abcd') for _ in range(5)]`
    #[test]
    fn seed_42_choice_matches_cpython() {
        let mut r = PyRandom::seed_u64(42);
        let seq = ["a", "b", "c", "d"];
        let got: Vec<&str> = (0..5).map(|_| r.choice_str(&seq)).collect();
        assert_eq!(got, vec!["a", "a", "c", "b", "b"]);
    }

    /// `random.seed(42); [random.randrange(5, 96) for _ in range(5)]`
    #[test]
    fn seed_42_randrange_matches_cpython() {
        let mut r = PyRandom::seed_u64(42);
        let got: Vec<i64> = (0..5).map(|_| r.randrange(5, 96)).collect();
        assert_eq!(got, vec![86, 19, 8, 40, 36]);
    }

    /// `random.randint` is inclusive at both ends — the bound is reachable.
    #[test]
    fn randint_bounds_are_inclusive() {
        // randint(0, 0) is degenerate but must not loop forever.
        let mut r = PyRandom::seed_u64(1);
        for _ in 0..50 {
            assert_eq!(r.randint(0, 0), 0);
        }
        let mut r = PyRandom::seed_u64(7);
        let mut seen_low = false;
        let mut seen_high = false;
        for _ in 0..500 {
            let v = r.randint(1, 3);
            assert!((1..=3).contains(&v), "out of range: {v}");
            seen_low |= v == 1;
            seen_high |= v == 3;
        }
        assert!(seen_low && seen_high, "both endpoints should occur");
    }

    /// Distinct seeds must produce distinct streams, and the same seed must
    /// reproduce identically — the property the mock generator relies on.
    #[test]
    fn streams_are_deterministic_and_seed_dependent() {
        let draw = |seed: u64| {
            let mut r = PyRandom::seed_u64(seed);
            (0..8).map(|_| r.random()).collect::<Vec<f64>>()
        };
        assert_eq!(draw(42), draw(42));
        assert_ne!(draw(42), draw(43));
        assert_ne!(draw(1), draw(0));
    }

    /// Two instances seeded alike must agree word-for-word, including across the
    /// 624-word state regen boundary.
    #[test]
    fn state_regeneration_is_consistent_beyond_one_block() {
        let mut a = PyRandom::seed_u64(42);
        let mut b = PyRandom::seed_u64(42);
        for i in 0..2000 {
            assert_eq!(a.genrand_uint32(), b.genrand_uint32(), "diverged at word {i}");
        }
    }

    /// `random()` is a 53-bit draw in [0, 1).
    #[test]
    fn random_is_within_unit_interval() {
        let mut r = PyRandom::seed_u64(2026);
        for _ in 0..2000 {
            let v = r.random();
            assert!((0.0..1.0).contains(&v), "out of [0,1): {v}");
        }
    }

    #[test]
    fn getrandbits_returns_at_most_k_bits() {
        let mut r = PyRandom::seed_u64(99);
        for k in 0..=64u32 {
            let v = r.getrandbits(k);
            if k == 0 {
                assert_eq!(v, 0);
            } else if k < 64 {
                assert!(v < (1u64 << k), "k={k} produced {v}");
            }
        }
    }

    #[test]
    #[should_panic(expected = "empty sequence")]
    fn choice_rejects_empty_sequences() {
        let mut r = PyRandom::seed_u64(1);
        let empty: [&str; 0] = [];
        let _ = r.choice_str(&empty);
    }
}
