use crate::launcher::{AppEntry, APP_CACHE};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use rayon::prelude::*;
use tokio::sync::oneshot;

pub struct SearchResult {
    pub app: AppEntry,
    pub score: i64,
}

pub async fn search_applications(query: &str) -> Vec<SearchResult> {
    let (tx, rx) = oneshot::channel();

    let query = query.to_string();
    tokio::task::spawn_blocking(move || {
        let cache = APP_CACHE.blocking_read();
        let results = if query.is_empty() {
            let mut results: Vec<_> = cache
                .values()
                .par_bridge()
                .map(|app| {
                    let base_score = if app.launch_count > 0 {
                        (app.launch_count as i64 * 100) + 1000
                    } else {
                        0
                    };
                    let image_score = if app.icon_name != "application-x-executable" {
                        500
                    } else {
                        0
                    };
                    SearchResult {
                        app: app.clone(),
                        score: base_score + image_score,
                    }
                })
                .collect();
            results.par_sort_unstable_by(|a, b| b.score.cmp(&a.score));
            results.truncate(100);
            results
        } else {
            let matcher = SkimMatcherV2::default().smart_case();
            let cache_vec: Vec<_> = cache.values().collect();

            let mut results: Vec<SearchResult> = cache_vec
                .par_iter()
                .filter_map(|app| {
                    matcher.fuzzy_match(&app.name, &query).map(|score| {
                        let heat_bonus = if app.launch_count > 0 {
                            (app.launch_count as i64 * 50) + 500
                        } else {
                            0
                        };
                        let image_bonus = if app.icon_name != "application-x-executable" {
                            250
                        } else {
                            0
                        };
                        SearchResult {
                            app: (*app).clone(),
                            score: score + heat_bonus + image_bonus,
                        }
                    })
                })
                .collect();

            results.par_sort_unstable_by(|a, b| b.score.cmp(&a.score));
            results.truncate(100);
            results
        };
        let _ = tx.send(results);
    });

    rx.await.unwrap_or_default()
}
