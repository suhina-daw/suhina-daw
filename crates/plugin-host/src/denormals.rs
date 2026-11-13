// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Plug-in audio runs with subnormal numbers flushed to zero (#1258). A plug-in's filters, delays
//! and reverb tails can decay below the smallest normal float. From there the plug-in computes
//! with subnormal numbers, which is many times slower than with normal ones, and feedback can hold
//! its state there for as long as its input is silent. Its host process then keeps a core busy,
//! and its blocks come back late. So the threads that run plug-in audio turn on flush-to-zero (a
//! result that would be subnormal becomes zero) and denormals-are-zero (a subnormal input counts
//! as zero).
//!
//! The mode belongs to a thread, so it is set where the plug-ins' blocks run. Each host audio
//! thread sets it when it starts and again before each instance's part of a block
//! ([`crate::audio`]), because a plug-in may change it inside its own processing. These threads
//! run nothing else, so the mode is never put back. Leaving it set outside an asm block departs
//! from Rust's rule for these bits; `x86::write` says why that is accepted. The host's main
//! thread keeps the default mode, and so do the threads plug-ins start from it.
//!
//! - x86_64: MXCSR's FTZ bit (15), and DAZ (6) where the processor has it. The first processors
//!   with SSE had no DAZ, and `ldmxcsr` faults on a bit the processor does not have, so the
//!   `MXCSR_MASK` that `fxsave` stores says whether DAZ may be set.
//! - aarch64: FPCR's FZ bit (24), which flushes both inputs and results.
//! - Elsewhere nothing changes.

/// The floating-point mode plug-in audio runs in, for this processor. Made once per audio
/// thread; [`Flush::apply`] puts the calling thread in it.
pub(crate) struct Flush {
    /// The MXCSR bits to set.
    #[cfg(target_arch = "x86_64")]
    bits: u32,
}

impl Flush {
    /// The mode for the processor this runs on.
    pub(crate) fn detect() -> Self {
        Self {
            #[cfg(target_arch = "x86_64")]
            bits: x86::flush_bits(x86::mask()),
        }
    }

    /// Flushes subnormal numbers to zero on the calling thread from now on. Real-time safe: it
    /// reads a register, and writes it only when the mode is not set yet.
    pub(crate) fn apply(&self) {
        #[cfg(target_arch = "x86_64")]
        {
            let csr = x86::read();
            if csr & self.bits != self.bits {
                x86::write(csr | self.bits);
            }
        }
        #[cfg(target_arch = "aarch64")]
        {
            let fpcr = arm::read();
            if fpcr & arm::FZ == 0 {
                arm::write(fpcr | arm::FZ);
            }
        }
    }
}

/// Turns flushing off on the calling thread, as a plug-in that sets a mode of its own does.
#[cfg(test)]
pub(crate) fn clear() {
    #[cfg(target_arch = "x86_64")]
    {
        x86::write(x86::read() & !(x86::FTZ | x86::DAZ));
    }
    #[cfg(target_arch = "aarch64")]
    {
        arm::write(arm::read() & !arm::FZ);
    }
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    /// Flush-to-zero: a result that would be subnormal is zero.
    pub(super) const FTZ: u32 = 1 << 15;
    /// Denormals-are-zero: a subnormal input counts as zero.
    pub(super) const DAZ: u32 = 1 << 6;
    /// The `MXCSR_MASK` a processor that stores zero there has: every bit except DAZ.
    const DEFAULT_MASK: u32 = 0xFFBF;

    /// The bits to set on a processor whose `MXCSR_MASK` is `mask`: FTZ, and DAZ where it has it.
    pub(super) fn flush_bits(mask: u32) -> u32 {
        let mask = if mask == 0 { DEFAULT_MASK } else { mask };
        FTZ | (DAZ & mask)
    }

    /// The processor's `MXCSR_MASK`: bytes 28 to 31 of what `fxsave` stores.
    pub(super) fn mask() -> u32 {
        #[repr(C, align(16))]
        struct Area([u8; 512]);
        let mut area = Area([0; 512]);
        // SAFETY: `fxsave` writes 512 bytes at a 16-byte aligned address, and `area` is both and
        // lives through the call. Every x86_64 processor has FXSR, which the target enables.
        unsafe { core::arch::x86_64::_fxsave(area.0.as_mut_ptr()) };
        match area.0.get(28..32) {
            Some(&[a, b, c, d]) => u32::from_le_bytes([a, b, c, d]),
            _ => 0,
        }
    }

    /// The calling thread's MXCSR.
    pub(super) fn read() -> u32 {
        let mut csr = 0u32;
        // SAFETY: `stmxcsr` stores the 4-byte MXCSR at the address, a live local `u32`; it
        // changes neither the stack nor the flags.
        unsafe {
            core::arch::asm!(
                "stmxcsr dword ptr [{}]",
                in(reg) &raw mut csr,
                options(nostack, preserves_flags),
            );
        }
        csr
    }

    /// Sets the calling thread's MXCSR to `csr`: a value read from it, with bits changed only by
    /// [`flush_bits`] (which sets DAZ only where `MXCSR_MASK` allows it) or by clearing them.
    pub(super) fn write(csr: u32) {
        // SAFETY: `ldmxcsr` loads the 4 bytes at the address, a live local `u32`, into MXCSR. No
        // reserved bit is set (see above), so it does not fault; it changes neither the stack nor
        // the flags. Rust's rules for inline assembly ask that FTZ and DAZ be put back before the
        // block ends, because compiled code assumes the default mode. This block leaves them set
        // on purpose: the plug-in's foreign code must run under the mode, and nothing can set it
        // for that code alone. The host's own code on these threads only copies, compares and
        // sums normal values, where flushing changes no result.
        unsafe {
            core::arch::asm!(
                "ldmxcsr dword ptr [{}]",
                in(reg) &raw const csr,
                options(nostack, preserves_flags, readonly),
            );
        }
    }
}

