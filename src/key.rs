//! A Typesense API key that, when it comes from a file, follows that file.
//!
//! Rotating a key (for example a Kubernetes Secret update) rewrites the file;
//! polling its metadata on use avoids a restart without a file-watcher.
//! The lock is only held to read or update the cached state, never across
//! file system calls, so a hung mount can't block other callers.

use std::{
    fmt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};

/// How often a key file's metadata is looked at, at most.
const CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// What tells a rewritten file from an unchanged one: modified time and
/// length, plus, on unix, the inode, device and change time, which also catch
/// a rewrite that keeps the length and restores the mtime (`cp -p`, `rsync -t`)
/// or a permission fix.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Sig {
    modified: SystemTime,
    len: u64,
    #[cfg(unix)]
    unix: (u64, u64, i64, i64),
}

type Signature = Option<Sig>;

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
    /// Signature of the file as last read successfully; a failed read leaves
    /// it, so the next check tries again.
    signature: Signature,
    last_check: Instant,
    /// A caller is looking at the file; others use the cached key meanwhile.
    checking: bool,
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
                    checking: false,
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
        let seen = {
            let mut state = self.lock();
            if state.checking || state.last_check.elapsed() < self.interval {
                return state.key.clone();
            }
            state.checking = true;
            state.last_check = Instant::now();
            state.signature.clone()
        };
        // Unlocked: the file system may be slow or hung.
        let now = signature(&self.path);
        let read = (now.is_none() || now != seen).then(|| crate::cli::read_key_file(&self.path));

        let mut state = self.lock();
        state.checking = false;
        match read {
            None => {}
            Some(Ok(key)) => {
                state.signature = now;
                state.warned = None;
                if key != state.key {
                    state.key = key;
                    tracing::info!(
                        "reloaded the Typesense API key from {}",
                        self.path.display()
                    );
                }
            }
            Some(Err(err)) => {
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

    fn lock(&self) -> std::sync::MutexGuard<'_, FileState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Follows symlinks, as Kubernetes Secret volumes swap a `..data` link.
fn signature(path: &Path) -> Signature {
    let meta = std::fs::metadata(path).ok()?;
    #[cfg(unix)]
    let unix = {
        use std::os::unix::fs::MetadataExt;
        (meta.ino(), meta.dev(), meta.ctime(), meta.ctime_nsec())
    };
    Some(Sig {
        modified: meta.modified().ok()?,
        len: meta.len(),
        #[cfg(unix)]
        unix,
    })
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

    fn stored_signature(source: &KeySource) -> Signature {
        match &*source.inner {
            Inner::File(f) => f.state.lock().unwrap().signature.clone(),
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
    fn a_rewrite_with_the_same_length_and_mtime_is_noticed_on_unix() {
        let d = dir("unchanged");
        let f = d.join("key");
        std::fs::write(&f, "aaaa").unwrap();
        let mtime = std::fs::metadata(&f).unwrap().modified().unwrap();
        let s = KeySource::file_with_interval(&f, Duration::ZERO).unwrap();
        // Coarse change-time clocks need a moment between the two writes.
        std::thread::sleep(Duration::from_millis(50));
        // Same length and mtime, as `cp -p` or `rsync -t` leave it.
        std::fs::write(&f, "bbbb").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&f)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
        // The change time (unix only) tells the files apart; elsewhere the
        // rewrite is invisible, as documented.
        let expected = if cfg!(unix) { "bbbb" } else { "aaaa" };
        assert_eq!(s.current(), expected);
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_read_is_retried_so_a_permission_fix_takes_effect() {
        use std::os::unix::fs::PermissionsExt;
        let d = dir("chmod");
        let f = d.join("key");
        std::fs::write(&f, "old").unwrap();
        let s = KeySource::file_with_interval(&f, Duration::ZERO).unwrap();
        let good = stored_signature(&s);
        std::fs::write(&f, "new-key").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read(&f).is_ok() {
            return; // root ignores permissions
        }
        assert_eq!(s.current(), "old");
        assert_ne!(warned(&s), None);
        assert_eq!(
            stored_signature(&s),
            good,
            "a failed read is not remembered"
        );
        // Only the change time moves: the mtime and length stay as they were.
        std::thread::sleep(Duration::from_millis(50));
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(s.current(), "new-key");
        assert_eq!(warned(&s), None);
    }

    #[test]
    fn a_key_that_is_not_a_header_value_is_rejected_without_naming_it() {
        let d = dir("bom");
        let f = d.join("key");
        std::fs::write(&f, "\u{feff}secret-key").unwrap();
        let err = KeySource::file(&f).unwrap_err().to_string();
        assert!(err.contains("HTTP header"), "{err}");
        assert!(err.contains(f.to_str().unwrap()), "{err}");
        assert!(!err.contains("secret-key"), "{err}");
        // On reload the previous key stays and the problem is warned about.
        std::fs::write(&f, "good-key").unwrap();
        let s = KeySource::file_with_interval(&f, Duration::ZERO).unwrap();
        std::fs::write(&f, "\u{feff}secret-key-2").unwrap();
        assert_eq!(s.current(), "good-key");
        assert!(warned(&s).unwrap().contains("HTTP header"));
        std::fs::write(&f, "bad\x01key-longer").unwrap();
        assert_eq!(s.current(), "good-key");
    }

    #[cfg(unix)]
    #[test]
    fn a_slow_file_system_does_not_block_other_callers() {
        use std::{process::Command, sync::mpsc, thread};
        let d = dir("fifo");
        std::fs::write(d.join("key"), "old").unwrap();
        let link = d.join("link");
        std::os::unix::fs::symlink(d.join("key"), &link).unwrap();
        let s = KeySource::file_with_interval(&link, Duration::ZERO).unwrap();
        // Point the key at a FIFO: reading it blocks until someone writes.
        let fifo = d.join("fifo");
        assert!(Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success());
        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&fifo, &link).unwrap();

        let slow = {
            let s = s.clone();
            thread::spawn(move || s.current())
        };
        thread::sleep(Duration::from_millis(300));
        let (tx, rx) = mpsc::channel();
        let s2 = s.clone();
        thread::spawn(move || tx.send(s2.current()));
        let other = rx.recv_timeout(Duration::from_secs(2));
        // Unblock the reader whatever the outcome.
        std::fs::write(&fifo, "fifo-key").unwrap();
        assert_eq!(other.expect("blocked behind the slow read"), "old");
        assert_eq!(slow.join().unwrap(), "fifo-key");
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
