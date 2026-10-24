// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared memory between the engine and a host process, freed by the operating system when the
//! last process that has it lets go, however it ends (#805):
//!
//! - Windows: a mapping backed by the page file, named `Local\suhina-<kind>-…` in the session's
//!   namespace. The address is its name; the host opens it by name. Nothing is written to a
//!   folder.
//! - Linux: a `memfd` (memory with no name in any folder). The address is
//!   `/proc/<engine pid>/fd/<n>#<token>`: the host opens the path while the engine holds it,
//!   and the token, which is in the memfd's name, tells that memfd from another that gets the
//!   same descriptor number later.
//! - macOS and other Unix systems: a file in the temporary folder, named after its process and
//!   that process's start time. The engine removes its name when the host has opened it
//!   ([`Shared::unlink`]) or when it lets go of it; a file that a crash left before then is
//!   removed by the next Suhina process ([`crate::sweep`]).
//!
//! `kind` is `ipc` for a link's region and `ara` for an audio source, so neither opens as the
//! other.

#[cfg(not(any(windows, target_os = "linux")))]
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::IpcError;

/// Whether the opening side may write the memory.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    ReadWrite,
    Read,
}

/// Memory shared with another process: a mapping, and what keeps it reachable by its address.
pub(crate) struct Shared {
    #[cfg(not(windows))]
    map: memmap2::MmapRaw,
    #[cfg(windows)]
    view: windows::View,
    /// What the other process opens it by.
    address: String,
    /// A name unique among the live ones, for objects that go with it (the doorbells).
    name: String,
    /// The memfd, kept open by the side that made it so its `/proc` path stays.
    #[cfg(target_os = "linux")]
    _file: Option<std::fs::File>,
    /// The file to remove, while it has a name and this side made it.
    #[cfg(not(any(windows, target_os = "linux")))]
    path: Option<PathBuf>,
}

impl Shared {
    /// Makes `len` bytes of zeroed shared memory of `kind`.
    pub(crate) fn create(kind: &str, len: usize) -> Result<Self, IpcError> {
        if len == 0 {
            return Err(IpcError::BadLink("empty shared memory"));
        }
        crate::sweep::before_making_files();
        platform::create(kind, len)
    }

    /// Opens the shared memory of `kind` at `address`, which [`Shared::address`] gave.
    pub(crate) fn open(kind: &str, address: &str, access: Access) -> Result<Self, IpcError> {
        platform::open(kind, address, access)
    }

    pub(crate) fn address(&self) -> &str {
        &self.address
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// The first byte. Page-aligned.
    pub(crate) fn as_ptr(&self) -> *mut u8 {
        #[cfg(not(windows))]
        return self.map.as_mut_ptr();
        #[cfg(windows)]
        return self.view.ptr;
    }

    /// Bytes mapped: at least what was asked for when it was made.
    pub(crate) fn len(&self) -> usize {
        #[cfg(not(windows))]
        return self.map.len();
        #[cfg(windows)]
        return self.view.len;
    }

    /// The other process has opened it, so it no longer needs a name: on macOS (where the memory
    /// is a file) the file's name goes now, and the memory stays until both sides let go. A
    /// crash after this leaves nothing. Elsewhere nothing has a name to remove.
    pub(crate) fn unlink(&mut self) {
        #[cfg(not(any(windows, target_os = "linux")))]
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        // The mapping (and, on Linux, the memfd) closes with the fields.
        self.unlink();
    }
}

/// Shared memory of `kind` holding `bytes`, for tests of what opening it refuses.
#[cfg(test)]
pub(crate) fn with_bytes(kind: &str, bytes: &[u8]) -> Shared {
    let shared = Shared::create(kind, bytes.len()).unwrap_or_else(|e| panic!("{e}"));
    // SAFETY: the mapping holds at least `bytes.len()` bytes, and nothing else uses it yet.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), shared.as_ptr(), bytes.len()) };
    shared
}

/// `suhina-<kind>-` followed by digits, `-` and (on macOS) `@`: the names this crate makes.
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn is_name(kind: &str, name: &str) -> bool {
    name.strip_prefix("suhina-")
        .and_then(|rest| rest.strip_prefix(kind))
        .and_then(|rest| rest.strip_prefix('-'))
        .is_some_and(|rest| {
            !rest.is_empty() && rest.len() <= 64 && rest.bytes().all(|b| b.is_ascii_digit() || b == b'-' || b == b'@')
        })
}

/// A number no other object of this process uses.
fn next() -> u32 {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[cfg(windows)]
use windows as platform;

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::System::Memory::{
        CreateFileMappingW, FILE_MAP_READ, FILE_MAP_WRITE, MEMORY_BASIC_INFORMATION, MEMORY_MAPPED_VIEW_ADDRESS,
        MapViewOfFile, OpenFileMappingW, PAGE_READWRITE, UnmapViewOfFile, VirtualQuery,
    };

    use super::{Access, Shared, is_name, next};
    use crate::IpcError;

    /// A mapping's handle and this process's view of it.
    pub(super) struct View {
        handle: HANDLE,
        pub(super) ptr: *mut u8,
        pub(super) len: usize,
    }

    // SAFETY: a file-mapping handle and a view of it may be used and closed from any thread;
    // what is done with the bytes is up to `Region` and `AudioSource`, which use atomics or hand
    // the buffers to one side at a time, as with the memory mappings on other systems.
    unsafe impl Send for View {}
    // SAFETY: as above; `View` itself only hands out the pointer.
    unsafe impl Sync for View {}

    impl Drop for View {
        fn drop(&mut self) {
            // SAFETY: `ptr` is the view MapViewOfFile returned, unmapped only here.
            unsafe { UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS { Value: self.ptr.cast() }) };
            // SAFETY: the handle came from CreateFileMappingW or OpenFileMappingW and is closed
            // only here.
            unsafe { CloseHandle(self.handle) };
        }
    }

