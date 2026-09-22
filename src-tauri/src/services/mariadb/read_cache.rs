//! Executable discovery only: never cache credentials, connections, or table metadata.
//! Inspired by Hunter Corlett's client cache in fork 53f183437438109093b4a73a747b1d650ce31dc9.

use std::{
    ffi::OsString,
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

const CLIENT_TTL: Duration = Duration::from_secs(60);

#[derive(Default)]
struct ClientCache {
    entry: Option<(PathBuf, Option<OsString>, Instant)>,
}

impl ClientCache {
    fn resolve(
        &mut self,
        now: Instant,
        search_path: Option<OsString>,
        valid: impl Fn(&PathBuf) -> bool,
        discover: impl FnOnce() -> Option<PathBuf>,
    ) -> Option<PathBuf> {
        if let Some((path, previous_search_path, cached_at)) = &self.entry {
            if *previous_search_path == search_path
                && now.saturating_duration_since(*cached_at) < CLIENT_TTL
                && path.is_absolute()
                && valid(path)
            {
                return Some(path.clone());
            }
        }
        self.entry = None;
        let path = discover().filter(|path| path.is_absolute() && valid(path))?;
        self.entry = Some((path.clone(), search_path, now));
        Some(path)
    }
}

static CLIENT: Mutex<ClientCache> = Mutex::new(ClientCache { entry: None });

pub(super) fn find(discover: impl FnOnce() -> Option<PathBuf>) -> Option<PathBuf> {
    // Serialize cache misses so concurrent reads do not each launch discovery processes.
    // Reset takes the same lock, so an in-flight discovery cannot repopulate a cleared cache.
    CLIENT
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .resolve(
            Instant::now(),
            std::env::var_os("PATH"),
            |path| path.is_file(),
            discover,
        )
}

pub(super) fn reset() {
    CLIENT
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .entry = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn fixture() -> PathBuf {
        std::env::temp_dir().join("fixture-mariadb.exe")
    }

    #[test]
    fn repeated_reads_reuse_discovery_not_database_results() {
        let mut cache = ClientCache::default();
        let calls = Cell::new(0);
        let discover = || {
            calls.set(calls.get() + 1);
            Some(fixture())
        };
        for _ in 0..10 {
            assert_eq!(
                cache.resolve(Instant::now(), None, |_| true, discover),
                Some(fixture())
            );
        }
        assert_eq!(calls.get(), 1);
        cache.entry = None;
        cache.resolve(Instant::now(), None, |_| true, discover);
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn expired_deleted_and_path_changed_clients_are_rediscovered() {
        let now = Instant::now();
        let mut cache = ClientCache::default();
        for (time, path, valid) in [
            (now + CLIENT_TTL, None, true),
            (now, None, false),
            (now, Some(OsString::from("changed")), true),
        ] {
            cache.entry = Some((fixture(), None, now));
            let called = Cell::new(false);
            cache.resolve(
                time,
                path,
                |_| valid,
                || {
                    called.set(true);
                    None
                },
            );
            assert!(called.get());
            assert!(cache.entry.is_none());
        }
    }

    #[test]
    fn relative_missing_and_failed_discoveries_are_not_cached() {
        let mut cache = ClientCache::default();
        for discovered in [None, Some(PathBuf::from("mariadb.exe")), Some(fixture())] {
            assert!(cache
                .resolve(Instant::now(), None, |_| false, || discovered)
                .is_none());
            assert!(cache.entry.is_none());
        }
        assert!(cache
            .resolve(
                Instant::now(),
                None,
                |_| true,
                || Some(PathBuf::from("mariadb.exe"))
            )
            .is_none());
    }
}
