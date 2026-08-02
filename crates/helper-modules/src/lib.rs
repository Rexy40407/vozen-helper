//! Module registry for Core, Studio, Security, Support, Events, Community,
//! Automate and Insights. Feature handlers are added behind these boundaries.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use helper_contracts::{EntitlementSnapshot, Plan};
use helper_core::Capability;
use helper_store::Store;
use hmac::{Hmac, Mac};
use quick_xml::{Reader, escape::unescape, events::Event};
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{sync::Mutex as AsyncMutex, task::JoinHandle};
use url::Url;

/// Small, server-side client for the official YouTube Data API.
///
/// The API key is read only from the process environment. It is never part
/// of a response, log line, or panel payload. The client is intentionally
/// limited to public channel metadata and a channel's official uploads
/// playlist. Alert polling and Discord delivery use the same boundary.
#[derive(Clone)]
pub struct YouTubeClient {
    api_key: Arc<str>,
    http: Client,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct YouTubeChannel {
    pub id: String,
    pub title: String,
    pub description: String,
    pub custom_url: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct YouTubeVideo {
    pub id: String,
    pub title: String,
    pub description: String,
    pub url: String,
    pub published_at: String,
    pub channel_title: String,
}

/// Server-side client for public RSS/Atom feeds. URLs are structurally
/// validated, resolved away from private/link-local addresses and fetched
/// without following redirects so this cannot become an SSRF proxy.
#[derive(Clone)]
pub struct RssClient {
    http: Client,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RssItem {
    pub id: String,
    pub title: String,
    pub description: String,
    pub url: String,
    pub published_at: String,
    pub feed_title: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RssFeed {
    pub url: String,
    pub title: String,
    pub latest: Option<RssItem>,
}

/// Official Twitch Helix/EventSub client. Credentials and the EventSub
/// signing secret are always read from the server environment.
#[derive(Clone)]
pub struct TwitchClient {
    client_id: Arc<str>,
    client_secret: Arc<str>,
    callback_url: Arc<str>,
    eventsub_secret: Arc<str>,
    http: Client,
    token: Arc<AsyncMutex<Option<TwitchToken>>>,
}

#[derive(Clone)]
struct TwitchToken {
    access_token: String,
    expires_at: Instant,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TwitchUser {
    pub id: String,
    pub login: String,
    pub display_name: String,
}

#[derive(Debug, Deserialize)]
struct TwitchTokenResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct TwitchUsersResponse {
    #[serde(default)]
    data: Vec<TwitchUserPayload>,
}

#[derive(Debug, Deserialize)]
struct TwitchUserPayload {
    id: String,
    login: String,
    display_name: String,
}

#[derive(Debug, Deserialize)]
struct TwitchEventSubResponse {
    #[serde(default)]
    data: Vec<TwitchEventSubPayload>,
}

#[derive(Debug, Deserialize)]
struct TwitchEventSubPayload {
    status: String,
    #[serde(default)]
    condition: HashMap<String, String>,
}

impl TwitchClient {
    pub fn from_env() -> Option<Self> {
        let callback_url = std::env::var("TWITCH_EVENTSUB_CALLBACK_URL")
            .ok()
            .or_else(|| {
                std::env::var("TWITCH_CALLBACK_URL")
                    .ok()
                    .filter(|value| value.contains("/eventsub"))
            })
            .unwrap_or_else(|| "https://api.vozen.org/rust/api/providers/twitch/eventsub".into());
        Self::new(
            std::env::var("TWITCH_CLIENT_ID").ok()?,
            std::env::var("TWITCH_CLIENT_SECRET").ok()?,
            callback_url,
            std::env::var("TWITCH_EVENTSUB_SECRET").ok()?,
        )
    }

    pub fn new(
        client_id: impl Into<String>,
        client_secret: impl Into<String>,
        callback_url: impl Into<String>,
        eventsub_secret: impl Into<String>,
    ) -> Option<Self> {
        let client_id = client_id.into().trim().to_string();
        let client_secret = client_secret.into().trim().to_string();
        let callback_url = callback_url.into().trim().to_string();
        let eventsub_secret = eventsub_secret.into().trim().to_string();
        if client_id.is_empty()
            || client_secret.is_empty()
            || !(10..=100).contains(&eventsub_secret.len())
            || !callback_url.starts_with("https://")
        {
            return None;
        }
        Some(Self {
            client_id: Arc::from(client_id),
            client_secret: Arc::from(client_secret),
            callback_url: Arc::from(callback_url),
            eventsub_secret: Arc::from(eventsub_secret),
            http: Client::builder()
                .timeout(Duration::from_secs(10))
                .user_agent("Vozen-Helper/1.0 (+https://vozen.org)")
                .build()
                .expect("valid Twitch HTTP client"),
            token: Arc::new(AsyncMutex::new(None)),
        })
    }

    pub fn is_configured(&self) -> bool {
        !self.client_id.is_empty() && !self.client_secret.is_empty()
    }

    pub fn callback_url(&self) -> &str {
        &self.callback_url
    }

    pub fn eventsub_secret(&self) -> &str {
        &self.eventsub_secret
    }

    pub async fn user(&self, login: &str) -> anyhow::Result<Option<TwitchUser>> {
        let login = login.trim().trim_start_matches('@').to_ascii_lowercase();
        if login.is_empty()
            || login.len() > 25
            || !login
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        {
            anyhow::bail!("invalid_twitch_login");
        }
        let token = self.app_token().await?;
        let response = self
            .http
            .get("https://api.twitch.tv/helix/users")
            .query(&[("login", login.as_str())])
            .header("Client-Id", self.client_id.as_ref())
            .bearer_auth(token)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("twitch_api_error:{status}");
        }
        let payload: TwitchUsersResponse = response.json().await?;
        Ok(payload.data.into_iter().next().map(|value| TwitchUser {
            id: value.id,
            login: value.login,
            display_name: value.display_name,
        }))
    }

    /// Ensures a single stream.online EventSub subscription exists for the
    /// broadcaster. Twitch may resend notifications, so the webhook handler
    /// deduplicates by EventSub message id before dispatching to Discord.
    pub async fn ensure_stream_online_subscription(&self, user_id: &str) -> anyhow::Result<()> {
        let token = self.app_token().await?;
        let existing = self
            .http
            .get("https://api.twitch.tv/helix/eventsub/subscriptions")
            .query(&[("type", "stream.online"), ("user_id", user_id)])
            .header("Client-Id", self.client_id.as_ref())
            .bearer_auth(&token)
            .send()
            .await?;
        if !existing.status().is_success() {
            anyhow::bail!("twitch_api_error:{}", existing.status());
        }
        let payload: TwitchEventSubResponse = existing.json().await?;
        if payload.data.iter().any(|item| {
            item.status == "enabled"
                && item
                    .condition
                    .get("broadcaster_user_id")
                    .is_some_and(|value| value == user_id)
        }) {
            return Ok(());
        }
        let response = self
            .http
            .post("https://api.twitch.tv/helix/eventsub/subscriptions")
            .header("Client-Id", self.client_id.as_ref())
            .bearer_auth(token)
            .json(&serde_json::json!({
                "type": "stream.online",
                "version": "1",
                "condition": { "broadcaster_user_id": user_id },
                "transport": {
                    "method": "webhook",
                    "callback": self.callback_url.as_ref(),
                    "secret": self.eventsub_secret.as_ref()
                }
            }))
            .send()
            .await?;
        if response.status().is_success() {
            return Ok(());
        }
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if status == reqwest::StatusCode::CONFLICT || body.contains("already exists") {
            return Ok(());
        }
        anyhow::bail!("twitch_api_error:{status}");
    }

    async fn app_token(&self) -> anyhow::Result<String> {
        let mut cached = self.token.lock().await;
        if let Some(value) = cached.as_ref()
            && value.expires_at > Instant::now() + Duration::from_secs(60)
        {
            return Ok(value.access_token.clone());
        }
        let response = self
            .http
            .post("https://id.twitch.tv/oauth2/token")
            .form(&[
                ("client_id", self.client_id.as_ref()),
                ("client_secret", self.client_secret.as_ref()),
                ("grant_type", "client_credentials"),
            ])
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("twitch_auth_error:{}", response.status());
        }
        let value: TwitchTokenResponse = response.json().await?;
        let token = TwitchToken {
            access_token: value.access_token.clone(),
            expires_at: Instant::now() + Duration::from_secs(value.expires_in),
        };
        *cached = Some(token);
        Ok(value.access_token)
    }
}

impl RssClient {
    pub fn new() -> Self {
        Self {
            http: Client::builder()
                .timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .user_agent("Vozen-Helper/1.0 (+https://vozen.org)")
                .build()
                .expect("valid RSS HTTP client"),
        }
    }

    pub async fn fetch(&self, raw_url: &str) -> anyhow::Result<Option<RssFeed>> {
        let url = validate_feed_url(raw_url).await?;
        let response = self.http.get(url.clone()).send().await?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("rss_http_error:{status}");
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_FEED_BYTES as u64)
        {
            anyhow::bail!("rss_feed_too_large");
        }
        let mut response = response;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            if bytes.len().saturating_add(chunk.len()) > MAX_FEED_BYTES {
                anyhow::bail!("rss_feed_too_large");
            }
            bytes.extend_from_slice(&chunk);
        }
        let feed = parse_feed(&bytes)?;
        Ok(feed.map(|mut value| {
            value.url = url.to_string();
            value
        }))
    }
}

impl Default for RssClient {
    fn default() -> Self {
        Self::new()
    }
}

const MAX_FEED_BYTES: usize = 1_048_576;

async fn validate_feed_url(raw_url: &str) -> anyhow::Result<Url> {
    let url = Url::parse(raw_url.trim()).map_err(|_| anyhow::anyhow!("invalid_rss_url"))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        anyhow::bail!("invalid_rss_url");
    }
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
    {
        anyhow::bail!("rss_private_host");
    }
    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow::anyhow!("invalid_rss_url"))?;
    let addresses = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|_| anyhow::anyhow!("rss_host_unresolvable"))?
        .collect::<Vec<_>>();
    if addresses.is_empty()
        || addresses
            .iter()
            .any(|address| private_or_local(address.ip()))
    {
        anyhow::bail!("rss_private_host");
    }
    Ok(url)
}

