//! Native Discord attachment delivery. No messages are sent by renderer tests.
use serenity::all::{
    ChannelId, Context, CreateActionRow, CreateAttachment, CreateButton, CreateEmbed,
    CreateMessage, EditAttachments, EditMessage, Message, MessageId, User,
};
use std::{sync::OnceLock, time::Duration};

struct Visual {
    png: Vec<u8>,
    alt: String,
}

fn safe_avatar_url(raw: &str) -> Option<reqwest::Url> {
    let mut url = reqwest::Url::parse(raw).ok()?;
    if url.scheme() != "https"
        || url.host_str() != Some("cdn.discordapp.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !(url.path().starts_with("/avatars/") || url.path().starts_with("/embed/avatars/"))
    {
        return None;
    }
    url.set_query(Some("size=128"));
    Some(url)
}

async fn avatar(user: &User) -> Option<(String, Vec<u8>)> {
    static CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    let client = CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(2))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .ok()
        })
        .as_ref()?;
    let url = safe_avatar_url(&user.static_face())?;
    let mut response = client.get(url).send().await.ok()?.error_for_status().ok()?;
    let mime = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)?
        .to_str()
        .ok()?
        .split(';')
        .next()?
        .trim()
        .to_string();
    if !matches!(mime.as_str(), "image/png" | "image/jpeg" | "image/webp")
        || response
            .content_length()
            .is_some_and(|length| length > 256 * 1024)
    {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if bytes.len() + chunk.len() > 256 * 1024 {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    Some((mime, bytes))
}

fn embed(link: &str) -> CreateEmbed {
    CreateEmbed::new()
        .colour(0xFFC56B)
        .url(link)
        .image("attachment://starboard.png")
}

fn components(link: &str) -> Vec<CreateActionRow> {
    vec![CreateActionRow::Buttons(vec![
        CreateButton::new_link(link).label("View original message"),
    ])]
}

fn attachment(visual: &Visual) -> CreateAttachment {
    CreateAttachment::bytes(visual.png.clone(), "starboard.png").description(&visual.alt)
}

fn create(
    content: &str,
    author: serenity::all::UserId,
    link: &str,
    visual: Option<&Visual>,
) -> CreateMessage {
    let message = CreateMessage::new()
        .content(content)
        .allowed_mentions(super::starboard_allowed_mentions(author));
    if let Some(visual) = visual {
        message
            .embed(embed(link))
            .add_file(attachment(visual))
            .components(components(link))
    } else {
        message
    }
}

fn edit(
    content: &str,
    author: serenity::all::UserId,
    link: &str,
    visual: Option<&Visual>,
) -> EditMessage {
    let message = EditMessage::new()
        .content(content)
        .allowed_mentions(super::starboard_allowed_mentions(author))
        .attachments(EditAttachments::new());
    if let Some(visual) = visual {
        message
            .embeds(vec![embed(link)])
            .new_attachment(attachment(visual))
            .components(components(link))
    } else {
        message.embeds(vec![]).components(vec![])
    }
}

fn definite_rejection(error: &serenity::Error) -> bool {
    // Never retry a possibly accepted send after a timeout or unknown network error.
    matches!(error, serenity::Error::Http(serenity::http::HttpError::UnsuccessfulRequest(response))
        if response.status_code.is_client_error() && response.status_code.as_u16() != 429)
}

fn alt_text(author: &str, message: &str) -> String {
    let mut units = 0;
    format!("{author}: {message}")
        .chars()
        .take_while(|character| {
            units += character.len_utf16();
            units <= 1_000
        })
        .collect()
}

pub(super) async fn publish(
    ctx: &Context,
    board: ChannelId,
    existing: Option<MessageId>,
    original: &Message,
    count: i64,
    link: &str,
    content: &str,
) -> serenity::Result<Message> {
    let author = original
        .member
        .as_ref()
        .and_then(|member| member.nick.as_ref())
        .or(original.author.global_name.as_ref())
        .unwrap_or(&original.author.name)
        .clone();
    let channel = original
        .channel_id
        .to_channel(&ctx.http)
        .await
        .ok()
        .and_then(|channel| channel.guild())
        .map(|channel| channel.name)
        .unwrap_or_else(|| original.channel_id.to_string());
    let message = original.content.clone();
    let avatar = avatar(&original.author).await;
    let alt = alt_text(&author, &message);
    let visual = tokio::task::spawn_blocking(move || {
        super::starboard_card::render(&super::starboard_card::Card {
            author: &author,
            message: &message,
            channel: &channel,
            stars: count,
            avatar: avatar
                .as_ref()
                .map(|(mime, bytes)| (mime.as_str(), bytes.as_slice())),
        })
        .map(|png| Visual { png, alt })
    })
    .await
    .ok()
    .flatten();
    // Retain the bounded text, attribution and attachment links for accessibility.
    let result = if let Some(id) = existing {
        board
            .edit_message(
                &ctx.http,
                id,
                edit(content, original.author.id, link, visual.as_ref()),
            )
            .await
    } else {
        board
            .send_message(
                &ctx.http,
                create(content, original.author.id, link, visual.as_ref()),
            )
            .await
    };
    match result {
        Err(error) if visual.is_some() && definite_rejection(&error) => {
            tracing::warn!("Starboard card rejected; attempting text-only fallback");
            if let Some(id) = existing {
                board
                    .edit_message(&ctx.http, id, edit(content, original.author.id, link, None))
                    .await
            } else {
                board
                    .send_message(&ctx.http, create(content, original.author.id, link, None))
                    .await
            }
        }
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serenity::all::UserId;
    const LINK: &str = "https://discord.com/channels/1/2/3";

    #[test]
    fn unicode_alt_text_fits_discord_limit() {
        let alt = alt_text("Rexy", &"😀".repeat(2_000));
        assert!(alt.encode_utf16().count() <= 1_000);
        assert!(alt.starts_with("Rexy: "));
    }

    #[test]
    fn avatar_urls_are_restricted_to_static_discord_cdn() {
        assert_eq!(
            safe_avatar_url("https://cdn.discordapp.com/avatars/1/a.png?size=4096")
                .unwrap()
                .query(),
            Some("size=128")
        );
        for url in [
            "http://cdn.discordapp.com/avatars/a.png",
            "https://localhost/avatars/a.png",
            "https://cdn.discordapp.com@localhost/avatars/a.png",
            "https://cdn.discordapp.com/attachments/a.png",
            "https://cdn.discordapp.com:444/avatars/a.png",
        ] {
            assert!(safe_avatar_url(url).is_none());
        }
    }

    #[test]
    fn create_and_edit_use_one_card_and_original_link() {
        let visual = Visual {
            png: vec![1, 2],
            alt: "Rexy: batata".into(),
        };
        let create =
            serde_json::to_value(create("safe text", UserId::new(1), LINK, Some(&visual))).unwrap();
        let edit =
            serde_json::to_value(edit("safe text", UserId::new(1), LINK, Some(&visual))).unwrap();
        for packet in [create, edit] {
            assert_eq!(
                packet["embeds"][0]["image"]["url"],
                "attachment://starboard.png"
            );
            assert_eq!(packet["components"][0]["components"][0]["url"], LINK);
            assert_eq!(packet["attachments"].as_array().unwrap().len(), 1);
            assert_eq!(packet["allowed_mentions"]["users"][0], "1");
            assert_eq!(packet["attachments"][0]["description"], "Rexy: batata");
        }
    }

    #[test]
    fn text_fallback_removes_previous_card_without_losing_original_content() {
        let packet =
            serde_json::to_value(edit("@everyone safe text", UserId::new(1), LINK, None)).unwrap();
        assert_eq!(packet["content"], "@everyone safe text");
        for key in ["attachments", "embeds", "components"] {
            assert!(packet[key].as_array().unwrap().is_empty());
        }
        assert!(
            packet["allowed_mentions"]["parse"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
}
