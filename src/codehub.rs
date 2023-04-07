//! Codehub specific logic

use crate::model;
use futures::future::LocalBoxFuture;
use log::info;
use serde::Serialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

type UserId = i64;

#[derive(Debug)]
pub struct Config {
    pub summary_path: PathBuf,
    pub time_to_run: Option<f64>,
    pub user_id_by_token: HashMap<model::UserToken, UserId>,
}

pub fn detect() -> Option<Config> {
    // GAME_LOG_LOCATION is not actual game log location :) I call it summary
    if let Some(summary_path) = std::env::var_os("GAME_LOG_LOCATION") {
        let summary_path = summary_path.into();
        let clients_json = std::env::var("CLIENTS_JSON").expect("CLIENTS_JSON env var expected");
        let client_tokens: HashMap<UserId, model::UserToken> =
            serde_json::from_str(&clients_json).expect("Failed to parse CLIENTS_JSON");
        let user_id_by_token = client_tokens
            .into_iter()
            .map(|(id, token)| (token, id))
            .collect();
        let config = Config {
            summary_path,
            user_id_by_token,
            time_to_run: std::env::var("TIME_TO_RUN")
                .ok()
                .map(|time| time.parse().expect("Failed to parse TIME_TO_RUN")),
        };
        info!("Detected codehub: {config:#?}");
        Some(config)
    } else {
        None
    }
}

// Reports "user" errors to game log if running on codehub
pub async fn wrapper<F>(f: F) -> anyhow::Result<()>
where
    F: FnOnce(Option<&Config>) -> LocalBoxFuture<anyhow::Result<()>>,
    // LocalBoxFuture reason:
    // https://github.com/rust-lang/rust/issues/52662#issuecomment-475164924
{
    let config = detect();
    let config = config.as_ref();
    if let Err(e) = f(config).await {
        if let Some(config) = config {
            #[derive(Debug, Serialize)]
            struct Results {
                errors: Vec<String>,
            }
            let results = Results {
                errors: vec![e.to_string()],
            };
            serde_json::to_writer_pretty(
                std::fs::File::create(&config.summary_path)
                    .expect("Failed to create results file (errors)"),
                &results,
            )
            .expect("Failed to write errors");
            Ok(())
        } else {
            Err(e)
        }
    } else {
        Ok(())
    }
}

#[derive(Debug, serde::Serialize)]
pub struct PlayerResult {
    pub crashed: bool,
    pub crash_tick: Option<usize>,
    pub time_used: Option<f64>,
    pub comment: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct Results {
    pub players: Option<HashMap<UserId, PlayerResult>>,
    pub results: HashMap<UserId, f64>,
    pub seed: Option<u64>,
}

pub fn write_game_log(config: &Config, game_log_path: impl AsRef<Path>, results: Results) {
    let results_path = "results.json";
    serde_json::to_writer_pretty(
        std::fs::File::create(results_path).expect("Failed to create results file"),
        &results,
    )
    .expect("Failed to write results");

    #[derive(Debug, Serialize)]
    struct File {
        filename: String,
        location: PathBuf,
        is_private: bool,
    }
    impl File {
        fn new(path: impl AsRef<Path>, private: bool) -> Self {
            let path = path.as_ref();
            Self {
                filename: path.file_name().unwrap().to_str().unwrap().to_owned(),
                location: path.canonicalize().unwrap(),
                is_private: private,
            }
        }
    }
    #[derive(Debug, Serialize)]
    struct Summary {
        visio: File,
        scores: File,
    }
    let results = Summary {
        visio: File::new(game_log_path, false),
        scores: File::new(results_path, false),
    };
    serde_json::to_writer_pretty(
        std::fs::File::create(&config.summary_path).expect("Failed to create summary file"),
        &results,
    )
    .expect("Failed to write summary");
}