fn private_or_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(value) => {
            value.is_private()
                || value.is_loopback()
                || value.is_link_local()
                || value.is_unspecified()
                || value.is_broadcast()
                || value.octets()[0] == 0
        }
        IpAddr::V6(value) => {
            value.is_loopback()
                || value.is_unique_local()
                || value.is_unicast_link_local()
                || value.is_unspecified()
                || value.is_multicast()
        }
    }
}

#[derive(Default)]
struct RawRssItem {
    id: String,
    title: String,
    description: String,
    url: String,
    published_at: String,
}

fn parse_feed(bytes: &[u8]) -> anyhow::Result<Option<RssFeed>> {
    let mut reader = Reader::from_reader(bytes);
    // Keep whitespace around `Event::GeneralRef` entities (for example
    // `Hello &amp; welcome`), then trim each field once the item is complete.
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut feed_title = String::new();
    let mut current_tag = String::new();
    let mut current_item: Option<RawRssItem> = None;
    let mut first_item: Option<RawRssItem> = None;
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(event)) => {
                let name =
                    String::from_utf8_lossy(event.local_name().as_ref()).to_ascii_lowercase();
                if name == "item" || name == "entry" {
                    if first_item.is_none() {
                        current_item = Some(RawRssItem::default());
                    }
                    current_tag.clear();
                } else {
                    if let Some(item) = current_item.as_mut() {
                        if name == "link" {
                            for attribute in event.attributes().flatten() {
                                if attribute.key.as_ref() == b"href"
                                    && let Ok(value) = attribute.decoded_and_normalized_value(
                                        quick_xml::XmlVersion::Implicit1_0,
                                        reader.decoder(),
                                    )
                                {
                                    item.url = value.into_owned();
                                }
                            }
                        }
                        current_tag = name;
                    } else {
                        current_tag = name;
                    }
                }
            }
            Ok(Event::Empty(event)) => {
                if current_item.is_some()
                    && event.local_name().as_ref().eq_ignore_ascii_case(b"link")
                {
                    for attribute in event.attributes().flatten() {
                        if attribute.key.as_ref() == b"href"
                            && let Ok(value) = attribute.decoded_and_normalized_value(
                                quick_xml::XmlVersion::Implicit1_0,
                                reader.decoder(),
                            )
                            && let Some(item) = current_item.as_mut()
                        {
                            item.url = value.into_owned();
                        }
                    }
                }
            }
            Ok(Event::Text(event)) => assign_feed_text(
                &mut current_item,
                &mut feed_title,
                &current_tag,
                event.as_ref(),
            ),
            Ok(Event::CData(event)) => assign_feed_text(
                &mut current_item,
                &mut feed_title,
                &current_tag,
                event.as_ref(),
            ),
            Ok(Event::GeneralRef(event)) => {
                assign_feed_reference(&mut current_item, &mut feed_title, &current_tag, &event)
            }
            Ok(Event::End(event)) => {
                let name =
                    String::from_utf8_lossy(event.local_name().as_ref()).to_ascii_lowercase();
                if name == "item" || name == "entry" {
                    if let Some(mut item) = current_item.take() {
                        item.title = item.title.trim().to_string();
                        item.description = item.description.trim().to_string();
                        item.url = item.url.trim().to_string();
                        item.id = item.id.trim().to_string();
                        item.published_at = item.published_at.trim().to_string();
                        if item.id.is_empty() {
                            item.id = item.url.clone();
                        }
                        if item.id.is_empty() {
                            let mut digest = Sha256::new();
                            digest.update(format!(
                                "{}|{}|{}",
                                item.title, item.published_at, item.description
                            ));
                            item.id = hex::encode(digest.finalize());
                        }
                        if first_item.is_none() {
                            first_item = Some(item);
                        }
                    }
                    current_tag.clear();
                }
            }
            Ok(Event::Eof) => break,
            Err(error) => return Err(anyhow::anyhow!("rss_xml_error:{error}")),
            _ => {}
        }
        buffer.clear();
    }
    let Some(item) = first_item else {
        return Ok(None);
    };
    let item = RssItem {
        id: item.id,
        title: item.title,
        description: item.description,
        url: item.url,
        published_at: item.published_at,
        feed_title: feed_title.clone(),
    };
    Ok(Some(RssFeed {
        url: String::new(),
        title: feed_title,
        latest: Some(item),
    }))
}

