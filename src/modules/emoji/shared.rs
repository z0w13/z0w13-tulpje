use std::{collections::HashMap, sync::Arc};

use poise::serenity_prelude as serenity;

use super::db;
use crate::types::Error;

#[derive(Debug, PartialEq, poise::ChoiceParameter)]
pub(crate) enum StatsSort {
    #[name = "Most Used"]
    CountDesc,
    #[name = "Least Used"]
    CountAsc,
    #[name = "Most Recent"]
    DateDesc,
    #[name = "Least Recent"]
    DateAsc,
}

impl StatsSort {
    // alias poise::ChoiceParameter::name to avoid extra imports
    pub(crate) fn name(&self) -> &'static str {
        poise::ChoiceParameter::name(self)
    }

    pub(crate) fn id(&self) -> &'static str {
        match self {
            Self::CountDesc => "count_desc",
            Self::CountAsc => "count_asc",
            Self::DateDesc => "date_desc",
            Self::DateAsc => "date_asc",
        }
    }

    pub(crate) fn try_from_string(string: &str) -> Result<Self, Error> {
        match string {
            "count_desc" => Ok(Self::CountDesc),
            "count_asc" => Ok(Self::CountAsc),
            "date_desc" => Ok(Self::DateDesc),
            "date_asc" => Ok(Self::DateAsc),
            _ => Err(format!("unknown sort {}", string).into()),
        }
    }
}

impl From<StatsSort> for serenity::CreateSelectMenuOption {
    fn from(val: StatsSort) -> Self {
        serenity::CreateSelectMenuOption::new(val.name(), val.id())
    }
}

pub(crate) fn parse_emojis_from_string(guild_id: u64, content: &str) -> Vec<db::Emoji> {
    let re = regex::Regex::new(r"<(a?):([[:word:]]+):([[:digit:]]+)>").unwrap();
    re.captures_iter(content)
        .map(|caps| {
            let (_, [animated, name, id]) = caps.extract();
            db::Emoji {
                animated: animated == "a",
                guild_id,
                id: id.parse::<u64>().unwrap(),
                name: name.to_string(),
            }
        })
        .collect()
}

// using i16 for count because a discord message can currently be max 2000 characters
// so we definitely can't have 32_768 emoji in a single message
pub(crate) fn count_emojis(emojis: Vec<db::Emoji>) -> HashMap<db::Emoji, i16> {
    let mut counts = HashMap::new();
    for emoji in emojis {
        if let Some(count) = counts.get_mut(&emoji) {
            *count += 1;
        } else {
            counts.insert(emoji.clone(), 1);
        }
    }

    counts
}

// TODO: Check if this is a 404 emoji not found so we can assume
//       safely it's an emoji in a different guild
pub(crate) async fn is_guild_emoji(
    http: Arc<serenity::Http>,
    guild_id: serenity::GuildId,
    emoji_id: serenity::EmojiId,
) -> bool {
    http.get_emoji(guild_id, emoji_id).await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_emojis_from_string_test() {
        let result = parse_emojis_from_string(0, "<a:animated:0> <:static:1>");
        assert_eq!(
            result,
            vec![
                db::Emoji {
                    id: 0,
                    guild_id: 0,
                    name: String::from("animated"),
                    animated: true
                },
                db::Emoji {
                    id: 1,
                    guild_id: 0,
                    name: String::from("static"),
                    animated: false
                }
            ]
        )
    }

    #[test]
    fn count_emojis_test() {
        // emoji creation helper func
        fn emoji(id: u64) -> db::Emoji {
            db::Emoji {
                id,
                guild_id: 0,
                name: String::from("foo"),
                animated: false,
            }
        }

        let result = count_emojis(vec![emoji(0), emoji(0), emoji(1)]);
        assert_eq!(result, HashMap::from([(emoji(0), 2), (emoji(1), 1)]));
    }
}
