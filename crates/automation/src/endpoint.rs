// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The app's local endpoints (ADR 0028): a named pipe on Windows, a Unix domain socket
//! elsewhere, never a network port. Each build has its own, named from the folder its
//! executables sit in, so `suhina-cli` reaches the app of its own build and builds in other
//! folders never meet.
//!
//! [`Endpoint::App`] is the build's single-instance lock and carries "open these files";
//! [`Endpoint::Mcp`] serves agents while the person lets them in.

use std::io;
use std::path::{Path, PathBuf};

use tokio::io::{AsyncRead, AsyncWrite};

/// Which of a build's endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    /// The running app: the lock that keeps one instance per build, and "open these files".
    App,
    /// MCP for the app's open projects.
    Mcp,
}

/// A stream to or from an endpoint.
pub trait Stream: AsyncRead + AsyncWrite + Send + Unpin + 'static {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin + 'static> Stream for T {}

/// The key of the build in `folder`: a hash of its canonical path (lower-cased on Windows,
/// where case does not tell folders apart), as 16 hex digits.
pub fn build_key(folder: &Path) -> String {
    let real = folder.canonicalize().unwrap_or_else(|_| folder.to_path_buf());
    let text = real.to_string_lossy();
    let text = if cfg!(windows) { text.to_lowercase() } else { text.into_owned() };
    // FNV-1a: stable across Rust versions, unlike the standard library's hasher.
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    format!("{hash:016x}")
}

/// The folder of the running executable: the build's folder. A link is followed, so the macOS
/// package's `suhina-cli`, a link into `Suhina.app`, finds the app's endpoints.
pub fn build_folder() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let exe = exe.canonicalize().unwrap_or(exe);
    exe.parent().map(Path::to_path_buf).ok_or_else(|| io::Error::other("the executable has no folder"))
}

fn base_name(key: &str, endpoint: Endpoint) -> String {
    match endpoint {
        Endpoint::App => format!("suhina-{key}"),
        Endpoint::Mcp => format!("suhina-{key}-mcp"),
    }
}

/// Whether an error from [`Listener::bind`] means another app of the build holds the endpoint.
pub fn is_taken(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::PermissionDenied | io::ErrorKind::AddrInUse | io::ErrorKind::AlreadyExists)
}

/// Whether an error from [`connect`] means nothing listens there.
pub fn is_absent(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused)
}

#[cfg(windows)]
mod platform {
    use std::io;
    use std::time::Duration;

    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer, ServerOptions};

    use super::{Endpoint, Stream, base_name};

    /// `ERROR_PIPE_BUSY`: every instance of the pipe is in use; another is made in a moment.
    const PIPE_BUSY: i32 = 231;

    fn pipe_name(key: &str, endpoint: Endpoint) -> String {
        format!(r"\\.\pipe\{}", base_name(key, endpoint))
    }

    /// A pipe this process holds. Its default security lets only its creator, administrators
    /// and the system open it for writing; remote clients are refused.
    pub struct Listener {
        name: String,
        next: NamedPipeServer,
    }

    impl Listener {
        /// Fails with [`super::is_taken`] when another process holds the pipe.
        pub fn bind(key: &str, endpoint: Endpoint) -> io::Result<Self> {
            let name = pipe_name(key, endpoint);
            let next = ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(&name)?;
            Ok(Self { name, next })
        }

        /// The next client.
        pub async fn accept(&mut self) -> io::Result<Box<dyn Stream>> {
            self.next.connect().await?;
            let fresh = ServerOptions::new().reject_remote_clients(true).create(&self.name)?;
            Ok(Box::new(std::mem::replace(&mut self.next, fresh)))
        }
    }

    pub async fn connect(key: &str, endpoint: Endpoint) -> io::Result<Box<dyn Stream>> {
        let name = pipe_name(key, endpoint);
        for _ in 0..100 {
            match ClientOptions::new().open(&name) {
                Ok(client) => return Ok(Box::new(client)),
                Err(e) if e.raw_os_error() == Some(PIPE_BUSY) => tokio::time::sleep(Duration::from_millis(50)).await,
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::new(io::ErrorKind::TimedOut, "the app's pipe stayed busy"))
    }
}