fn assign_feed_text(
    current_item: &mut Option<RawRssItem>,
    feed_title: &mut String,
    current_tag: &str,
    bytes: &[u8],
) {
    let text = unescape(&String::from_utf8_lossy(bytes))
        .map(|value| value.into_owned())
        .unwrap_or_else(|_| String::from_utf8_lossy(bytes).into_owned());
    append_feed_text(current_item, feed_title, current_tag, &text);
}

fn assign_feed_reference(
    current_item: &mut Option<RawRssItem>,
    feed_title: &mut String,
    current_tag: &str,
    reference: &quick_xml::events::BytesRef<'_>,
) {
    let name = reference
        .decode()
        .map(|value| value.into_owned())
        .unwrap_or_default();
    let text = match name.as_str() {
        "amp" => "&".to_string(),
        "lt" => "<".to_string(),
        "gt" => ">".to_string(),
        "quot" => "\"".to_string(),
        "apos" => "'".to_string(),
        _ => reference
            .resolve_char_ref()
            .ok()
            .flatten()
            .map(|value| value.to_string())
            .unwrap_or_else(|| format!("&{name};")),
    };
    append_feed_text(current_item, feed_title, current_tag, &text);
}

fn append_feed_text(
    current_item: &mut Option<RawRssItem>,
    feed_title: &mut String,
    current_tag: &str,
    text: &str,
) {
    if let Some(item) = current_item.as_mut() {
        match current_tag {
            "title" => item.title.push_str(text),
            "description" | "summary" | "content" => item.description.push_str(text),
            "link" if item.url.is_empty() => item.url.push_str(text.trim()),
            "guid" | "id" => item.id.push_str(text),
            "pubdate" | "published" | "updated" | "date" => item.published_at.push_str(text),
            _ => {}
        }
    } else if current_tag == "title" && feed_title.is_empty() {
        *feed_title = text.to_string();
    }
}

