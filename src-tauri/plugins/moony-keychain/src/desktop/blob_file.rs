//! The private file that holds the wrapped secret on the desktop.
//!
//! The macOS and Windows natives do not store the secret themselves: they wrap it with a key that
//! only the biometry can use, and the wrapped blob lives here, next to the database. The blob is
//! useless without the biometric-gated key, but it is still kept private (`0o600` on unix) and
//! written atomically, so a crash never leaves a half-written blob that would later fail to unwrap.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Where the blob for `key` lives: `<dir>/<key>.bin`.
pub fn path(dir: &Path, key: &str) -> PathBuf {
    dir.join(format!("{key}.bin"))
}

/// Writes `bytes` as the blob for `key`: a temp file in the same directory, then a rename over the
/// target, so a reader sees either the old blob or the new one, never a partial write.
pub fn write(dir: &Path, key: &str, bytes: &[u8]) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let target = path(dir, key);
    let temp = dir.join(format!("{key}.bin.tmp"));

    let result = write_temp(&temp, bytes).and_then(|()| fs::rename(&temp, &target));
    if result.is_err() {
        // Best effort: do not leave the temp file behind when the write or the rename failed.
        let _ = fs::remove_file(&temp);
    }
    result
}

fn write_temp(temp: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(temp)?;
    #[cfg(unix)]
    {
        // `mode` only applies when the file is created; a stale temp file from a crashed write
        // keeps its old permissions, so set them explicitly before the secret goes in.
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)?;
    file.sync_all()
}

/// Reads the blob for `key`; `None` when there is none.
pub fn read(dir: &Path, key: &str) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path(dir, key)) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Deletes the blob for `key`; a missing blob is fine (removing twice must not fail).
pub fn remove(dir: &Path, key: &str) -> io::Result<()> {
    match fs::remove_file(path(dir, key)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "moony.master-key";

    #[test]
    fn path_is_the_key_with_a_bin_extension() {
        assert_eq!(
            path(Path::new("/data"), KEY),
            Path::new("/data").join("moony.master-key.bin")
        );
    }

    #[test]
    fn write_then_read_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), KEY, &[1, 2, 3, 4]).expect("write");
        assert_eq!(read(dir.path(), KEY).expect("read"), Some(vec![1, 2, 3, 4]));
    }

    #[test]
    fn read_missing_is_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(read(dir.path(), KEY).expect("read"), None);
    }

    #[test]
    fn remove_missing_is_ok() {
        let dir = tempfile::tempdir().expect("tempdir");
        remove(dir.path(), KEY).expect("remove a blob that was never written");
    }

    #[test]
    fn remove_deletes_the_blob() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), KEY, &[9]).expect("write");
        remove(dir.path(), KEY).expect("remove");
        assert_eq!(read(dir.path(), KEY).expect("read"), None);
    }

    #[test]
    fn write_replaces_atomically_and_leaves_no_temp_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), KEY, &[1, 1, 1]).expect("first write");
        write(dir.path(), KEY, &[2, 2]).expect("second write");

        assert_eq!(read(dir.path(), KEY).expect("read"), Some(vec![2, 2]));
        let entries = fs::read_dir(dir.path()).expect("read_dir").count();
        assert_eq!(entries, 1, "only the blob itself is left in the directory");
    }

    #[test]
    fn write_creates_a_missing_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("app-data");
        write(&nested, KEY, &[7]).expect("write");
        assert_eq!(read(&nested, KEY).expect("read"), Some(vec![7]));
    }

    #[cfg(unix)]
    #[test]
    fn written_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        write(dir.path(), KEY, &[1]).expect("write");
        let mode = fs::metadata(path(dir.path(), KEY))
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