#[cfg(target_arch = "aarch64")]
mod arm {
    /// FPCR's flush-to-zero bit: subnormal inputs and results are zero.
    pub(super) const FZ: u64 = 1 << 24;

    /// The calling thread's FPCR.
    pub(super) fn read() -> u64 {
        let fpcr: u64;
        // SAFETY: reads the floating-point control register; no memory, stack or flags change.
        unsafe { core::arch::asm!("mrs {}, fpcr", out(reg) fpcr, options(nomem, nostack, preserves_flags)) };
        fpcr
    }

    /// Sets the calling thread's FPCR to `fpcr`, a value read from it with FZ changed.
    pub(super) fn write(fpcr: u64) {
        // SAFETY: writes the floating-point control register with a value it held, FZ aside (a
        // bit every AArch64 processor has); `isb` makes the instructions after it see the mode.
        unsafe {
            core::arch::asm!(
                "msr fpcr, {}",
                "isb",
                in(reg) fpcr,
                options(nomem, nostack, preserves_flags),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Half the smallest normal `f32`: subnormal, unless the thread flushes it to zero.
    fn halved_smallest() -> f32 {
        std::hint::black_box(f32::MIN_POSITIVE) * std::hint::black_box(0.5)
    }

    /// On its own thread, so the test runner's threads keep their mode.
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    #[test]
    fn a_thread_flushes_after_apply_and_not_before() {
        let seen = std::thread::spawn(|| {
            let before = halved_smallest();
            let flush = Flush::detect();
            flush.apply();
            let flushed = halved_smallest();
            clear();
            let cleared = halved_smallest();
            flush.apply();
            flush.apply();
            let again = halved_smallest();
            (before, flushed, cleared, again)
        })
        .join();
        let Ok((before, flushed, cleared, again)) = seen else { panic!("the thread panicked") };
        assert!(before > 0.0 && before < f32::MIN_POSITIVE, "a new thread computes subnormals: {before:e}");
        assert_eq!(flushed.to_bits(), 0, "flushed: {flushed:e}");
        assert!(cleared > 0.0, "cleared: {cleared:e}");
        assert_eq!(again.to_bits(), 0, "applied again: {again:e}");
        // The test runner's thread was never touched.
        assert!(halved_smallest() > 0.0);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn daz_is_set_only_where_the_processor_has_it() {
        use super::x86::{DAZ, FTZ, flush_bits};
        assert_eq!(flush_bits(0xFFFF), FTZ | DAZ);
        assert_eq!(flush_bits(0xFFBF), FTZ, "a mask without DAZ");
        assert_eq!(flush_bits(0), FTZ, "a zero mask is the default one, without DAZ");
        assert_eq!(flush_bits(0x0040), FTZ | DAZ);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn this_processor_reports_a_mask_with_ftz() {
        // FTZ is part of every SSE processor; the mask either says so or is zero (the default).
        let mask = super::x86::mask();
        assert!(mask == 0 || mask & super::x86::FTZ != 0, "{mask:#x}");
    }
}
