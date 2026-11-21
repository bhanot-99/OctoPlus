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

    // Verify config.toml and nudges.toml exist with 0600
    let cfg_file = cfg_dir.join("config.toml");
    assert!(cfg_file.exists());
    assert_eq!(
        fs::metadata(&cfg_file).unwrap().permissions().mode() & 0o777,
        0o600
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
    };
    let rc2 = run_init(opts2, styler).await;
    assert_eq!(rc2, 0);
    assert_eq!(fs::read(&hmac_file).unwrap(), hmac_data);
    assert_eq!(fs::read(&fetch_file).unwrap(), fetch_data);
}