#[cfg(unix)]
mod platform {
    use std::io;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::path::PathBuf;

    use tokio::net::{UnixListener, UnixStream};

    use super::{Endpoint, Stream, base_name};

    /// A folder only the user can enter: the runtime folder the session gives them, or one in
    /// their home folder.
    fn socket_folder() -> io::Result<PathBuf> {
        let base = match std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
            Some(runtime) => PathBuf::from(runtime).join("suhina"),
            None => {
                let home = std::env::var_os("HOME").ok_or_else(|| io::Error::other("HOME is not set"))?;
                PathBuf::from(home).join(".local").join("state").join("suhina").join("run")
            }
        };
        std::fs::DirBuilder::new().recursive(true).mode(0o700).create(&base)?;
        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700))?;
        Ok(base)
    }

    fn socket_path(key: &str, endpoint: Endpoint) -> io::Result<PathBuf> {
        Ok(socket_folder()?.join(format!("{}.sock", base_name(key, endpoint))))
    }

    /// A socket this process holds, 0600 in a 0700 folder; the file goes when it is dropped.
    pub struct Listener {
        path: PathBuf,
        listener: UnixListener,
    }

    impl Listener {
        /// Fails with [`super::is_taken`] when another process listens there. A socket file
        /// left by an app that ended without removing it is replaced.
        pub fn bind(key: &str, endpoint: Endpoint) -> io::Result<Self> {
            let path = socket_path(key, endpoint)?;
            if path.exists() {
                if std::os::unix::net::UnixStream::connect(&path).is_ok() {
                    return Err(io::Error::new(io::ErrorKind::AddrInUse, "another app of this build is running"));
                }
                std::fs::remove_file(&path)?;
            }
            let listener = UnixListener::bind(&path)?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
            Ok(Self { path, listener })
        }

        /// The next client.
        pub async fn accept(&mut self) -> io::Result<Box<dyn Stream>> {
            let (stream, _) = self.listener.accept().await?;
            Ok(Box::new(stream))
        }
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.path);
        }
    }

    pub async fn connect(key: &str, endpoint: Endpoint) -> io::Result<Box<dyn Stream>> {
        let path = socket_path(key, endpoint)?;
        Ok(Box::new(UnixStream::connect(&path).await?))
    }
}

pub use platform::Listener;

/// Connects to `endpoint` of the build with `key`. Fails with [`is_absent`] when nothing
/// listens there.
pub async fn connect(key: &str, endpoint: Endpoint) -> io::Result<Box<dyn Stream>> {
    platform::connect(key, endpoint).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_builds_key_is_stable_and_differs_between_folders() {
        let a = std::env::temp_dir().join("suhina-key-a");
        let b = std::env::temp_dir().join("suhina-key-b");
        assert_eq!(build_key(&a), build_key(&a));
        assert_ne!(build_key(&a), build_key(&b));
        assert_eq!(build_key(&a).len(), 16);
        assert!(build_key(&a).bytes().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn an_endpoint_is_held_once_and_reached_by_a_client() {
        let key = format!("test{:012x}", std::process::id());
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut first = Listener::bind(&key, Endpoint::App).unwrap();
            let second = Listener::bind(&key, Endpoint::App);
            assert!(second.as_ref().is_err_and(is_taken), "a second holder is refused");
            let server = tokio::spawn(async move {
                // On Unix the refused second bind looked by connecting, and that empty connection
                // comes first; the app skips such connections too.
                loop {
                    let mut stream = first.accept().await.unwrap();
                    let mut buf = [0u8; 5];
                    if stream.read_exact(&mut buf).await.is_ok() {
                        stream.write_all(&buf).await.unwrap();
                        return;
                    }
                }
            });
            let mut client = connect(&key, Endpoint::App).await.unwrap();
            client.write_all(b"hello").await.unwrap();
            let mut back = [0u8; 5];
            client.read_exact(&mut back).await.unwrap();
            assert_eq!(&back, b"hello");
            server.await.unwrap();
            assert!(connect(&key, Endpoint::Mcp).await.is_err_and(|e| is_absent(&e)), "nothing listens there");
        });
    }
}
