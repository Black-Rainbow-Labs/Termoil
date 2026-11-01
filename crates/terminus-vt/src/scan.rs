//! The SIMD scan that finds the end of a run of printable bytes.
//!
//! The parser spends most of its time in ground state looking at text. This module answers one
//! question as fast as the machine allows: where is the first byte in this slice that is not
//! printable ASCII (0x20 to 0x7E)? Everything before it can be written to the grid without going
//! through the state machine. A second function finds the first C0 control or DEL, which ends a run
//! of UTF-8 text.
//!
//! `unsafe` in this crate is confined to this module and the arena module; it is covered by Miri and
//! by the parser fuzz targets.

/// Returns the index of the first byte in `bytes` that is not printable ASCII, or `bytes.len()`.
#[inline]
pub fn first_non_printable(bytes: &[u8]) -> usize {
    #[cfg(all(target_arch = "x86_64", not(miri)))]
    {
        // SAFETY: SSE2 is part of the x86_64 baseline, so the intrinsics are always available.
        unsafe { sse2::first_non_printable(bytes) }
    }
    #[cfg(not(all(target_arch = "x86_64", not(miri))))]
    {
        first_non_printable_scalar(bytes)
    }
}

/// Returns the index of the first byte below 0x20 or equal to 0x7F, or `bytes.len()`.
#[inline]
pub fn first_control(bytes: &[u8]) -> usize {
    #[cfg(all(target_arch = "x86_64", not(miri)))]
    {
        // SAFETY: SSE2 is part of the x86_64 baseline, so the intrinsics are always available.
        unsafe { sse2::first_control(bytes) }
    }
    #[cfg(not(all(target_arch = "x86_64", not(miri))))]
    {
        first_control_scalar(bytes)
    }
}

/// Views a run of printable ASCII as a `str` without validating it again.
///
/// Callers pass only bytes that [`first_non_printable`] has just accepted, so every byte is in
/// 0x20 to 0x7E. A debug build checks that.
#[inline]
pub fn ascii_str(bytes: &[u8]) -> &str {
    debug_assert!(bytes.iter().all(|b| (0x20..0x7f).contains(b)));
    // SAFETY: every byte is ASCII (checked by the caller's scan, asserted above in debug
    // builds), and ASCII is valid UTF-8.
    unsafe { core::str::from_utf8_unchecked(bytes) }
}

/// The scalar version of [`first_non_printable`], used on targets without SSE2 and by the tests
/// that check both paths agree.
#[inline]
pub fn first_non_printable_scalar(bytes: &[u8]) -> usize {
    bytes.iter().position(|&b| !(0x20..0x7f).contains(&b)).unwrap_or(bytes.len())
}

/// The scalar version of [`first_control`].
#[inline]
pub fn first_control_scalar(bytes: &[u8]) -> usize {
    bytes.iter().position(|&b| b < 0x20 || b == 0x7f).unwrap_or(bytes.len())
}

#[cfg(all(target_arch = "x86_64", not(miri)))]
mod sse2 {
    use core::arch::x86_64::*;

    /// SSE2 scan for the first byte outside 0x20..=0x7E, sixteen bytes at a time.
    ///
    /// # Safety
    /// The caller must run on a CPU with SSE2, which every x86_64 CPU has.
    #[inline]
    pub unsafe fn first_non_printable(bytes: &[u8]) -> usize {
        let len = bytes.len();
        let mut i = 0;
        // SAFETY: the loads read 16 bytes starting at `i`, and the loop condition keeps
        // `i + 16 <= len`, so every load is inside the slice.
        unsafe {
            // Adding 0x60 maps 0x20..=0x7E onto 0x80..=0xDE; as signed bytes that is -128..=-34.
            // A byte is printable exactly when (b + 0x60) as i8 < -33.
            let bias = _mm_set1_epi8(0x60);
            let limit = _mm_set1_epi8(-33);
            while i + 16 <= len {
                let v = _mm_loadu_si128(bytes.as_ptr().add(i) as *const __m128i);
                let shifted = _mm_add_epi8(v, bias);
                let printable = _mm_cmplt_epi8(shifted, limit);
                let mask = _mm_movemask_epi8(printable) as u32;
                if mask != 0xffff {
                    return i + (!mask).trailing_zeros() as usize;
                }
                i += 16;
            }
        }
        i + super::first_non_printable_scalar(&bytes[i..])
    }

    /// SSE2 scan for the first byte below 0x20 or equal to 0x7F.
    ///
    /// # Safety
    /// The caller must run on a CPU with SSE2, which every x86_64 CPU has.
    #[inline]
    pub unsafe fn first_control(bytes: &[u8]) -> usize {
        let len = bytes.len();
        let mut i = 0;
        // SAFETY: as above, every 16 byte load stays inside the slice.
        unsafe {
            let space = _mm_set1_epi8(0x20);
            let del = _mm_set1_epi8(0x7f);
            let zero = _mm_setzero_si128();
            while i + 16 <= len {
                let v = _mm_loadu_si128(bytes.as_ptr().add(i) as *const __m128i);
                // Unsigned b < 0x20 is (max(b, 0x20) != b) ... use saturating subtract instead:
                // 0x20 - b saturates to zero exactly when b >= 0x20.
                let below = _mm_subs_epu8(space, v);
                let is_c0 = _mm_xor_si128(_mm_cmpeq_epi8(below, zero), _mm_set1_epi8(-1));
                let is_del = _mm_cmpeq_epi8(v, del);
                let mask = _mm_movemask_epi8(_mm_or_si128(is_c0, is_del)) as u32;
                if mask != 0 {
                    return i + mask.trailing_zeros() as usize;
                }
                i += 16;
            }
        }
        i + super::first_control_scalar(&bytes[i..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both scans agree with their scalar versions on every single byte value at every position.
    #[test]
    fn simd_agrees_with_scalar() {
        for len in 0..40 {
            for pos in 0..len {
                for b in 0..=255u8 {
                    let mut v = vec![b'a'; len];
                    v[pos] = b;
                    assert_eq!(first_non_printable(&v), first_non_printable_scalar(&v), "len {len} pos {pos} byte {b}");
                    assert_eq!(first_control(&v), first_control_scalar(&v), "len {len} pos {pos} byte {b}");
                }
            }
        }
    }
}
