//! A Typesense API key that, when it comes from a file, follows that file.
//!
//! Rotating a key (for example a Kubernetes Secret update) rewrites the file;
//! polling its metadata on use avoids a restart without a file-watcher.

use std::{
    fmt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

/// How often a key file's metadata is looked at, at most.
const CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// Modified time and length: what tells a rewritten file from an unchanged one.
type Signature = Option<(SystemTime, u64)>;

#[derive(Clone)]
pub struct KeySource {
    inner: Arc<Inner>,
}

enum Inner {
    /// From a flag or environment variable: never changes.
    Fixed(String),
    File(FileKey),
}

struct FileKey {
    path: PathBuf,
    interval: Duration,
    state: Mutex<FileState>,
}

struct FileState {
    key: String,
    /// Signature of the file as last looked at, whether or not it was usable.
    signature: Signature,
    last_check: Instant,
    /// The last failure warned about, so a persistent one is logged once.
    warned: Option<String>,
}

impl KeySource {
    pub fn fixed(key: String) -> Self {
        Self {
            inner: Arc::new(Inner::Fixed(key)),
        }
    }

    /// Read the key file now; fails if it is unreadable or empty.
    pub fn file(path: &Path) -> std::io::Result<Self> {
        Self::file_with_interval(path, CHECK_INTERVAL)
    }

    fn file_with_interval(path: &Path, interval: Duration) -> std::io::Result<Self> {
        let signature = signature(path);
        let key = crate::cli::read_key_file(path)?;
        Ok(Self {
            inner: Arc::new(Inner::File(FileKey {
                path: path.to_path_buf(),
                interval,
                state: Mutex::new(FileState {
                    key,
                    signature,
                    last_check: Instant::now(),
                    warned: None,
                }),
            })),
        })
    }

    /// The current key. For a file, this first re-reads it if its metadata
    /// changed since the last look (checked at most once per second).
    pub fn current(&self) -> String {
        match &*self.inner {
            Inner::Fixed(key) => key.clone(),
            Inner::File(file) => file.current(),
        }
    }
}

impl FileKey {
    fn current(&self) -> String {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.last_check.elapsed() < self.interval {
            return state.key.clone();
        }
        state.last_check = Instant::now();
        let now = signature(&self.path);
        if now == state.signature {
            return state.key.clone();
        }
        state.signature = now;
        match crate::cli::read_key_file(&self.path) {
            Ok(key) => {
                state.warned = None;
                if key != state.key {
                    state.key = key;
                    tracing::info!(
                        "reloaded the Typesense API key from {}",
                        self.path.display()
                    );
                }
            }
            Err(err) => {
                let message = err.to_string();
                if state.warned.as_ref() != Some(&message) {
                    tracing::warn!(
                        "cannot reload the Typesense API key from {}, keeping the previous key: {message}",
                        self.path.display()
                    );
                    state.warned = Some(message);
                }
            }
        }
        state.key.clone()
    }
}

/// Follows symlinks, as Kubernetes Secret volumes swap a `..data` link.
fn signature(path: &Path) -> Signature {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// Never prints the key.
impl fmt::Debug for KeySource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &*self.inner {
            Inner::Fixed(_) => f.write_str("KeySource::Fixed(<redacted>)"),
            Inner::File(file) => write!(f, "KeySource::File({})", file.path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ts-key-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn warned(source: &KeySource) -> Option<String> {
        match &*source.inner {
            Inner::File(f) => f.state.lock().unwrap().warned.clone(),
            Inner::Fixed(_) => None,
        }
    }

    #[test]
    fn fixed_key_never_changes() {
        let s = KeySource::fixed("k".into());
        assert_eq!(s.current(), "k");
        assert_eq!(format!("{s:?}"), "KeySource::Fixed(<redacted>)");
    }

    #[test]
    fn changed_contents_give_the_new_key() {
        let d = dir("changed");
        let f = d.join("key");
        std::fs::write(&f, "old\n").unwrap();
        let s = KeySource::file_with_interval(&f, Duration::ZERO).unwrap();
        assert_eq!(s.current(), "old");
        std::fs::write(&f, " newer-key \n").unwrap();
        assert_eq!(s.current(), "newer-key");
        assert_eq!(warned(&s), None);
    }

    #[test]
    fn empty_or_missing_file_keeps_the_old_key_and_warns_once() {
        let d = dir("empty");
        let f = d.join("key");
        std::fs::write(&f, "old").unwrap();
        let s = KeySource::file_with_interval(&f, Duration::ZERO).unwrap();
        std::fs::write(&f, "  \n").unwrap();
        assert_eq!(s.current(), "old");
        let first = warned(&s).expect("warned about the empty file");
        assert!(first.contains("empty"), "{first}");
        assert_eq!(s.current(), "old");
        assert_eq!(warned(&s), Some(first));
        std::fs::remove_file(&f).unwrap();
        assert_eq!(s.current(), "old");
        assert_ne!(warned(&s), None);
        // A usable file again clears the warning and takes effect.
        std::fs::write(&f, "back").unwrap();
        assert_eq!(s.current(), "back");
        assert_eq!(warned(&s), None);
    }

    #[test]
    fn unchanged_metadata_is_not_re_read() {
        let d = dir("unchanged");
        let f = d.join("key");
        std::fs::write(&f, "aaaa").unwrap();
        let mtime = std::fs::metadata(&f).unwrap().modified().unwrap();
        let s = KeySource::file_with_interval(&f, Duration::ZERO).unwrap();
        // Same length and mtime: the file is trusted not to have changed.
        std::fs::write(&f, "bbbb").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&f)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
        assert_eq!(s.current(), "aaaa");
    }

    #[test]
    fn checks_at_most_once_per_interval() {
        let d = dir("interval");
        let f = d.join("key");
        std::fs::write(&f, "old").unwrap();
        let s = KeySource::file(&f).unwrap();
        std::fs::write(&f, "new-longer").unwrap();
        assert_eq!(s.current(), "old", "within the first second");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_swap_like_a_kubernetes_secret_is_followed() {
        use std::os::unix::fs::symlink;
        let d = dir("symlink");
        // <d>/key -> ..data/key, <d>/..data -> v1
        std::fs::create_dir_all(d.join("v1")).unwrap();
        std::fs::create_dir_all(d.join("v2")).unwrap();
        std::fs::write(d.join("v1/key"), "same-len-1").unwrap();
        std::fs::write(d.join("v2/key"), "same-len-2").unwrap();
        symlink("v1", d.join("..data")).unwrap();
        symlink("..data/key", d.join("key")).unwrap();
        let s = KeySource::file_with_interval(&d.join("key"), Duration::ZERO).unwrap();
        assert_eq!(s.current(), "same-len-1");
        // Kubernetes swaps ..data atomically via a temporary link and rename.
        symlink("v2", d.join("..data_tmp")).unwrap();
        std::fs::rename(d.join("..data_tmp"), d.join("..data")).unwrap();
        assert_eq!(s.current(), "same-len-2");
    }

    #[test]
    fn startup_fails_on_an_unusable_file() {
        let d = dir("startup");
        assert!(KeySource::file(&d.join("missing")).is_err());
        std::fs::write(d.join("empty"), "\n").unwrap();
        assert!(KeySource::file(&d.join("empty")).is_err());
    }
}
