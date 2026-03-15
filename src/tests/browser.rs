use crate::App;
use crate::settings::Settings;
use std::path::PathBuf;
use std::collections::HashSet;

#[test]
fn test_add_folder_updates_browser_state() {
    let mut app = App::new_headless();
    assert!(app.sample_browser.root_folders.is_empty());
    assert!(!app.sample_browser.visible);

    // Use a real directory so `add_folder` / `from_state` accept it
    let tmp = std::env::temp_dir();
    app.sample_browser.add_folder(tmp.clone());

    assert_eq!(app.sample_browser.root_folders.len(), 1);
    assert_eq!(app.sample_browser.root_folders[0], tmp);
    assert!(app.sample_browser.expanded.contains(&tmp));
}

#[test]
fn test_add_duplicate_folder_ignored() {
    let mut app = App::new_headless();
    let tmp = std::env::temp_dir();

    app.sample_browser.add_folder(tmp.clone());
    app.sample_browser.add_folder(tmp.clone());

    assert_eq!(app.sample_browser.root_folders.len(), 1);
}

#[test]
fn test_settings_sample_library_folders_roundtrip() {
    let mut settings = Settings::default();
    assert!(settings.sample_library_folders.is_empty());

    settings.sample_library_folders = vec![
        "/tmp/samples_a".to_string(),
        "/tmp/samples_b".to_string(),
    ];

    let json = serde_json::to_string(&settings).unwrap();
    let loaded: Settings = serde_json::from_str(&json).unwrap();

    assert_eq!(loaded.sample_library_folders.len(), 2);
    assert_eq!(loaded.sample_library_folders[0], "/tmp/samples_a");
    assert_eq!(loaded.sample_library_folders[1], "/tmp/samples_b");
}

#[test]
fn test_settings_without_sample_folders_defaults_empty() {
    // Simulate loading a settings.json written before this field existed
    let json = r#"{"grid_line_intensity":0.26,"brightness":1.0,"color_intensity":0.0}"#;
    let loaded: Settings = serde_json::from_str(json).unwrap();
    assert!(loaded.sample_library_folders.is_empty());
}

#[test]
fn test_browser_from_state_restores_global_folders() {
    let tmp = std::env::temp_dir();
    let folders = vec![tmp.clone()];
    let mut expanded = HashSet::new();
    expanded.insert(tmp.clone());

    let browser =
        crate::ui::browser::SampleBrowser::from_state(folders, expanded, true);

    assert_eq!(browser.root_folders.len(), 1);
    assert_eq!(browser.root_folders[0], tmp);
    assert!(browser.visible);
    assert!(browser.expanded.contains(&tmp));
}

#[test]
fn test_browser_merge_global_and_project_folders() {
    // Simulates the startup merge logic: global settings folders + project folders
    let tmp = std::env::temp_dir();
    let global_folders = vec![tmp.clone()];
    let project_folders: Vec<PathBuf> = vec![];

    let mut merged = global_folders.clone();
    for f in &project_folders {
        if !merged.contains(f) {
            merged.push(f.clone());
        }
    }

    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0], tmp);

    // Now with overlapping folders — no duplicates
    let project_folders_2 = vec![tmp.clone()];
    let mut merged2 = global_folders.clone();
    for f in &project_folders_2 {
        if !merged2.contains(f) {
            merged2.push(f.clone());
        }
    }
    assert_eq!(merged2.len(), 1);
}
