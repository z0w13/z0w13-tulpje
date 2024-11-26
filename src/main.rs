use std::{sync::Arc, time::Duration};

use poise::serenity_prelude::{self as serenity};
use sqlx::{
    postgres::{PgConnectOptions, PgPoolOptions},
    ConnectOptions,
};
use tracing::{debug, info, log::LevelFilter};

use crate::types::Data;

mod config;
mod events;
mod modules;
mod task;
mod types;
mod util;

#[tokio::main]
async fn main() {
    // load env vars
    dotenvy::dotenv().expect("error loading env vars");

    // set-up logging
    tracing_subscriber::fmt::init();

    info!("Starting DMServ ...");
    info!(" [-] Version:  {}", env!("CARGO_PKG_VERSION"));
    info!(
        " [-] Commit:   {}{}",
        env!("VERGEN_GIT_SHA"),
        match env!("VERGEN_GIT_DIRTY") {
            "true" => "-dirty",
            _ => "",
        }
    );
    info!(" [-] Branch:   {}", env!("VERGEN_GIT_BRANCH"));
    info!(" [-] Built At: {}", env!("VERGEN_BUILD_TIMESTAMP"));

    let config = config::load_config().expect("error loading config from env");
    let connect_opts = config
        .db
        .url
        .parse::<PgConnectOptions>()
        .unwrap_or_else(|_| panic!("couldn't parse db url: {}", config.db.url))
        .log_statements(LevelFilter::Trace)
        .log_slow_statements(LevelFilter::Warn, Duration::from_secs(5));

    info!("connecting to database...");
    let db = PgPoolOptions::new()
        .max_connections(5)
        .connect_with(connect_opts)
        .await
        .expect("error connecting to db");

    info!("running migrations...");
    sqlx::migrate!()
        .run(&db)
        .await
        .expect("error running migrations");

    let intents = serenity::GatewayIntents::all();
    let options = poise::FrameworkOptions {
        pre_command: |ctx| {
            Box::pin(async move {
                debug!("executing command /{}...", ctx.invoked_command_name());
            })
        },
        post_command: |ctx| {
            Box::pin(async move {
                debug!("finished executing command /{}", ctx.invoked_command_name());
            })
        },
        event_handler: |ctx, evt, framework, data| {
            Box::pin(events::handler(ctx, evt, framework, data))
        },

        // registere module commands
        commands: vec![
            modules::fronters::commands(),
            modules::roles::commands(),
            modules::pk::commands(),
            modules::stats::commands(),
        ]
        .into_iter()
        .flatten()
        .collect(),
        ..Default::default()
    };

    let framework = poise::Framework::builder()
        .options(options)
        // ran on initial connection
        .setup(|ctx, ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                // TODO: figure out if shard total can change during runtime,
                //       if so figure out how to handle it
                let data = Arc::new(Data::new(db, ready.shard.unwrap().total));

                // register module tasks
                modules::stats::start_tasks(ctx.to_owned(), data.clone());
                modules::fronters::start_tasks(ctx.to_owned(), data.clone());

                Ok(data.clone())
            })
        })
        .build();

    let client = serenity::ClientBuilder::new(config.bot.token, intents)
        .framework(framework)
        .await;

    client.unwrap().start().await.unwrap();
}
