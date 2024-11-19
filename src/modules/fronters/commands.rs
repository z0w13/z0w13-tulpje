use std::collections::{HashMap, HashSet};
use std::error;
use std::fmt;

use pkrs::model::PkId;
use poise::serenity_prelude::{self as serenity, CacheHttp};
use serde_either::StringOrStruct;

use crate::modules::fronters::db;
use crate::types::{Context, Error};
use crate::util::get_member_name;

#[derive(Debug, Clone)]
struct NoFronterCategoryError {
    id: String,
    name: String,
}

impl error::Error for NoFronterCategoryError {}

impl fmt::Display for NoFronterCategoryError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "No fronter category for server '{}' ({})",
            self.name, self.id
        )
    }
}

async fn get_desired_fronters(system: &PkId, token: String) -> Result<HashSet<String>, Error> {
    let pk = pkrs::client::PkClient {
        token: token.into(),
        ..Default::default()
    };

    let fronters = pk
        .get_system_fronters(system)
        .await?
        .members
        .into_iter()
        .filter_map(|m| match m {
            StringOrStruct::String(_) => None,
            StringOrStruct::Struct(member) => Some(get_member_name(&member)),
        })
        .collect();

    Ok(fronters)
}

async fn get_fronter_channels(
    ctx: &serenity::Context,
    guild: serenity::GuildId,
    cat_id: serenity::ChannelId,
) -> Result<Vec<serenity::GuildChannel>, Error> {
    let channels = ctx
        .http
        .get_guild(guild)
        .await?
        .channels(&ctx)
        .await?
        .into_values()
        .filter(|c| c.parent_id.is_some() && c.parent_id.unwrap() == cat_id)
        .collect();

    Ok(channels)
}

async fn get_fronter_category(
    ctx: &serenity::Context,
    guild: &serenity::PartialGuild,
    opt_cat_name: Option<String>,
) -> Result<serenity::GuildChannel, Error> {
    let cat_name = opt_cat_name
        .unwrap_or("current fronters".into())
        .to_lowercase();

    match guild
        .channels(&ctx)
        .await?
        .into_values()
        .find(|c| c.name.to_lowercase() == cat_name && c.kind == serenity::ChannelType::Category)
    {
        None => {
            return Err(NoFronterCategoryError {
                id: guild.id.to_string(),
                name: guild.name.to_owned(),
            }
            .into());
        }
        Some(cat) => Ok(cat),
    }
}

// TODO: Instrument why this bitch slow, are we even using discord's cache?
//       should definitely do that
pub(crate) async fn update_fronter_channels(
    ctx: &serenity::Context,
    guild: serenity::PartialGuild,
    cat: serenity::GuildChannel,
) -> Result<(), Error> {
    let fronter_channels = get_fronter_channels(ctx, guild.id, cat.id).await?;
    let desired_fronters = get_desired_fronters(&PkId("***REMOVED***".into()), "".into()).await?;
    let current_fronters: HashSet<String> =
        fronter_channels.iter().map(|c| c.name.to_owned()).collect();

    let mut fronter_channel_map: HashMap<String, serenity::GuildChannel> = fronter_channels
        .iter()
        .map(|c| (c.name.to_owned(), c.to_owned()))
        .collect();

    let fronter_pos_map: HashMap<String, u16> = desired_fronters
        .iter()
        .enumerate()
        // WARN: could this result in a panic/error? usize into u16
        .map(|(k, v)| (v.to_owned(), k.try_into().unwrap()))
        .collect();

    let delete_fronters = current_fronters.difference(&desired_fronters);
    let create_fronters = desired_fronters.difference(&current_fronters);

    for fronter in delete_fronters {
        let channel = fronter_channel_map.get(fronter).unwrap();
        if let Err(e) = channel.delete(&ctx).await {
            println!("error deleting channel '{}': {}", fronter, e);
            continue;
        }

        fronter_channel_map.remove(fronter);
    }

    for fronter in create_fronters {
        let pos = fronter_pos_map
            .get(fronter)
            .expect("couldn't get position for fronter, this should never happen!");

        let permissions = vec![serenity::PermissionOverwrite {
            deny: serenity::Permissions::CONNECT,
            allow: serenity::Permissions::empty(),
            kind: serenity::PermissionOverwriteType::Role(guild.id.everyone_role()),
        }];

        let channel_create = serenity::CreateChannel::new(fronter)
            .position(*pos)
            .category(cat.id)
            .permissions(permissions)
            .kind(serenity::ChannelType::Voice);

        let channel = match guild.create_channel(&ctx, channel_create).await {
            Ok(chan) => chan,
            Err(e) => {
                println!("error creating fronter channel '{}': {}", fronter, e);
                continue;
            }
        };

        fronter_channel_map.insert(fronter.to_owned(), channel.to_owned());
    }
    for (name, position) in fronter_pos_map.iter() {
        let mut channel = fronter_channel_map
            .get(name)
            .expect("couldn't get channel from fronter_channel_map, this should never happen!")
            .to_owned();

        if channel.position == *position {
            continue;
        }

        if let Err(e) = channel
            .edit(&ctx, serenity::EditChannel::new().position(*position))
            .await
        {
            println!("error moving channel '{}': {}", name, e);
            continue;
        }
    }

    Ok(())
}

