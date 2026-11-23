//! The demo profile's folder and lifecycle (ADR 0037): `<app data folder>/demo` holds `moony.db`,
//! `attachments.db` and [`STATE_FILE`], the completeness marker that also records when the demo was
//! last opened. The databases use [`DEMO_KEY_HEX`], a fixed key that protects nothing: it only lets
//! the demo use the same SQLCipher storage as every profile. There is no `salt`, `key.enc` or
//! `recovery.enc` in the folder.
//!
//! Creating the demo is not atomic (the generator's parts commit one by one, `demo::seed`), so the
//! marker is written last: a creation that fails closes the database and deletes the folder, and a
//! folder found without the marker is deleted and generated again.
//!
//! Every function that deletes checks the folder first ([`check_demo_folder`]): its name is `demo`,
//! it is a real folder (no symlink), and no real profile's key file is in it. [`discard`] also
//! requires the folder to be `<real folder>/demo` and the demo to be closed.

use std::fs;
use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{DemoStatus, InsertUserProfile, UserProfile};
use crate::services::backup::DB_FILE_NAME;
use crate::services::demo::{self, text::LANGUAGES, DemoOptions};
use crate::services::{auth, profile_dir};

/// The demo databases' SQLCipher key, in the `'hex'` form `Database::create_with_key` takes. It is
/// the bytes of `moony-demo-profile-not-a-secret.` and NOT a secret: the demo holds synthetic data
/// only, and the key is the same in every installation.
pub const DEMO_KEY_HEX: &str = "'6d6f6f6e792d64656d6f2d70726f66696c652d6e6f742d612d7365637265742e'";

/// The completeness marker and state file inside the demo folder, written last.
pub const STATE_FILE: &str = "demo-profile.json";

/// The format of [`STATE_FILE`].
const STATE_VERSION: u32 = 1;

/// A real profile's key files: never found in, and never deleted with, a demo folder.
const REAL_KEY_FILES: [&str; 3] = ["salt", "key.enc", "recovery.enc"];

/// [`STATE_FILE`]'s content.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DemoState {
    #[serde(rename = "version")]
    version: u32,
    #[serde(rename = "createdAt")]
    created_at: i64,
    #[serde(rename = "lastOpenedAt")]
    last_opened_at: i64,
}

