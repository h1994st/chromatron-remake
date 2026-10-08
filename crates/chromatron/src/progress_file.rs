//! The save file: `~/Library/Application Support/Chromatron N/com.silverspaceship.chromatron.dat`,
//! byte for byte the original's format (platform.md P7), so an old save can be dropped in.

use std::cell::Cell;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::{env, fs, io};

use lumen::{SaveData, decode_save};

/// The original's file name (no game number in it).
pub const SAVE_FILE_NAME: &str = "com.silverspaceship.chromatron.dat";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgressFile {
    path: PathBuf,
    /// Set by `load` when a save it could not use is still in place: `store` then refuses.
    locked: Cell<bool>,
}

impl ProgressFile {
    /// The save file inside `dir`.
    pub fn in_dir(dir: &Path) -> ProgressFile {
        ProgressFile {
            path: dir.join(SAVE_FILE_NAME),
            locked: Cell::new(false),
        }
    }

    /// `~/Library/Application Support/<support_dir>/`. `None` without a home directory.
    pub fn for_app(support_dir: &str) -> Option<ProgressFile> {
        let home = env::home_dir()?;
        Some(ProgressFile::in_dir(
            &home.join("Library/Application Support").join(support_dir),
        ))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Where a corrupt save is moved: the file name plus `.bak`.
    pub fn backup_path(&self) -> PathBuf {
        let mut name = self.path.as_os_str().to_owned();
        name.push(".bak");
        PathBuf::from(name)
    }

    /// The saved progress. A missing file is a fresh game. A corrupt or unreadable one is renamed
    /// to `.bak`, logged, and gives a fresh game (spec §7); a corrupt file replaces any older
    /// `.bak`. If such a file cannot be moved aside either, the game still starts fresh but this
    /// `ProgressFile` is locked: `store` refuses, so the unread save is never overwritten.
    pub fn load(&self) -> SaveData {
        let (reason, bytes) = match fs::read(&self.path) {
            Ok(bytes) => match decode_save(&bytes) {
                Ok(save) => return save,
                Err(e) => (format!("is corrupt ({e})"), bytes),
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => return SaveData::default(),
            Err(e) => (format!("cannot be read ({e})"), Vec::new()),
        };
        drop(bytes);
        let backup = self.backup_path();
        match fs::rename(&self.path, &backup) {
            Ok(()) => log::warn!(
                "{} {reason}; moved it to {} and starting fresh",
                self.path.display(),
                backup.display()
            ),
            Err(rename) => {
                self.locked.set(true);
                log::error!(
                    "{} {reason} and cannot be moved aside ({rename}); starting fresh without saving",
                    self.path.display()
                );
            }
        }
        SaveData::default()
    }

    /// Writes the whole file: into a temporary file next to it, then renamed over it, so a crash
    /// mid-write never leaves a truncated save.
    pub fn store(&self, bytes: &[u8]) -> io::Result<()> {
        if self.locked.get() {
            return Err(io::Error::other(
                "the existing save could not be read or moved aside",
            ));
        }
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut temporary = self.path.as_os_str().to_owned();
        temporary.push(".tmp");
        // Flushed to disk before the rename, so a crash never leaves the new name on empty data.
        let mut file = fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_file_lives_in_application_support() {
        let file = ProgressFile::for_app("Chromatron 1").expect("tests run with a home directory");
        let home = env::home_dir().unwrap();
        assert_eq!(
            file.path(),
            home.join(
                "Library/Application Support/Chromatron 1/com.silverspaceship.chromatron.dat"
            )
        );
        assert_eq!(
            file.backup_path(),
            home.join(
                "Library/Application Support/Chromatron 1/com.silverspaceship.chromatron.dat.bak"
            )
        );
    }

    #[test]
    fn a_missing_file_is_a_fresh_game() {
        let dir = tempfile::tempdir().unwrap();
        let file = ProgressFile::in_dir(&dir.path().join("Chromatron 1"));
        assert_eq!(file.load(), SaveData::default());
        assert!(!file.backup_path().exists());
    }

    #[test]
    fn store_creates_the_folder_and_load_reads_it_back() {
        let dir = tempfile::tempdir().unwrap();
        let file = ProgressFile::in_dir(&dir.path().join("Chromatron 1"));
        let mut bytes = vec![0u8; 32];
        bytes[0] = 0x01;
        file.store(&bytes).unwrap();
        assert_eq!(fs::read(file.path()).unwrap(), bytes);
        let save = file.load();
        assert!(
            save.completed[0],
            "slot 0 alone is `01` then zeros (platform.md P7)"
        );
        assert!(save.completed[1..].iter().all(|&c| !c));
        file.store(&[0u8; 32]).unwrap();
        assert_eq!(fs::read(file.path()).unwrap(), [0u8; 32]);
    }

    #[test]
    fn a_corrupt_file_is_moved_to_bak_and_the_game_starts_fresh() {
        let dir = tempfile::tempdir().unwrap();
        let file = ProgressFile::in_dir(dir.path());
        // Slot 0's check value is 1, so an all-ones bitmap is corrupt (platform.md P7).
        let corrupt = [0xffu8; 32];
        fs::write(file.path(), corrupt).unwrap();
        assert_eq!(file.load(), SaveData::default());
        assert!(!file.path().exists());
        assert_eq!(fs::read(file.backup_path()).unwrap(), corrupt);
        // A second corrupt save (a record for level id 200) replaces the old backup.
        let mut worse = vec![0u8; 32 + 856];
        worse[32] = 200;
        fs::write(file.path(), &worse).unwrap();
        assert_eq!(file.load(), SaveData::default());
        assert_eq!(fs::read(file.backup_path()).unwrap(), worse);
    }

    #[test]
    fn a_short_file_is_a_fresh_game_and_stays_put() {
        let dir = tempfile::tempdir().unwrap();
        let file = ProgressFile::in_dir(dir.path());
        fs::write(file.path(), [1u8; 10]).unwrap();
        assert_eq!(file.load(), SaveData::default());
        assert!(file.path().exists());
    }

    #[test]
    fn an_unwritable_folder_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("Chromatron 1");
        fs::write(&blocker, b"a file where the folder should be").unwrap();
        let file = ProgressFile::in_dir(&blocker);
        assert!(file.store(&[0u8; 32]).is_err());
        assert_eq!(file.load(), SaveData::default());
    }

    /// Makes `path` unreadable for the length of the test, restoring access on drop so the temp
    /// dir can be cleaned up.
    struct Unreadable(PathBuf, u32);
    impl Unreadable {
        fn new(path: &Path) -> Unreadable {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(path).unwrap().permissions().mode();
            fs::set_permissions(path, fs::Permissions::from_mode(0o000)).unwrap();
            Unreadable(path.to_owned(), mode)
        }
    }
    impl Drop for Unreadable {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(self.1));
        }
    }