#[poise::command(slash_command, guild_only = true, rename = "update-fronters")]
pub(crate) async fn update_fronters(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let guild = ctx
        .partial_guild()
        .await
        .ok_or("couldn't get guild from context")?;

    let cat_id = db::get_fronter_category(&ctx.data().db, guild.id.get())
        .await?
        .ok_or("fronter category not set-up, please run /setup-fronters")?;

    let cat = ctx
        .http()
        .get_channel(cat_id.into())
        .await?
        .guild()
        .ok_or(format!("channel {} isn't a guild channel", cat_id))?;

    match update_fronter_channels(ctx.serenity_context(), guild, cat).await {
        // TODO: Figure out if there's a nicer way to match errors like this
        Err(e) => match e.downcast_ref::<NoFronterCategoryError>() {
            Some(_) => {
                return Err("no fronters category in the server, please run /setup-fronters".into())
            }
            None => return Err(e),
        },
        Ok(_) => (),
    };

    ctx.reply("fronter list updated!").await?;
    Ok(())
}

async fn create_or_get_fronter_channel(
    ctx: &serenity::Context,
    guild: &serenity::PartialGuild,
    cat_name: String,
) -> Result<serenity::GuildChannel, Error> {
    let fronters_category = get_fronter_category(ctx, guild, Some(cat_name.to_owned())).await;

    if let Err(err) = fronters_category {
        // other errors, return
        if err.downcast_ref::<NoFronterCategoryError>().is_some() {
            return Err(err);
        }

        let permissions = vec![serenity::PermissionOverwrite {
            deny: serenity::Permissions::VIEW_CHANNEL,
            allow: serenity::Permissions::empty(),
            kind: serenity::PermissionOverwriteType::Role(guild.id.everyone_role()),
        }];

        let builder = serenity::CreateChannel::new(cat_name)
            .kind(serenity::ChannelType::Category)
            .permissions(permissions);

        Ok(guild.create_channel(ctx.http(), builder).await?)

        // category doesn't exist create it
    } else {
        return Ok(fronters_category.unwrap());
    }
}

#[poise::command(slash_command, guild_only = true, rename = "setup-fronters")]
pub(crate) async fn setup_fronters(
    ctx: Context<'_>,
    #[description = "Name of the fronters category"] name: String,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let guild = ctx.partial_guild().await.ok_or("couldn't fetch guild")?;
    let fronters_category =
        create_or_get_fronter_channel(ctx.serenity_context(), &guild, name).await?;

    // Save category into db
    db::save_fronter_category(&ctx.data().db, guild.id.get(), fronters_category.id.get()).await?;

    // Inform user of success
    ctx.reply("fronter list setup!").await?;
    Ok(())
}
