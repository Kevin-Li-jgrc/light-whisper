use tauri::utils::config::{BuildConfig, FrontendDist};

#[path = "../frontend_build_support.rs"]
mod frontend_build_support;

#[test]
fn release_guard_rejects_drive_paths_urls_and_missing_homepages() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for value in [
        "D:/build/dist",
        "C:\\build\\dist",
        "file:///D:/build/dist",
        "https://example.com",
        "/tmp/dist",
        "../missing-frontend-fixture",
        "",
    ] {
        assert!(
            frontend_build_support::validate_frontend_dist(root, &serde_json::json!(value))
                .is_err(),
            "{value}"
        );
    }
    assert!(
        frontend_build_support::validate_frontend_dist(root, &serde_json::Value::Null).is_err()
    );
}

#[test]
fn production_package_embeds_a_directory_instead_of_navigating_to_a_drive_url() {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.package.conf.json")).unwrap();
    let build: BuildConfig = serde_json::from_value(config["build"].clone()).unwrap();
    assert!(
        matches!(build.frontend_dist, Some(FrontendDist::Directory(_))),
        "Packaged frontend must be embedded, not treated as a URL: {:?}",
        build.frontend_dist
    );
}

#[test]
fn default_production_config_also_embeds_the_frontend() {
    let config: tauri::utils::config::Config =
        serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
    assert!(matches!(
        config.build.frontend_dist,
        Some(FrontendDist::Directory(_))
    ));
}