/// One of the ten UI languages (the part before a region suffix, lower case); English for anything
/// else.
pub fn language_code(language: &str) -> &'static str {
    let base = language
        .split(['-', '_'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    LANGUAGES
        .iter()
        .find(|code| **code == base)
        .copied()
        .unwrap_or("en")
}

/// Whether a complete demo is stored and when it was last opened. A demo next to a real profile
/// counts as none (it is deleted at the next start); `active` is the process-wide flag.
pub fn status(real_dir: &Path, demo_dir: &Path) -> DemoStatus {
    let state = if real_profile_exists(real_dir) {
        None
    } else {
        read_state(demo_dir)
    };
    DemoStatus {
        exists: state.is_some(),
        last_opened_at: state.map(|s| s.last_opened_at),
        active: profile_dir::demo_open(),
    }
}

/// Generate the demo into `demo_dir` (an incomplete folder there is deleted first) and leave it
/// open in `db`: the profile row of Alex Meyer (EUR, `language`), the data set with its documents,
/// then the marker. On any error the database is closed and the folder deleted.
///
/// The caller makes sure no real profile session is open in `db`: creating replaces it.
pub fn create(db: &Database, demo_dir: &Path, language: &str, now: i64) -> Result<UserProfile> {
    create_with(db, demo_dir, language, now, |conn, opts| {
        demo::seed(conn, opts).map(|_| ())
    })
}

/// [`create`] with the generator passed in, so a test can make it fail.
fn create_with(
    db: &Database,
    demo_dir: &Path,
    language: &str,
    now: i64,
    seed: impl FnOnce(&Connection, &DemoOptions) -> Result<()>,
) -> Result<UserProfile> {
    check_demo_folder(demo_dir)?;
    remove_folder(demo_dir)?;
    let language = language_code(language);
    let built = (|| {
        fs::create_dir_all(demo_dir)?;
        db.create_with_key(demo_dir.join(DB_FILE_NAME), DEMO_KEY_HEX)?;
        let options = DemoOptions {
            language: language.to_string(),
            now,
            attachments: true,
        };
        db.with_conn(|conn| {
            auth::insert_user_profile(conn, persona(language))?;
            seed(conn, &options)
        })?;
        let profile = read_profile(db)?;
        write_state(
            demo_dir,
            &DemoState {
                version: STATE_VERSION,
                created_at: now,
                last_opened_at: now,
            },
        )?;
        Ok(profile)
    })();
    built.inspect_err(|e| {
        log::warn!("[DEMO] Generating the demo profile failed, its folder goes: {e}");
        db.close();
        if let Err(removal) = remove_folder(demo_dir) {
            log::warn!("[DEMO] The incomplete demo folder could not be removed: {removal}");
        }
    })
}

/// Open the stored demo in `db` with the fixed key, record when, and switch the profile's language
/// to `language` when it differs (the data keeps the names it was generated with). A folder
/// without the marker, or one whose database does not open, is generated again.
pub fn open(db: &Database, demo_dir: &Path, language: &str, now: i64) -> Result<UserProfile> {
    check_demo_folder(demo_dir)?;
    let Some(mut state) = read_state(demo_dir) else {
        log::info!("[DEMO] No complete demo profile is stored: generating it");
        return create(db, demo_dir, language, now);
    };
    let language = language_code(language);
    let opened = (|| {
        db.open_with_key(demo_dir.join(DB_FILE_NAME), DEMO_KEY_HEX)?;
        db.with_conn(|conn| set_profile_language(conn, language))?;
        read_profile(db)
    })();
    match opened {
        Ok(profile) => {
            state.last_opened_at = now;
            if let Err(e) = write_state(demo_dir, &state) {
                log::warn!("[DEMO] Could not record when the demo was opened: {e}");
            }
            Ok(profile)
        }
        Err(e) => {
            log::warn!("[DEMO] The stored demo profile did not open, generating it again: {e}");
            db.close();
            create(db, demo_dir, language, now)
        }
    }
}

/// Close the demo's database.
pub fn close(db: &Database) {
    db.close();
}

/// Throw the open demo away and generate it again in the profile's current language.
pub fn reset(db: &Database, demo_dir: &Path, now: i64) -> Result<UserProfile> {
    check_demo_folder(demo_dir)?;
    let language = read_profile(db)?.language;
    close(db);
    remove_folder(demo_dir)?;
    create(db, demo_dir, &language, now)
}

/// Delete the stored demo. Refused unless `demo_dir` is `<real_dir>/demo` and passes
/// [`check_demo_folder`], and while the demo is open (close it first). A missing folder is fine.
pub fn discard(real_dir: &Path, demo_dir: &Path) -> Result<()> {
    if demo_dir != profile_dir::demo_dir_in(real_dir) {
        return Err(AppError::Internal(
            "Refusing to delete a folder that is not the demo profile's".into(),
        ));
    }
    if profile_dir::demo_open() {
        return Err(AppError::Internal(
            "The demo profile is open: close it before deleting it".into(),
        ));
    }
    remove_folder(demo_dir)
}

/// Delete a demo folder that sits next to a real profile (at startup, and once a real profile was
/// created or restored); whether one was deleted.
pub fn discard_beside_real_profile(real_dir: &Path, demo_dir: &Path) -> Result<bool> {
    if !real_profile_exists(real_dir) || fs::symlink_metadata(demo_dir).is_err() {
        return Ok(false);
    }
    discard(real_dir, demo_dir)?;
    Ok(true)
}

/// Alex Meyer, in EUR, with the UI in `language`.
fn persona(language: &str) -> InsertUserProfile {
    InsertUserProfile {
        name: demo::PERSONA_NAME.into(),
        surname: demo::PERSONA_SURNAME.into(),
        email: demo::PERSONA_EMAIL.into(),
        menu_preferences: None,
        currency: Some(demo::PERSONA_CURRENCY.into()),
        language: Some(language.into()),
        exclude_personal_real_estate: Some(false),
    }
}

fn read_profile(db: &Database) -> Result<UserProfile> {
    auth::get_user_profile(db)?
        .ok_or_else(|| AppError::Internal("The demo profile has no profile row".into()))
}

/// The profile's language only; no other row changes.
fn set_profile_language(conn: &Connection, language: &str) -> Result<()> {
    conn.execute(
        "UPDATE user_profile SET language = ?1 WHERE language IS NOT ?1",
        [language],
    )?;
    Ok(())
}

fn real_profile_exists(real_dir: &Path) -> bool {
    auth::database_exists(&real_dir.join(DB_FILE_NAME))
}

/// The marker's content, or `None` when it is missing or unreadable (an incomplete demo).
fn read_state(demo_dir: &Path) -> Option<DemoState> {
    let text = fs::read_to_string(demo_dir.join(STATE_FILE)).ok()?;
    serde_json::from_str::<DemoState>(&text)
        .ok()
        .filter(|state| state.version == STATE_VERSION)
}

/// Write the marker through a staging file and a rename, so a half-written marker never exists.
fn write_state(demo_dir: &Path, state: &DemoState) -> Result<()> {
    let path = demo_dir.join(STATE_FILE);
    let staging = demo_dir.join(format!("{STATE_FILE}.tmp"));
    fs::write(&staging, serde_json::to_vec(state)?)?;
    fs::rename(&staging, &path)?;
    Ok(())
}

/// The defence before anything is deleted: the folder is named `demo`, is not a symlink or a file,
/// and holds none of a real profile's key files. A missing folder passes.
fn check_demo_folder(demo_dir: &Path) -> Result<()> {
    let refuse = |why: &str| {
        Err(AppError::Internal(format!(
            "Not a demo profile folder: {why}"
        )))
    };
    if demo_dir.file_name().and_then(|n| n.to_str()) != Some(profile_dir::DEMO_DIR_NAME) {
        return refuse("wrong name");
    }
    match fs::symlink_metadata(demo_dir) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
            return refuse("not a plain folder")
        }
        Ok(_) => {}
        Err(_) => return Ok(()),
    }
    if REAL_KEY_FILES
        .iter()
        .any(|name| fs::symlink_metadata(demo_dir.join(name)).is_ok())
    {
        return refuse("it holds a real profile's key files");
    }
    Ok(())
}