    #[test]
    fn an_unreadable_save_is_moved_aside_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let file = ProgressFile::in_dir(dir.path());
        let mut bytes = vec![0u8; 32];
        bytes[0] = 1;
        fs::write(file.path(), &bytes).unwrap();
        {
            let _guard = Unreadable::new(file.path());
            assert_eq!(file.load(), SaveData::default());
        }
        // The folder was writable, so the file went to `.bak` and saving is safe again.
        assert!(!file.path().exists());
        // The moved file keeps its mode; open it up to compare.
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(file.backup_path(), fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(fs::read(file.backup_path()).unwrap(), bytes);
        file.store(&[0u8; 32]).unwrap();
        assert_eq!(fs::read(file.backup_path()).unwrap(), bytes);
    }

    #[test]
    fn a_save_that_can_be_neither_read_nor_moved_is_never_written() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = ProgressFile::in_dir(dir.path());
        let mut bytes = vec![0u8; 32];
        bytes[0] = 1;
        fs::write(file.path(), &bytes).unwrap();
        let _file_guard = Unreadable::new(file.path());
        let _dir_guard = Unreadable(dir.path().to_owned(), 0o755);
        // A read-only folder: the rename to `.bak` fails too.
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
        assert_eq!(file.load(), SaveData::default());
        assert!(file.store(&[0u8; 32]).is_err(), "saving is disabled");
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(
            file.store(&[0u8; 32]).is_err(),
            "even once the folder is writable again"
        );
        fs::set_permissions(file.path(), fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            fs::read(file.path()).unwrap(),
            bytes,
            "the save is untouched"
        );
    }

    #[test]
    fn a_corrupt_save_that_cannot_be_moved_is_never_written() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = ProgressFile::in_dir(dir.path());
        fs::write(file.path(), [0xffu8; 32]).unwrap();
        let _dir_guard = Unreadable(dir.path().to_owned(), 0o755);
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o500)).unwrap();
        assert_eq!(file.load(), SaveData::default());
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(file.store(&[0u8; 32]).is_err());
        assert_eq!(fs::read(file.path()).unwrap(), [0xffu8; 32]);
    }
}
