use std::{path::PathBuf, time::Duration};

use modules::fronters::update_fronter_channels;
use poise::serenity_prelude::{self as serenity, CacheHttp, GuildId};
use serde::Deserialize;
use tokio::time::MissedTickBehavior;

use crate::types::{Error, UserData};

mod modules;
mod types;
mod util;

#[derive(Deserialize, Debug)]
struct Config {
    token: String,
}

fn load_config() -> Result<Config, Error> {
    let conf: Config = serde_envfile::prefixed("DMSERV_").from_file(&PathBuf::from("env"))?;

    Ok(conf)
}

fn start_long_running_tasks(ctx: serenity::Context) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let guild = ctx
                .http()
                .get_guild(GuildId::new(0000000000000000000 /* REDACTED */))
                .await;

            if let Err(e) = guild {
                println!("Error fetching guild: {}", e);
                continue;
            }

            if let Err(e) = update_fronter_channels(&ctx, guild.unwrap()).await {
                println!("Error updating fronters: {}", e);
            } else {
                println!("Fronters updated");
            }
        }
    });
}

#[tokio::main]
async fn main() {
    let config = load_config().expect("error loading envfile");

    let intents = serenity::GatewayIntents::all();
    let options = poise::FrameworkOptions {
        commands: vec![
            modules::roles::update_member_roles(),
            modules::fronters::update_fronters(),
        ],
        ..Default::default()
    };

    let framework = poise::Framework::builder()
        .options(options)
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;

                start_long_running_tasks(ctx.to_owned());

                Ok(UserData {})
            })
        })
        .build();

    let client = serenity::ClientBuilder::new(config.token, intents)
        .framework(framework)
        .await;

    client.unwrap().start().await.unwrap();
}
