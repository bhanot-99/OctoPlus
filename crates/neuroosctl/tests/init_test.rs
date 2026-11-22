use std::fs;
use std::os::unix::fs::PermissionsExt;
use tempfile::tempdir;

use neuroosctl::init::{InitOptions, run_init};
use neuroosctl::style::{ColorChoice, Styler};

#[tokio::test]
async fn test_init_provisions_keys_and_configs() {
    let tmp = tempdir().unwrap();
    let cfg_dir = tmp.path().join(".config/neuroos");

    let opts = InitOptions {
        force: false,
        skip_models: true,
        yes: false,
        config_dir: Some(cfg_dir.clone()),
        system_fetch_key: Some(tmp.path().join("no-system.key")),
    };
    let styler = Styler::new(ColorChoice::Never);

    let rc = run_init(opts, styler).await;
    assert_eq!(rc, 0);

    // Verify directory exists with 0700
    let meta = fs::metadata(&cfg_dir).unwrap();
    assert_eq!(meta.permissions().mode() & 0o777, 0o700);

    // Verify hmac.key exists with 0600 and 32 bytes
    let hmac_file = cfg_dir.join("hmac.key");
    assert!(hmac_file.exists());
    let hmac_data = fs::read(&hmac_file).unwrap();
    assert_eq!(hmac_data.len(), 32);
    let hmac_meta = fs::metadata(&hmac_file).unwrap();
    assert_eq!(hmac_meta.permissions().mode() & 0o777, 0o600);

    // Verify fetch.key exists with 0600 and 32 bytes
    let fetch_file = cfg_dir.join("fetch.key");
    assert!(fetch_file.exists());
    let fetch_data = fs::read(&fetch_file).unwrap();
    assert_eq!(fetch_data.len(), 32);
    let fetch_meta = fs::metadata(&fetch_file).unwrap();
    assert_eq!(fetch_meta.permissions().mode() & 0o777, 0o600);

    // Verify config.toml and nudges.toml exist with 0600 and parse without errors
    let cfg_file = cfg_dir.join("config.toml");
    assert!(cfg_file.exists());
    assert_eq!(
        fs::metadata(&cfg_file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let parsed_cfg = neuroos_common::config::Config::load_from(&cfg_file);
    assert!(
        parsed_cfg.is_ok(),
        "config.toml failed schema validation: {:?}",
        parsed_cfg.err()
    );

    let nudges_file = cfg_dir.join("nudges.toml");
    assert!(nudges_file.exists());
    assert_eq!(
        fs::metadata(&nudges_file).unwrap().permissions().mode() & 0o777,
        0o600
    );

    // Running again without force retains identical keys
    let opts2 = InitOptions {
        force: false,
        skip_models: true,
        yes: false,
        config_dir: Some(cfg_dir.clone()),
        system_fetch_key: Some(tmp.path().join("no-system.key")),
    };
    let rc2 = run_init(opts2, styler).await;
    assert_eq!(rc2, 0);
    assert_eq!(fs::read(&hmac_file).unwrap(), hmac_data);
    assert_eq!(fs::read(&fetch_file).unwrap(), fetch_data);
}

/// Regression (audit A-15): the system fetch key is copied byte for byte when readable,
/// init refuses to invent a mismatched key when it is not readable, and --force replaces a
/// stale user key.
#[tokio::test]
async fn init_copies_the_system_fetch_key_or_refuses() {
    let tmp = tempdir().unwrap();
    let styler = Styler::new(ColorChoice::Never);
    let sys = tmp.path().join("etc-fetch.key");
    fs::write(&sys, [7u8; 32]).unwrap();
    fs::set_permissions(&sys, fs::Permissions::from_mode(0o640)).unwrap();
    let opts = |dir: &std::path::Path, force: bool| InitOptions {
        force,
        skip_models: true,
        yes: false,
        config_dir: Some(dir.to_path_buf()),
        system_fetch_key: Some(sys.clone()),
    };

    let a = tmp.path().join("a");
    assert_eq!(run_init(opts(&a, false), styler).await, 0);
    assert_eq!(fs::read(a.join("fetch.key")).unwrap(), [7u8; 32]);
    assert_eq!(
        fs::metadata(a.join("fetch.key"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    // A stale user key is kept without --force and replaced with it.
    fs::write(a.join("fetch.key"), [1u8; 32]).unwrap();
    assert_eq!(run_init(opts(&a, false), styler).await, 0);
    assert_eq!(fs::read(a.join("fetch.key")).unwrap(), [1u8; 32]);
    assert_eq!(run_init(opts(&a, true), styler).await, 0);
    assert_eq!(fs::read(a.join("fetch.key")).unwrap(), [7u8; 32]);

    // Unreadable system key: init fails and writes no fetch key of its own.
    fs::set_permissions(&sys, fs::Permissions::from_mode(0o000)).unwrap();
    if fs::read(&sys).is_err() {
        let b = tmp.path().join("b");
        assert_eq!(run_init(opts(&b, false), styler).await, 1);
        assert!(!b.join("fetch.key").exists());
    }
}
