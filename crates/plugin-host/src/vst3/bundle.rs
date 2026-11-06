// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The plug-in bundle macOS hosts hand a VST3 module: a CoreFoundation `CFBundle`, its
//! handle passed to `bundleEntry` ([`super`]'s module contract) and released when the
//! module is. The types are Apple's public C API (`CFBundle`, `CFURL`, `CFBase`),
//! declared here by hand: three calls, no crate of its own. Null is what a host without
//! a bundle passes, and it is what this host passed before, but a plug-in may keep the
//! handle and read it later without a null check: Xfer's Serum 2 2.1.5 crashed its
//! host's main thread inside the editor's view creation when it was handed null (#1223).

use std::ffi::c_void;
use std::path::Path;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    /// `CFAllocatorRef CFAllocatorGetDefault(void)`: the allocator to pass the classes
    /// below, which is created lazily and kept by CoreFoundation itself.
    fn CFAllocatorGetDefault() -> *const c_void;
    /// `CFURLRef CFURLCreateFromFileSystemRepresentation(CFAllocatorRef allocator, const
    /// UInt8 *buffer, CFIndex bufLen, Boolean isDirectory)`, a reference the caller owns.
    fn CFURLCreateFromFileSystemRepresentation(
        allocator: *const c_void,
        buffer: *const u8,
        length: isize,
        directory: u8,
    ) -> *mut c_void;
    /// `CFBundleRef CFBundleCreate(CFAllocatorRef allocator, CFURLRef bundleURL)`, a
    /// reference the caller owns.
    fn CFBundleCreate(allocator: *const c_void, url: *const c_void) -> *mut c_void;
    /// `void CFRelease(CFTypeRef cf)`.
    fn CFRelease(cf: *const c_void);
}

/// A CoreFoundation `CFBundle` for a plug-in folder, alive as long as it is held: the
/// reference count CoreFoundation's create calls start with is the caller's, so dropping
/// it releases (`CFRelease`) once and lets go. The handle is `*mut c_void`, the same
/// shape `bundleEntry` takes.
pub(crate) struct Bundle(*mut c_void);

impl Bundle {
    /// The bundle for a plug-in folder `path`, when it is one: `None` for a bare file
    /// (the bundle-less `.vst3` layout) and anything else CoreFoundation will not call a
    /// bundle, where the host, as before this existed, passes null.
    pub(crate) fn open(path: &Path) -> Option<Self> {
        let Some(text) = super::bundle_text(path) else {
            // A bare file, a missing folder, or a path CoreFoundation's UTF-8 model
            // cannot hold: null, as before.
            return None;
        };
        let bytes = text.as_bytes();
        // SAFETY: the call reads no memory and answers with the allocator CoreFoundation
        // keeps as its default.
        let allocator = unsafe { CFAllocatorGetDefault() };
        // SAFETY: `bytes` live through the call with the length theirs; the returned
        // reference is owned (`CFRelease` after the bundle is made below).
        let url =
            unsafe { CFURLCreateFromFileSystemRepresentation(allocator, bytes.as_ptr(), bytes.len() as isize, 1) };
        if url.is_null() {
            return None;
        }
        // SAFETY: `url` lives through the call, is unrelated to what is handed back, and
        // is released after; the returned `CFBundle` is owned.
        let bundle = unsafe { CFBundleCreate(allocator, url) };
        // SAFETY: an owned `CFURL`, released once.
        unsafe { CFRelease(url) };
        if bundle.is_null() { None } else { Some(Self(bundle)) }
    }

    /// The handle, the shape `bundleEntry` takes.
    pub(crate) fn handle(&self) -> *mut c_void {
        self.0
    }
}

impl Drop for Bundle {
    fn drop(&mut self) {
        // SAFETY: an owned `CFBundle`, created by [`Bundle::open`], released once.
        unsafe { CFRelease(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A CoreFoundation bundle is a folder, whatever it carries: the plain fixture
    /// folder a scratch test makes holds one, made twice through the test (an owned
    /// reference releases in `drop`; the second open is a second, independent handle).
    #[test]
    fn a_plugin_folder_makes_a_bundle() {
        let folder = std::env::temp_dir().join(format!("suhina-bundle-test-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap_or_else(|e| panic!("create the folder: {e}"));
        let bundle = Bundle::open(&folder);
        let Some(bundle) = bundle else { panic!("a folder is a bundle") };
        assert!(!bundle.handle().is_null(), "the handle is CoreFoundation's");
        drop(bundle);
        // Still fine afterwards: the second open is a second, independent handle.
        assert!(Bundle::open(&folder).is_some(), "opening again what was released");
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// A bare file is no bundle: the host passes null and the entry's other arm stays.
    #[test]
    fn a_bare_file_is_no_bundle() {
        let file = std::env::temp_dir().join(format!("suhina-bare-vst3-{}.vst3", std::process::id()));
        std::fs::write(&file, b"not a bundle").unwrap_or_else(|e| panic!("write {e}"));
        assert!(Bundle::open(&file).is_none(), "a file does not make a bundle");
        let _ = std::fs::remove_file(&file);
    }
}