/// Delete the demo folder after [`check_demo_folder`]; a missing one is fine.
fn remove_folder(demo_dir: &Path) -> Result<()> {
    check_demo_folder(demo_dir)?;
    match fs::remove_dir_all(demo_dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(AppError::Internal(format!(
            "Failed to delete the demo profile: {e}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::profile_dir::demo_flag_for_test;
    use chrono::{TimeZone, Utc};
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    const DAY: i64 = 86_400;

    /// "Today" of the tests: a fixed day in the past, so that `now() + n * DAY` stays before the
    /// wall clock (the bank service refuses a balance dated after the real today).
    fn now() -> i64 {
        Utc.with_ymd_and_hms(2026, 11, 20, 12, 0, 0)
            .single()
            .expect("date")
            .timestamp()
    }

    /// A temp app data folder and its demo folder.
    fn folders() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().expect("tempdir");
        let real = root.path().join("app-data");
        fs::create_dir_all(&real).expect("real dir");
        let demo = profile_dir::demo_dir_in(&real);
        (root, real, demo)
    }

    /// A real profile's files with recognisable bytes.
    fn fake_real_profile(real: &Path) -> BTreeMap<&'static str, Vec<u8>> {
        let files = [
            ("moony.db", b"real database".to_vec()),
            ("attachments.db", b"real attachments".to_vec()),
            ("salt", b"real salt".to_vec()),
            ("key.enc", b"real key".to_vec()),
            ("recovery.enc", b"real recovery".to_vec()),
        ];
        for (name, bytes) in &files {
            fs::write(real.join(name), bytes).expect("write real file");
        }
        files.into_iter().collect()
    }

    fn assert_untouched(real: &Path, files: &BTreeMap<&'static str, Vec<u8>>) {
        for (name, bytes) in files {
            assert_eq!(
                &fs::read(real.join(name)).expect("real file still there"),
                bytes,
                "{name} changed"
            );
        }
    }

    fn count(db: &Database, sql: &str) -> i64 {
        db.with_conn(|conn| Ok(conn.query_row(sql, [], |r| r.get(0))?))
            .expect("count")
    }

    fn account_names(db: &Database) -> Vec<String> {
        db.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT name FROM bank_accounts ORDER BY id")?;
            let names = stmt
                .query_map([], |r| r.get(0))?
                .collect::<std::result::Result<Vec<String>, _>>()?;
            Ok(names)
        })
        .expect("names")
    }

    fn add_marker_row(db: &Database) {
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO app_config (key, value) VALUES ('test.userChange', '1')",
                [],
            )?;
            Ok(())
        })
        .expect("user change");
    }

    fn marker_rows(db: &Database) -> i64 {
        count(
            db,
            "SELECT COUNT(*) FROM app_config WHERE key = 'test.userChange'",
        )
    }

    #[test]
    fn language_codes_fall_back_to_english() {
        assert_eq!(language_code("cs"), "cs");
        assert_eq!(language_code("de-AT"), "de");
        assert_eq!(language_code("ZH_cn"), "zh");
        assert_eq!(language_code("ar"), "en");
        assert_eq!(language_code(""), "en");
    }

    #[test]
    fn a_created_demo_is_reported_and_open() {
        let _seed = demo::seed_test_lock();
        let (_root, real, demo_dir) = folders();
        assert!(!status(&real, &demo_dir).exists);

        let db = Database::new();
        let profile = create(&db, &demo_dir, "en", now()).expect("create");

        assert_eq!(profile.name, demo::PERSONA_NAME);
        assert_eq!(profile.surname, demo::PERSONA_SURNAME);
        assert_eq!(profile.currency, "EUR");
        assert_eq!(profile.language, "en");
        assert!(db.is_open());
        assert!(count(&db, "SELECT COUNT(*) FROM bank_transactions") > 1_000);
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM files.blobs"),
            2,
            "the two documents went into the demo's attachments store"
        );
        for name in ["moony.db", "attachments.db", STATE_FILE] {
            assert!(demo_dir.join(name).exists(), "{name}");
        }
        for name in REAL_KEY_FILES {
            assert!(!demo_dir.join(name).exists(), "no {name} in the demo");
        }
        let reported = status(&real, &demo_dir);
        assert!(reported.exists);
        assert_eq!(reported.last_opened_at, Some(now()));
        assert!(!reported.active, "the flag is the command's to set");
    }

    #[test]
    fn opening_again_keeps_what_the_person_changed() {
        let _seed = demo::seed_test_lock();
        let (_root, real, demo_dir) = folders();
        let db = Database::new();
        create(&db, &demo_dir, "en", now()).expect("create");
        add_marker_row(&db);
        close(&db);
        assert!(!db.is_open());

        open(&db, &demo_dir, "en", now() + DAY).expect("first open");
        assert_eq!(marker_rows(&db), 1);
        close(&db);
        open(&db, &demo_dir, "en", now() + 2 * DAY).expect("second open");
        assert_eq!(marker_rows(&db), 1, "the change is still there");
        assert_eq!(
            status(&real, &demo_dir).last_opened_at,
            Some(now() + 2 * DAY)
        );
    }

    #[test]
    fn reset_restores_the_generated_data_in_the_current_language() {
        let _seed = demo::seed_test_lock();
        let (_root, _real, demo_dir) = folders();
        let db = Database::new();
        create(&db, &demo_dir, "de", now()).expect("create");
        let transactions = count(&db, "SELECT COUNT(*) FROM bank_transactions");
        let config_rows = count(&db, "SELECT COUNT(*) FROM app_config");
        add_marker_row(&db);
        db.with_conn(|conn| {
            conn.execute("UPDATE user_profile SET name = 'Someone else'", [])?;
            Ok(())
        })
        .expect("rename");

        let profile = reset(&db, &demo_dir, now()).expect("reset");

        assert_eq!(profile.name, demo::PERSONA_NAME);
        assert_eq!(profile.language, "de", "the profile's language is kept");
        assert_eq!(marker_rows(&db), 0);
        assert_eq!(count(&db, "SELECT COUNT(*) FROM app_config"), config_rows);
        assert_eq!(
            count(&db, "SELECT COUNT(*) FROM bank_transactions"),
            transactions
        );
    }

    #[test]
    fn opening_in_another_language_changes_only_the_profile_language() {
        let _seed = demo::seed_test_lock();
        let (_root, _real, demo_dir) = folders();
        let db = Database::new();
        create(&db, &demo_dir, "en", now()).expect("create");
        let names = account_names(&db);
        close(&db);

        let profile = open(&db, &demo_dir, "cs", now() + DAY).expect("open in Czech");

        assert_eq!(profile.language, "cs");
        assert_eq!(account_names(&db), names, "the data keeps its names");
    }

    #[test]
    fn a_failed_generation_leaves_no_folder() {
        let (_root, real, demo_dir) = folders();
        let db = Database::new();

        let err = create_with(&db, &demo_dir, "en", now(), |_, _| {
            Err(AppError::Internal("generator stopped".into()))
        })
        .expect_err("the generator fails");

        assert!(err.to_string().contains("generator stopped"));
        assert!(!demo_dir.exists(), "the half-made folder is gone");
        assert!(!db.is_open(), "the half-made database is closed");
        assert!(!status(&real, &demo_dir).exists);
    }

    #[test]
    fn a_folder_without_the_marker_is_generated_again() {
        let _seed = demo::seed_test_lock();
        let (_root, real, demo_dir) = folders();
        let db = Database::new();
        create(&db, &demo_dir, "en", now()).expect("create");
        add_marker_row(&db);
        close(&db);
        fs::remove_file(demo_dir.join(STATE_FILE)).expect("interrupt");
        assert!(
            !status(&real, &demo_dir).exists,
            "incomplete counts as none"
        );

        let profile = open(&db, &demo_dir, "en", now() + DAY).expect("open");

        assert_eq!(profile.name, demo::PERSONA_NAME);
        assert_eq!(marker_rows(&db), 0, "a fresh demo");
        assert_eq!(status(&real, &demo_dir).last_opened_at, Some(now() + DAY));
    }

    #[test]
    fn discard_refuses_anything_but_the_closed_demo_folder() {
        let (_root, real, demo_dir) = folders();
        fs::create_dir_all(&demo_dir).expect("demo dir");
        let other = real.join("elsewhere").join("demo");
        fs::create_dir_all(&other).expect("other dir");
        let renamed = real.join("demo2");
        fs::create_dir_all(&renamed).expect("renamed dir");

        assert!(discard(&real, &real).is_err(), "the real folder");
        assert!(discard(&real, &other).is_err(), "a demo folder elsewhere");
        assert!(discard(&real, &renamed).is_err(), "another name");
        assert!(other.exists() && renamed.exists());

        fs::write(demo_dir.join("key.enc"), b"real key").expect("key file");
        assert!(
            discard(&real, &demo_dir).is_err(),
            "a folder with a real key file"
        );
        assert!(demo_dir.join("key.enc").exists());
        fs::remove_file(demo_dir.join("key.enc")).expect("remove key");

        {
            let _open = demo_flag_for_test(true);
            assert!(discard(&real, &demo_dir).is_err(), "while the demo is open");
        }
        assert!(demo_dir.exists());

        discard(&real, &demo_dir).expect("the closed demo folder");
        assert!(!demo_dir.exists());
        discard(&real, &demo_dir).expect("a missing folder is fine");
    }

    #[cfg(unix)]
    #[test]
    fn discard_refuses_a_symlinked_demo_folder() {
        let (_root, real, demo_dir) = folders();
        let target = real.join("target");
        fs::create_dir_all(&target).expect("target");
        fs::write(target.join("moony.db"), b"keep").expect("file");
        std::os::unix::fs::symlink(&target, &demo_dir).expect("symlink");

        assert!(discard(&real, &demo_dir).is_err());
        assert!(target.join("moony.db").exists());
    }

    #[test]
    fn the_real_profile_is_never_touched() {
        let _seed = demo::seed_test_lock();
        let (_root, real, demo_dir) = folders();
        let files = fake_real_profile(&real);
        let db = Database::new();

        create(&db, &demo_dir, "en", now()).expect("create");
        assert_untouched(&real, &files);
        reset(&db, &demo_dir, now()).expect("reset");
        assert_untouched(&real, &files);
        close(&db);
        discard(&real, &demo_dir).expect("discard");
        assert_untouched(&real, &files);
        assert!(!demo_dir.exists());
    }

    #[test]
    fn a_demo_next_to_a_real_profile_is_deleted_and_not_offered() {
        let (_root, real, demo_dir) = folders();
        fs::create_dir_all(&demo_dir).expect("demo dir");
        write_state(
            &demo_dir,
            &DemoState {
                version: STATE_VERSION,
                created_at: now(),
                last_opened_at: now(),
            },
        )
        .expect("marker");
        assert!(status(&real, &demo_dir).exists);
        assert!(
            !discard_beside_real_profile(&real, &demo_dir).expect("no real profile"),
            "kept while no real profile exists"
        );
        assert!(demo_dir.exists());

        let files = fake_real_profile(&real);
        assert!(!status(&real, &demo_dir).exists, "not offered any more");
        assert!(discard_beside_real_profile(&real, &demo_dir).expect("discard"));
        assert!(!demo_dir.exists());
        assert_untouched(&real, &files);
        assert!(
            !discard_beside_real_profile(&real, &demo_dir).expect("again"),
            "nothing left to delete"
        );
    }
}
