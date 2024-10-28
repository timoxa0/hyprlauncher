use freedesktop_entry_parser::parse_entry;
use once_cell::sync::Lazy;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use tokio::sync::RwLock;
use walkdir::WalkDir;

pub static APP_CACHE: Lazy<RwLock<HashMap<String, AppEntry>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

#[derive(Clone, Serialize, Deserialize)]
pub struct AppEntry {
    pub name: String,
    pub exec: String,
    pub icon_name: String,
    pub path: String,
    pub launch_count: u32,
}

pub static HEATMAP_PATH: &str = "~/.local/share/hyprlauncher/heatmap.json";

pub fn increment_launch_count(app: &AppEntry) {
    let app_name = app.name.clone();

    tokio::spawn(async move {
        let mut cache = APP_CACHE.write().await;
        if let Some(entry) = cache.get_mut(&app_name) {
            entry.launch_count += 1;
            let count = entry.launch_count;
            tokio::task::spawn_blocking(move || save_heatmap(&app_name, count));
        }
    });
}

fn save_heatmap(name: &str, count: u32) {
    let path = shellexpand::tilde(HEATMAP_PATH).to_string();

    if let Some(dir) = std::path::Path::new(&path).parent() {
        std::fs::create_dir_all(dir).unwrap_or_default();
    }

    let mut heatmap = load_heatmap();
    heatmap.insert(name.to_string(), count);

    if let Ok(contents) = serde_json::to_string(&heatmap) {
        fs::write(path, contents).unwrap_or_default();
    }
}

fn load_heatmap() -> HashMap<String, u32> {
    let path = shellexpand::tilde(HEATMAP_PATH).to_string();
    fs::read_to_string(path)
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .unwrap_or_default()
}

pub async fn load_applications() {
    let heatmap = tokio::task::spawn_blocking(load_heatmap)
        .await
        .unwrap_or_default();

    let path = std::env::var("PATH").unwrap_or_default();
    let path_entries: Vec<_> = path.split(':').collect();

    let mut apps = HashMap::new();

    let results: Vec<_> = path_entries
        .par_iter()
        .flat_map(|path_entry| {
            WalkDir::new(path_entry)
                .follow_links(true)
                .max_depth(1)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.file_type().is_file()
                        && e.metadata()
                            .map(|m| m.permissions().mode() & 0o111 != 0)
                            .unwrap_or(false)
                })
                .filter_map(|entry| {
                    entry.file_name().to_str().map(|name| {
                        let name = name.to_string();
                        let path = entry.path().to_string_lossy().to_string();
                        let launch_count = heatmap.get(&name).copied().unwrap_or_default();

                        let icon_name = find_desktop_entry(&name)
                            .map(|e| e.icon_name)
                            .unwrap_or_else(|| "application-x-executable".to_string());

                        (
                            name.clone(),
                            AppEntry {
                                name,
                                exec: path.clone(),
                                icon_name,
                                path,
                                launch_count,
                            },
                        )
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect();

    for (name, entry) in results {
        apps.insert(name, entry);
    }

    let mut cache = APP_CACHE.write().await;
    *cache = apps;
}

struct DesktopEntry {
    icon_name: String,
}

fn find_desktop_entry(name: &str) -> Option<DesktopEntry> {
    let paths = [
        "/usr/share/applications",
        "/usr/local/share/applications",
        "~/.local/share/applications",
    ];

    for path in paths {
        let desktop_file = format!("{}/{}.desktop", path, name);
        if let Ok(entry) = parse_entry(&desktop_file) {
            if let Some(icon) = entry.section("Desktop Entry").attr("Icon") {
                return Some(DesktopEntry {
                    icon_name: icon.to_string(),
                });
            }
        }
    }
    None
}