#[derive(Debug, Deserialize)]
struct YouTubeChannelsResponse {
    #[serde(default)]
    items: Vec<YouTubeChannelItem>,
}

#[derive(Debug, Deserialize)]
struct YouTubeChannelItem {
    id: String,
    snippet: YouTubeChannelSnippet,
}

#[derive(Debug, Deserialize)]
struct YouTubeChannelDetailsResponse {
    #[serde(default)]
    items: Vec<YouTubeChannelDetailsItem>,
}

#[derive(Debug, Deserialize)]
struct YouTubeChannelDetailsItem {
    #[serde(rename = "contentDetails", default)]
    content_details: YouTubeChannelContentDetails,
}

#[derive(Debug, Default, Deserialize)]
struct YouTubeChannelContentDetails {
    #[serde(rename = "relatedPlaylists", default)]
    related_playlists: YouTubeRelatedPlaylists,
}

#[derive(Debug, Default, Deserialize)]
struct YouTubeRelatedPlaylists {
    #[serde(default)]
    uploads: Option<String>,
}

impl YouTubeChannelDetailsItem {
    fn content_details_uploads(self) -> Option<String> {
        self.content_details.related_playlists.uploads
    }
}

#[derive(Debug, Deserialize)]
struct YouTubeChannelSnippet {
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(rename = "customUrl", default)]
    custom_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct YouTubePlaylistItemsResponse {
    #[serde(default)]
    items: Vec<YouTubePlaylistItem>,
}