    /// Closes a handle unless it is kept.
    struct Handle(HANDLE);

    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: a handle this module opened, closed once.
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    impl Handle {
        fn keep(mut self) -> HANDLE {
            std::mem::replace(&mut self.0, std::ptr::null_mut())
        }
    }

    fn wide(name: &str) -> Vec<u16> {
        name.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub(super) fn create(kind: &str, len: usize) -> Result<Shared, IpcError> {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        let name = format!("suhina-{kind}-{}-{}-{nanos}", std::process::id(), next());
        let address = format!("Local\\{name}");
        let size = len as u64;
        let wide_address = wide(&address);
        // SAFETY: no file (INVALID_HANDLE_VALUE asks for the page file), no security attributes,
        // and a NUL-terminated name that outlives the call.
        let handle = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                std::ptr::null(),
                PAGE_READWRITE,
                (size >> 32) as u32,
                size as u32,
                wide_address.as_ptr(),
            )
        };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let handle = Handle(handle);
        // SAFETY: reads this thread's last error, set by the call above.
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            return Err(IpcError::BadLink("a shared memory of that name exists already"));
        }
        let view = map(handle, FILE_MAP_READ | FILE_MAP_WRITE)?;
        // Page-file memory starts zeroed.
        Ok(Shared { view, address, name })
    }

    pub(super) fn open(kind: &str, address: &str, access: Access) -> Result<Shared, IpcError> {
        let name = address
            .strip_prefix("Local\\")
            .filter(|name| is_name(kind, name))
            .ok_or(IpcError::BadLink("not a Suhina shared memory name"))?;
        let rights = match access {
            Access::ReadWrite => FILE_MAP_READ | FILE_MAP_WRITE,
            Access::Read => FILE_MAP_READ,
        };
        let wide_address = wide(address);
        // SAFETY: a NUL-terminated name that outlives the call.
        let handle = unsafe { OpenFileMappingW(rights, 0, wide_address.as_ptr()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let view = map(Handle(handle), rights)?;
        Ok(Shared { view, address: address.to_string(), name: name.to_string() })
    }

    /// Maps all of `handle`'s mapping with `rights`.
    fn map(handle: Handle, rights: u32) -> Result<View, IpcError> {
        // SAFETY: a valid file-mapping handle; offset 0 and length 0 map all of it.
        let view = unsafe { MapViewOfFile(handle.0, rights, 0, 0, 0) };
        if view.Value.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let mut info = MEMORY_BASIC_INFORMATION::default();
        // SAFETY: `view` is a mapped address; `info` is a live, writable struct of the size
        // passed.
        let written = unsafe { VirtualQuery(view.Value, &raw mut info, size_of::<MEMORY_BASIC_INFORMATION>()) };
        let view = View { handle: handle.keep(), ptr: view.Value.cast(), len: info.RegionSize };
        if written == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(view)
    }
}

#[cfg(target_os = "linux")]
use linux as platform;

#[cfg(target_os = "linux")]
mod linux {
    use std::fs::{File, OpenOptions};
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

    use super::{Access, Shared, next};
    use crate::IpcError;

    pub(super) fn create(kind: &str, len: usize) -> Result<Shared, IpcError> {
        // The token makes this memfd tell itself from any other that later gets the same
        // descriptor number: the address carries it and `open` checks it on the file.
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos());
        let token = format!("{:x}x{nanos:x}", next());
        let label = std::ffi::CString::new(format!("suhina-{kind}-{token}"))
            .map_err(|_| IpcError::BadLink("not a shared memory kind"))?;
        // SAFETY: a NUL-terminated name that outlives the call. The descriptor is closed on
        // exec, so the host process does not inherit it: it opens it by its `/proc` path.
        let fd = unsafe { libc::memfd_create(label.as_ptr(), libc::MFD_CLOEXEC) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: `fd` is a new descriptor that nothing else owns.
        let file = File::from(unsafe { OwnedFd::from_raw_fd(fd) });
        file.set_len(len as u64)?;
        let map = memmap2::MmapOptions::new().map_raw(&file)?;
        let (pid, n) = (std::process::id(), file.as_raw_fd());
        let address = format!("/proc/{pid}/fd/{n}#{token}");
        let name = format!("suhina-{kind}-{pid}-{n}-{token}");
        Ok(Shared { map, address, name, _file: Some(file) })
    }

    pub(super) fn open(kind: &str, address: &str, access: Access) -> Result<Shared, IpcError> {
        let parts = address
            .strip_prefix("/proc/")
            .and_then(|rest| rest.split_once("/fd/"))
            .and_then(|(pid, rest)| rest.split_once('#').map(|(n, token)| (pid, n, token)));
        let digits = |s: &str| !s.is_empty() && s.len() <= 10 && s.bytes().all(|b| b.is_ascii_digit());
        let token_ok =
            |s: &str| !s.is_empty() && s.len() <= 32 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'x');
        let Some((pid, n, token)) = parts.filter(|(pid, n, token)| digits(pid) && digits(n) && token_ok(token)) else {
            return Err(IpcError::BadLink("not a Suhina shared memory address"));
        };
        let path = format!("/proc/{pid}/fd/{n}");
        // What the descriptor is: the memfd made for this kind and this token reads
        // `/memfd:suhina-<kind>-<token> (deleted)`. A descriptor number that was closed and
        // reused by another region fails here instead of mapping that region.
        let target = std::fs::read_link(&path)?;
        let expected = format!("/memfd:suhina-{kind}-{token} (deleted)");
        if target.to_str() != Some(expected.as_str()) {
            return Err(IpcError::BadLink("not that Suhina shared memory"));
        }
        let file = OpenOptions::new().read(true).write(access == Access::ReadWrite).open(&path)?;
        // The descriptor could have been replaced between the check and the open.
        if std::fs::read_link(&path)?.to_str() != Some(expected.as_str()) {
            return Err(IpcError::BadLink("not that Suhina shared memory"));
        }
        if file.metadata()?.len() == 0 {
            return Err(IpcError::BadLink("empty shared memory"));
        }
        let map = match access {
            Access::ReadWrite => memmap2::MmapOptions::new().map_raw(&file)?,
            Access::Read => memmap2::MmapOptions::new().map_raw_read_only(&file)?,
        };
        let name = format!("suhina-{kind}-{pid}-{n}-{token}");
        Ok(Shared { map, address: address.to_string(), name, _file: None })
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
use files as platform;

/// macOS and other Unix systems: a file in the temporary folder.
#[cfg(not(any(windows, target_os = "linux")))]
mod files {
    use std::fs::OpenOptions;
    use std::path::Path;

    use super::{Access, Shared, is_name, next};
    use crate::IpcError;

    pub(super) fn create(kind: &str, len: usize) -> Result<Shared, IpcError> {
        let pid = std::process::id();
        let started = crate::sweep::started(pid);
        let name = format!("suhina-{kind}-{pid}@{started}-{}", next());
        let path = std::env::temp_dir().join(&name);
        let file = OpenOptions::new().read(true).write(true).create_new(true).open(&path)?;
        let mapped = file.set_len(len as u64).and_then(|()| memmap2::MmapOptions::new().map_raw(&file));
        match mapped {
            Ok(map) => Ok(Shared { map, address: path.display().to_string(), name, path: Some(path) }),
            Err(e) => {
                let _ = std::fs::remove_file(&path);
                Err(e.into())
            }
        }
    }

    pub(super) fn open(kind: &str, address: &str, access: Access) -> Result<Shared, IpcError> {
        let path = Path::new(address);
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| is_name(kind, n))
            .ok_or(IpcError::BadLink("not a Suhina shared memory name"))?
            .to_string();
        let file = OpenOptions::new().read(true).write(access == Access::ReadWrite).open(path)?;
        if file.metadata()?.len() == 0 {
            return Err(IpcError::BadLink("empty shared memory"));
        }
        let map = match access {
            Access::ReadWrite => memmap2::MmapOptions::new().map_raw(&file)?,
            Access::Read => memmap2::MmapOptions::new().map_raw_read_only(&file)?,
        };
        Ok(Shared { map, address: address.to_string(), name, path: None })
    }
}
