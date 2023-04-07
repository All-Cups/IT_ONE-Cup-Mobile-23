use actix::spawn;
use anyhow::Context;
use futures::{channel::mpsc, FutureExt, StreamExt};
use log::{debug, info};
use std::{io::Write, net::SocketAddr, path::PathBuf, time::Duration};

mod codehub;
mod logger;
mod model;
mod serde_duration;
mod server;

#[derive(clap::Parser)]
struct CliArgs {
    #[clap(long)]
    config: Option<PathBuf>,
    #[clap(long = "user")]
    users: Vec<model::UserToken>,
    #[clap(long)]
    save_log: Option<PathBuf>,
    #[clap(long)]
    save_results: Option<PathBuf>,
    #[clap(long, default_value = "127.0.0.1:8080")]
    addr: SocketAddr,
    #[clap(long)]
    serve_dir: Option<PathBuf>,
}

async fn run(codehub_config: Option<&codehub::Config>) -> anyhow::Result<()> {
    let mut args: CliArgs = clap::Parser::parse();
    let mut config: model::Config = match &args.config {
        Some(path) => {
            if path.to_str() == Some("-") {
                serde_json::from_reader(std::io::stdin().lock())
            } else {
                serde_json::from_reader(
                    std::fs::File::open(path).context("Failed to open config file")?,
                )
            }
        }
        .context("Failed to parse config")?,
        None => model::Config::default(),
    };
    if let Some(codehub_config) = &codehub_config {
        args.users = codehub_config.user_id_by_token.keys().cloned().collect();
        if let Some(time) = codehub_config.time_to_run {
            config.time_to_run = Some(time);
        }
        args.save_log = Some("game_log.jsonl".into());
    }

    let time_to_run = config.time_to_run.map(Duration::from_secs_f64);
    let enable_logs_api = codehub_config.is_none();
    let serve_dir = args.serve_dir.as_ref().filter(|_| codehub_config.is_none());

    let app = model::App::init(config, args.users);
    let log_writer = if let Some(path) = &args.save_log {
        let user_map = codehub_config.map(|config| config.user_id_by_token.clone());
        let (sender, mut receiver) = mpsc::unbounded();
        app.register_logs(sender.clone()).await;
        let file = std::fs::File::create(path).context("Failed to create log file")?;
        Some((
            sender,
            // Need to spawn here otherwise work only done on .await
            spawn(async move {
                let mut writer = std::io::BufWriter::new(file);
                while let Some(entry) = receiver.next().await {
                    if let Some(user_map) = &user_map {
                        serde_json::to_writer(
                            &mut writer,
                            &entry.map_user(|token| user_map[&token]),
                        )?;
                    } else {
                        serde_json::to_writer(&mut writer, &entry)?;
                    }
                    writeln!(&mut writer)?;
                }
                anyhow::Ok(())
            }),
        ))
    } else {
        None
    };

    let app = server::run(args.addr, app, time_to_run, serve_dir, enable_logs_api).await?;

    if let Some((sender, task)) = log_writer {
        app.unregister_logs(&sender).await;
        std::mem::drop(sender);
        // Wait for the log writer to finish
        // It should be finishing since sender is dropped
        task.await??;
    }

    let results = app.results().await;

    info!("Results: {results:#?}");
    if let Some(path) = &args.save_results {
        debug!("Saving results to {path:?}");
        serde_json::to_writer_pretty(
            std::io::BufWriter::new(
                std::fs::File::create(path).expect("Failed to create results file"),
            ),
            &results,
        )
        .expect("Failed to write results");
    }

    if let Some(codehub_config) = &codehub_config {
        codehub::write_game_log(
            codehub_config,
            args.save_log.as_ref().unwrap(),
            codehub::Results {
                players: None,
                results: results
                    .into_iter()
                    .map(|(token, score)| (codehub_config.user_id_by_token[&token], score as f64))
                    .collect(),
                seed: None,
            },
        );
    }

    Ok(())
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    logger::init();
    codehub::wrapper(|config| run(config).boxed_local()).await
}