#[derive(Debug, Deserialize)]
struct YouTubePlaylistItem {
    snippet: YouTubePlaylistSnippet,
    #[serde(rename = "contentDetails", default)]
    content_details: YouTubePlaylistContentDetails,
}

#[derive(Debug, Default, Deserialize)]
struct YouTubePlaylistContentDetails {
    #[serde(rename = "videoId", default)]
    video_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct YouTubePlaylistSnippet {
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(rename = "publishedAt", default)]
    published_at: String,
    #[serde(rename = "channelTitle", default)]
    channel_title: String,
}

impl YouTubeClient {
    pub fn from_env() -> Option<Self> {
        let key = std::env::var("YOUTUBE_API_KEY").ok()?;
        Self::new(key)
    }

    pub fn new(api_key: impl Into<String>) -> Option<Self> {
        let key = api_key.into().trim().to_string();
        if key.trim().is_empty() {
            return None;
        }
        Some(Self {
            api_key: Arc::<str>::from(key),
            http: Client::builder()
                .timeout(Duration::from_secs(8))
                .user_agent("Vozen-Helper/1.0 (+https://vozen.org)")
                .build()
                .expect("valid YouTube HTTP client"),
        })
    }

    pub fn is_configured(&self) -> bool {
        !self.api_key.is_empty()
    }

    /// Resolve one public channel by its stable YouTube channel ID.
    ///
    /// URLs, handles and arbitrary search terms are deliberately rejected so
    /// this endpoint cannot become an unrestricted proxy or SSRF primitive.
    pub async fn channel(&self, channel_id: &str) -> anyhow::Result<Option<YouTubeChannel>> {
        if !valid_channel_id(channel_id) {
            anyhow::bail!("invalid_youtube_channel_id");
        }
        let response = self
            .http
            .get("https://www.googleapis.com/youtube/v3/channels")
            .query(&[
                ("part", "snippet"),
                ("id", channel_id),
                ("key", self.api_key.as_ref()),
            ])
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            // Do not include Google's body: it can contain request details
            // and should never be reflected through the panel API.
            anyhow::bail!("youtube_api_error:{status}");
        }
        let payload = response.json::<YouTubeChannelsResponse>().await?;
        Ok(payload.items.into_iter().next().map(|item| YouTubeChannel {
            id: item.id,
            title: item.snippet.title,
            description: item.snippet.description,
            custom_url: item.snippet.custom_url,
        }))
    }

    /// Returns the newest public video through the channel's official uploads
    /// playlist. This costs a fraction of the quota used by `search.list` and
    /// still returns only one bounded result.
    pub async fn latest_video(&self, channel_id: &str) -> anyhow::Result<Option<YouTubeVideo>> {
        if !valid_channel_id(channel_id) {
            anyhow::bail!("invalid_youtube_channel_id");
        }
        let response = self
            .http
            .get("https://www.googleapis.com/youtube/v3/channels")
            .query(&[
                ("part", "contentDetails"),
                ("id", channel_id),
                ("key", self.api_key.as_ref()),
            ])
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("youtube_api_error:{status}");
        }
        let uploads = response
            .json::<YouTubeChannelDetailsResponse>()
            .await?
            .items
            .into_iter()
            .next()
            .and_then(|item| item.content_details_uploads());
        let Some(uploads_playlist_id) = uploads else {
            return Ok(None);
        };
        let response = self
            .http
            .get("https://www.googleapis.com/youtube/v3/playlistItems")
            .query(&[
                ("part", "snippet,contentDetails"),
                ("playlistId", uploads_playlist_id.as_str()),
                ("maxResults", "1"),
                ("key", self.api_key.as_ref()),
            ])
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("youtube_api_error:{status}");
        }
        let payload = response.json::<YouTubePlaylistItemsResponse>().await?;
        Ok(payload.items.into_iter().find_map(|item| {
            let id = item.content_details.video_id?;
            Some(YouTubeVideo {
                url: format!("https://www.youtube.com/watch?v={id}"),
                id,
                title: item.snippet.title,
                description: item.snippet.description,
                published_at: item.snippet.published_at,
                channel_title: item.snippet.channel_title,
            })
        }))
    }
}

