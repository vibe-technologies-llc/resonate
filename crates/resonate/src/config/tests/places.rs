use super::*;

#[test]
fn windows_keeps_settings_in_roaming_data_and_everything_else_in_local_data() {
    assert_eq!(windows_variable_for("XDG_CONFIG_HOME"), "APPDATA");
    assert_eq!(windows_variable_for("XDG_DATA_HOME"), "LOCALAPPDATA");
    assert_eq!(windows_variable_for("XDG_CACHE_HOME"), "LOCALAPPDATA");
}

#[test]
fn a_mac_keeps_settings_and_data_in_application_support_and_what_is_made_again_in_caches() {
    assert_eq!(
        library_folder_for("XDG_CONFIG_HOME"),
        "Library/Application Support"
    );
    assert_eq!(
        library_folder_for("XDG_DATA_HOME"),
        "Library/Application Support"
    );
    assert_eq!(library_folder_for("XDG_CACHE_HOME"), "Library/Caches");
}

#[test]
#[cfg(unix)]
fn a_data_folder_is_entered_by_its_owner_alone_whatever_it_was_made_as() {
    use std::os::unix::fs::PermissionsExt as _;

    let scratch = Scratch::new();
    let folder = scratch.path.parent().expect("a folder").join("data");

    fs::create_dir_all(&folder).expect("a writable temporary directory");
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).expect("a mode");

    entered_by_its_owner_alone(&folder).expect("a narrowed folder");

    let mode = fs::metadata(&folder)
        .expect("the folder")
        .permissions()
        .mode()
        & 0o777;

    assert_eq!(mode, 0o700);
}

#[test]
#[cfg(all(unix, feature = "sync"))]
fn a_store_and_its_write_ahead_files_are_their_owners_alone_made_now_or_before() {
    use std::os::unix::fs::PermissionsExt as _;

    let mode = |path: &Path| {
        fs::metadata(path)
            .expect("a store file")
            .permissions()
            .mode()
            & 0o777
    };
    let scratch = Scratch::new();
    let folder = scratch.path.parent().expect("a folder").join("stores");

    fs::create_dir_all(&folder).expect("a writable temporary directory");

    let fresh = folder.join("fresh.sqlite");
    let old = folder.join("old.sqlite");
    let old_wal = folder.join("old.sqlite-wal");
    let old_shm = folder.join("old.sqlite-shm");

    for existing in [&old, &old_wal, &old_shm] {
        fs::write(existing, b"rows").expect("a store file");
        fs::set_permissions(existing, fs::Permissions::from_mode(0o644)).expect("a mode");
    }

    private_store(&fresh).expect("a fresh store");
    private_store(&old).expect("an old store");

    assert_eq!(mode(&fresh), 0o600);
    assert_eq!(mode(&old), 0o600);
    assert_eq!(mode(&old_wal), 0o600);
    assert_eq!(mode(&old_shm), 0o600);
    assert_eq!(fs::read(&old).expect("the store"), b"rows");
}
