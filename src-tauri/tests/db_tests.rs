use std::path::PathBuf;
use xandsuite_lib::db::AppDb;

#[test]
fn test_db_open_and_migrations() {
    let tmpdir = tempfile::tempdir().expect("Failed to create temp dir");
    let db = AppDb::open(&PathBuf::from(tmpdir.path())).expect("Failed to open DB");

    // Verify tables exist
    let count: i64 = db.conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='conversations'",
        [],
        |row| row.get(0),
    ).unwrap();
    assert_eq!(count, 1, "conversations table should exist");
}

#[test]
fn test_settings_roundtrip() {
    let tmpdir = tempfile::tempdir().expect("Failed to create temp dir");
    let db = AppDb::open(&PathBuf::from(tmpdir.path())).expect("Failed to open DB");

    db.set_setting("test_key", "test_value").unwrap();
    let value = db.get_setting("test_key").unwrap();
    assert_eq!(value, Some("test_value".to_string()));
}

#[test]
fn test_missing_setting_returns_none() {
    let tmpdir = tempfile::tempdir().expect("Failed to create temp dir");
    let db = AppDb::open(&PathBuf::from(tmpdir.path())).expect("Failed to open DB");

    let value = db.get_setting("nonexistent_key").unwrap();
    assert_eq!(value, None);
}