fn valid_channel_id(value: &str) -> bool {
    (3..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_payload_uses_official_camel_case_fields() {
        let details: YouTubeChannelDetailsResponse = serde_json::from_value(serde_json::json!({
            "items": [{
                "contentDetails": {"relatedPlaylists": {"uploads": "UU123"}}
            }]
        }))
        .expect("channel details payload should decode");
        assert_eq!(
            details
                .items
                .into_iter()
                .next()
                .unwrap()
                .content_details_uploads(),
            Some("UU123".to_string())
        );

        let playlist: YouTubePlaylistItemsResponse = serde_json::from_value(serde_json::json!({
            "items": [{
                "snippet": {
                    "title": "Video",
                    "description": "Description",
                    "publishedAt": "2026-08-02T12:00:00Z",
                    "channelTitle": "Vozen"
                },
                "contentDetails": {"videoId": "abc123"}
            }]
        }))
        .expect("playlist item payload should decode");
        let item = playlist.items.into_iter().next().unwrap();
        assert_eq!(item.content_details.video_id.as_deref(), Some("abc123"));
        assert_eq!(item.snippet.channel_title, "Vozen");
        assert_eq!(item.snippet.published_at, "2026-08-02T12:00:00Z");
    }

    #[test]
    fn youtube_channel_id_validation_rejects_urls() {
        assert!(valid_channel_id("UC_abc-123"));
        assert!(!valid_channel_id("https://youtube.com/@vozen"));
        assert!(!valid_channel_id(""));
    }

    #[test]
    fn rss_parser_supports_rss_and_atom() {
        let rss = br#"<?xml version="1.0"?><rss><channel><title>Vozen News</title><item><guid>post-1</guid><title>First post</title><description>Hello &amp; welcome</description><link>https://example.com/post-1</link><pubDate>2026-08-02T12:00:00Z</pubDate></item></channel></rss>"#;
        let parsed = parse_feed(rss)
            .expect("RSS should parse")
            .expect("RSS has an item");
        assert_eq!(parsed.title, "Vozen News");
        let item = parsed.latest.expect("RSS latest item");
        assert_eq!(item.id, "post-1");
        assert_eq!(item.title, "First post");
        assert_eq!(item.description, "Hello & welcome");
        assert_eq!(item.url, "https://example.com/post-1");

        let atom = br#"<feed xmlns="http://www.w3.org/2005/Atom"><title>Atom Updates</title><entry><id>tag:example.com,2026:2</id><title>Atom entry</title><summary>Summary</summary><link href="https://example.com/atom-2"/><updated>2026-08-02T13:00:00Z</updated></entry></feed>"#;
        let parsed = parse_feed(atom)
            .expect("Atom should parse")
            .expect("Atom has an entry");
        assert_eq!(parsed.title, "Atom Updates");
        let item = parsed.latest.expect("Atom latest item");
        assert_eq!(item.id, "tag:example.com,2026:2");
        assert_eq!(item.url, "https://example.com/atom-2");
    }

    #[test]
    fn rss_private_address_check_rejects_local_ranges() {
        assert!(private_or_local("127.0.0.1".parse().unwrap()));
        assert!(private_or_local("10.0.0.1".parse().unwrap()));
        assert!(private_or_local("192.168.1.1".parse().unwrap()));
        assert!(private_or_local("::1".parse().unwrap()));
        assert!(!private_or_local("1.1.1.1".parse().unwrap()));
    }
}

pub const MODULES: &[Capability] = &[
    Capability::Core,
    Capability::Studio,
    Capability::Security,
    Capability::Support,
    Capability::Events,
    Capability::Community,
    Capability::Automate,
    Capability::Insights,
];

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct EntitlementClient {
    endpoint: String,
    secret: String,
    http: Client,
    cache: Arc<Mutex<HashMap<String, EntitlementSnapshot>>>,
}

#[derive(Debug, Deserialize)]
struct CentralResponse {
    subject_id: String,
    plan: String,
    guild_limit: u16,
    active: bool,
    expires_at_ms: Option<i64>,
    version: i64,
}

impl EntitlementClient {
    pub fn new(endpoint: Option<String>, secret: Option<String>) -> Option<Self> {
        Some(Self {
            endpoint: endpoint?,
            secret: secret?,
            http: Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .expect("valid entitlement HTTP client"),
            cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn resolve(
        &self,
        subject_id: &str,
        guild_id: Option<&str>,
    ) -> anyhow::Result<EntitlementSnapshot> {
        if let Some(snapshot) = self.cached(subject_id, guild_id)
            && snapshot.active
            && snapshot
                .expires_at
                .is_none_or(|expires_at| expires_at > Utc::now())
        {
            return Ok(snapshot);
        }
        let body = serde_json::json!({"subject_id": subject_id, "guild_id": guild_id});
        let bytes = serde_json::to_vec(&body)?;
        let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let nonce = uuid::Uuid::new_v4().to_string();
        let body_hash = URL_SAFE_NO_PAD.encode(Sha256::digest(&bytes));
        let mut mac = HmacSha256::new_from_slice(self.secret.as_bytes())?;
        mac.update(format!("{timestamp}\n{nonce}\n{body_hash}").as_bytes());
        let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
        let response = self
            .http
            .post(&self.endpoint)
            .header("x-vozen-timestamp", timestamp.to_string())
            .header("x-vozen-nonce", &nonce)
            .header("x-vozen-signature", format!("v1={signature}"))
            .json(&body)
            .send()
            .await?;
        if !response.status().is_success() {
            anyhow::bail!("central entitlement service returned {}", response.status());
        }
        let central: CentralResponse = response.json().await?;
        let plan = match central.plan.as_str() {
            "plus" => Plan::Plus,
            "premium" => Plan::Premium {
                guild_limit: central.guild_limit,
            },
            _ => Plan::Free,
        };
        let snapshot = EntitlementSnapshot {
            product: "vozen-helper".into(),
            subject_id: central.subject_id,
            plan,
            active: central.active,
            version: central.version.try_into().unwrap_or_default(),
            expires_at: central
                .expires_at_ms
                .and_then(chrono::DateTime::from_timestamp_millis),
            fetched_at: Utc::now(),
        };
        self.cache
            .lock()
            .expect("entitlement cache poisoned")
            .insert(Self::cache_key(subject_id, guild_id), snapshot.clone());
        Ok(snapshot)
    }

    fn cache_key(subject_id: &str, guild_id: Option<&str>) -> String {
        format!("{subject_id}:{}", guild_id.unwrap_or("user"))
    }

    pub fn cached(&self, subject_id: &str, guild_id: Option<&str>) -> Option<EntitlementSnapshot> {
        self.cache
            .lock()
            .ok()?
            .get(&Self::cache_key(subject_id, guild_id))
            .cloned()
    }
}

/// Bounded persistent scheduler. It never runs unbounded work on the gateway
/// task: each tick claims at most 100 due rows and removes them only after the
/// dispatch boundary has been reached.
pub fn start_scheduler(store: Store) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        let mut last_retention = Instant::now() - Duration::from_secs(86_401);
        loop {
            interval.tick().await;
            if last_retention.elapsed() >= Duration::from_secs(86_400) {
                match store.prune_retention(chrono::Utc::now().timestamp_millis()) {
                    Ok(summary) => tracing::info!(?summary, "retention sweep completed"),
                    Err(error) => tracing::error!(%error, "retention sweep failed"),
                }
                last_retention = Instant::now();
            }
            match store.due_scheduled_actions(chrono::Utc::now().timestamp_millis(), 100) {
                Ok(actions) => {
                    for (id, guild_id, kind, target_id, _payload) in actions {
                        tracing::info!(%id, %guild_id, %kind, %target_id, "dispatching scheduled helper action");
                        if let Err(error) = store.delete_scheduled_action(id) {
                            tracing::error!(%error, %id, "failed to ack scheduled action");
                        }
                    }
                }
                Err(error) => tracing::error!(%error, "scheduler tick failed"),
            }
        }
    })
}
