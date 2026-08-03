//! Discord gateway boundary. Handlers stay thin and delegate to core/modules.

use anyhow::Result;
use chrono::{Datelike, Utc};
use helper_contracts::{AntiSpamObservation, AntiSpamPolicy, Plan};
use helper_core::{
    Config, anti_spam_policy_from_json, evaluate_anti_spam, evaluate_scam, quota_limit,
    scam_policy_from_json,
};
use helper_modules::{
    EntitlementClient, RssClient, RssItem, TwitchClient, YouTubeClient, YouTubeVideo,
};
use helper_store::{
    RssSubscriptionRecord, Store, TwitchSubscriptionRecord, YouTubeSubscriptionRecord,
};
use rand::seq::SliceRandom;
use serenity::{
    all::{
        ButtonStyle, ChannelId, ChannelType, Client, Command, CommandDataOptionValue,
        CommandInteraction, Context, CreateActionRow, CreateAttachment, CreateButton,
        CreateChannel, CreateCommand, CreateCommandOption, CreateEmbed, CreateInteractionResponse,
        CreateInteractionResponseMessage, CreateMessage, EditChannel, EventHandler, GatewayIntents,
        Interaction, MessageUpdateEvent, PermissionOverwrite, PermissionOverwriteType, Permissions,
        Ready, RoleId,
    },
    async_trait,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tracing::{info, warn};

mod rank_card;

/// A small side-effect boundary used by policy tests. Production handlers
/// still use Serenity directly, while the fake makes permission failures and
/// emitted actions deterministic without connecting to Discord.
pub mod adapter {
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Effect {
        Timeout {
            guild_id: String,
            member_id: String,
            seconds: u64,
            reason: String,
        },
        Log {
            channel_id: String,
            content: String,
        },
        Reply {
            channel_id: String,
            content: String,
        },
    }

    pub trait DiscordAdapter {
        fn timeout_member(
            &mut self,
            guild_id: &str,
            member_id: &str,
            seconds: u64,
            reason: &str,
        ) -> Result<(), String>;
        fn log(&mut self, channel_id: &str, content: &str) -> Result<(), String>;
        fn reply(&mut self, channel_id: &str, content: &str) -> Result<(), String>;
    }

    #[derive(Debug, Default, Clone)]
    pub struct FakeDiscordAdapter {
        effects: Vec<Effect>,
        fail_next: bool,
    }

    impl FakeDiscordAdapter {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn effects(&self) -> &[Effect] {
            &self.effects
        }

        pub fn fail_next(&mut self) {
            self.fail_next = true;
        }

        fn check(&mut self) -> Result<(), String> {
            if self.fail_next {
                self.fail_next = false;
                Err("discord_permission_denied".into())
            } else {
                Ok(())
            }
        }
    }

    impl DiscordAdapter for FakeDiscordAdapter {
        fn timeout_member(
            &mut self,
            guild_id: &str,
            member_id: &str,
            seconds: u64,
            reason: &str,
        ) -> Result<(), String> {
            self.check()?;
            self.effects.push(Effect::Timeout {
                guild_id: guild_id.into(),
                member_id: member_id.into(),
                seconds,
                reason: reason.into(),
            });
            Ok(())
        }

        fn log(&mut self, channel_id: &str, content: &str) -> Result<(), String> {
            self.check()?;
            self.effects.push(Effect::Log {
                channel_id: channel_id.into(),
                content: content.into(),
            });
            Ok(())
        }

        fn reply(&mut self, channel_id: &str, content: &str) -> Result<(), String> {
            self.check()?;
            self.effects.push(Effect::Reply {
                channel_id: channel_id.into(),
                content: content.into(),
            });
            Ok(())
        }
    }
}

type DuplicateMessageCache = Arc<Mutex<HashMap<String, VecDeque<(Instant, String)>>>>;

#[derive(Clone)]
struct Handler {
    store: Store,
    spam: Arc<Mutex<HashMap<String, VecDeque<Instant>>>>,
    duplicate_messages: DuplicateMessageCache,
    spam_action_at: Arc<Mutex<HashMap<String, Instant>>>,
    joins: Arc<Mutex<HashMap<String, VecDeque<Instant>>>>,
    nuke_events: Arc<Mutex<HashMap<String, VecDeque<Instant>>>>,
    xp_awarded_at: Arc<Mutex<HashMap<String, Instant>>>,
    scheduler_started: Arc<AtomicBool>,
    entitlements: Option<EntitlementClient>,
    youtube: Option<YouTubeClient>,
    rss: Option<RssClient>,
    twitch: Option<TwitchClient>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!(user = %ready.user.name, "helper gateway ready");
        for guild in &ready.guilds {
            let guild_id = guild.id.to_string();
            if !feature_enabled(&self.store, &guild_id, "management.nickname", None) {
                continue;
            }
            let Some(nickname) = setting_string(&self.store, &guild_id, "identity.nickname") else {
                continue;
            };
            if let Err(error) = guild
                .id
                .edit_member(
                    &ctx.http,
                    ready.user.id,
                    serenity::all::EditMember::new().nickname(nickname.trim().to_owned()),
                )
                .await
            {
                warn!(%guild_id, %error, "failed to apply configured Helper nickname");
            }
        }
        if !self.scheduler_started.swap(true, Ordering::AcqRel) {
            let store = self.store.clone();
            let http = ctx.http.clone();
            tokio::spawn(async move {
                let mut last_birthday_day: Option<(i32, u32, u32)> = None;
                loop {
                    if let Ok(actions) =
                        store.due_scheduled_actions(chrono::Utc::now().timestamp_millis(), 100)
                    {
                        for (id, guild_id, action_type, target_id, payload) in actions {
                            let _ = deliver_scheduled_action(
                                &http,
                                &store,
                                id,
                                &guild_id,
                                &action_type,
                                &target_id,
                                &payload,
                            )
                            .await;
                        }
                    }
                    let birthday_now = chrono::Utc::now();
                    let birthday_day = (
                        birthday_now.year(),
                        birthday_now.month(),
                        birthday_now.day(),
                    );
                    if last_birthday_day != Some(birthday_day) {
                        let _ = deliver_birthday_announcements(&http, &store, birthday_day).await;
                        last_birthday_day = Some(birthday_day);
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            });
            if let Some(youtube) = self.youtube.clone() {
                let store = self.store.clone();
                let http = ctx.http.clone();
                tokio::spawn(async move {
                    run_youtube_worker(http, store, youtube).await;
                });
            }
            if let Some(rss) = self.rss.clone() {
                let store = self.store.clone();
                let http = ctx.http.clone();
                tokio::spawn(async move {
                    run_rss_worker(http, store, rss).await;
                });
            }
            if self.twitch.is_some() {
                let store = self.store.clone();
                let http = ctx.http.clone();
                tokio::spawn(async move {
                    run_twitch_worker(http, store).await;
                });
            }
        }

        let commands = vec![
            CreateCommand::new("ping").description("Check Helper latency"),
            CreateCommand::new("help").description("Show Helper modules"),
            CreateCommand::new("setup")
                .description("Run the guided Helper setup for this server")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Boolean,
                        "security",
                        "Enable Security module",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Boolean,
                        "support",
                        "Enable Support module",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Boolean,
                        "events",
                        "Enable Events module",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Boolean,
                        "community",
                        "Enable Community module",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Boolean,
                        "automate",
                        "Enable Automate module",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Boolean,
                        "insights",
                        "Enable Insights module",
                    )
                    .required(false),
                ),
            CreateCommand::new("modules").description("Show enabled Helper modules"),
            CreateCommand::new("status").description("Show Helper setup and health status"),
            CreateCommand::new("dashboard").description("Open the Helper dashboard"),
            CreateCommand::new("plan").description("Show the active Vozen plan"),
            CreateCommand::new("privacy")
                .description("Request your Helper data or erase voluntary data")
                .add_option(CreateCommandOption::new(
                    serenity::all::CommandOptionType::SubCommand,
                    "data",
                    "Explain how to export your data",
                ))
                .add_option(CreateCommandOption::new(
                    serenity::all::CommandOptionType::SubCommand,
                    "erase",
                    "Explain how to erase voluntary data",
                )),
            CreateCommand::new("permissions").description("Show the Helper Permission Passport"),
            CreateCommand::new("cases").description("List recent moderation cases"),
            CreateCommand::new("modlogs")
                .description("Show a member's moderation history")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::User,
                        "user",
                        "Member",
                    )
                    .required(true),
                ),
            CreateCommand::new("warn")
                .description("Create a moderation warning")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::User,
                        "user",
                        "Member to warn",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "reason",
                        "Reason",
                    )
                    .required(false),
                ),
            CreateCommand::new("violation")
                .description("Record a policy violation")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::User,
                        "user",
                        "Member",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "rule",
                        "Rule or policy",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "details",
                        "Evidence or details",
                    )
                    .required(false),
                ),
            CreateCommand::new("note")
                .description("Add a moderation note")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::User,
                        "user",
                        "Member",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "content",
                        "Note",
                    )
                    .required(true),
                ),
            CreateCommand::new("reason")
                .description("Update a case reason")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "case",
                        "Case number",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "reason",
                        "New reason",
                    )
                    .required(true),
                ),
            CreateCommand::new("kick")
                .description("Kick a member")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::User,
                        "user",
                        "Member to kick",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "reason",
                        "Reason",
                    )
                    .required(false),
                ),
            CreateCommand::new("ban")
                .description("Ban a member")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::User,
                        "user",
                        "Member to ban",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "reason",
                        "Reason",
                    )
                    .required(false),
                ),
            CreateCommand::new("timeout")
                .description("Timeout a member")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::User,
                        "user",
                        "Member to timeout",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "seconds",
                        "Duration in seconds (max 28 days)",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "reason",
                        "Reason",
                    )
                    .required(false),
                ),
            CreateCommand::new("tempban")
                .description("Temporarily ban a member; automatically unban after the duration")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::User,
                        "user",
                        "Member",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "duration",
                        "Examples: 10m, 2h, 1d",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "reason",
                        "Reason",
                    )
                    .required(false),
                ),
            CreateCommand::new("softban")
                .description("Ban and immediately unban a member to clear recent messages")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::User,
                        "user",
                        "Member",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "reason",
                        "Reason",
                    )
                    .required(false),
                ),
            CreateCommand::new("untimeout")
                .description("Remove a member timeout")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::User,
                        "user",
                        "Member",
                    )
                    .required(true),
                ),
            CreateCommand::new("unban")
                .description("Unban a user")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "user_id",
                        "User ID",
                    )
                    .required(true),
                ),
            CreateCommand::new("purge")
                .description("Delete recent messages")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "count",
                        "Messages to delete (1-100)",
                    )
                    .required(true),
                ),
            CreateCommand::new("afk")
                .description("Set or clear your AFK status")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "reason",
                        "Leave empty to clear AFK",
                    )
                    .required(false),
                ),
            CreateCommand::new("remind")
                .description("Create a durable reminder")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "time",
                        "Examples: 10m, 2h, 1d",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "text",
                        "Reminder text",
                    )
                    .required(true),
                ),
            CreateCommand::new("tag")
                .description("Show a saved tag")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "name",
                        "Tag name",
                    )
                    .required(true),
                ),
            CreateCommand::new("tags").description("List saved tags"),
            CreateCommand::new("birthday-set")
                .description("Save your birthday day and month")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "month",
                        "Month (1-12)",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "day",
                        "Day (1-31)",
                    )
                    .required(true),
                ),
            CreateCommand::new("birthday-remove").description("Remove your saved birthday"),
            CreateCommand::new("tag-set")
                .description("Create or update a tag")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "name",
                        "Tag name",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "content",
                        "Tag response",
                    )
                    .required(true),
                ),
            CreateCommand::new("tag-delete")
                .description("Delete a tag")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "name",
                        "Tag name",
                    )
                    .required(true),
                ),
            CreateCommand::new("rank")
                .description("Show a member's XP rank")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::User,
                        "user",
                        "Member",
                    )
                    .required(false),
                ),
            CreateCommand::new("leaderboard").description("Show the XP leaderboard"),
            CreateCommand::new("achievements").description("Show your community achievements"),
            CreateCommand::new("serverstats").description("Show basic server statistics"),
            CreateCommand::new("emojis").description("List this server's custom emojis"),
            CreateCommand::new("invites").description("Show current server invite usage"),
            CreateCommand::new("balance").description("Show your community balance"),
            CreateCommand::new("daily").description("Claim your daily community reward"),
            CreateCommand::new("temp-channel")
                .description("Create a temporary voice channel for yourself"),
            CreateCommand::new("embed")
                .description("Publish a safe embed in this channel (staff)")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "title",
                        "Embed title",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "description",
                        "Embed description",
                    )
                    .required(true),
                ),
            CreateCommand::new("starboard-set")
                .description("Configure the starboard channel (staff)")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Channel,
                        "channel",
                        "Starboard channel",
                    )
                    .required(true),
                ),
            CreateCommand::new("suggest")
                .description("Submit a community suggestion")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "text",
                        "Suggestion text",
                    )
                    .required(true),
                ),
            CreateCommand::new("suggestion")
                .description("Review a community suggestion (staff)")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "id",
                        "Suggestion ID",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "status",
                        "pending, approved, denied or considered",
                    )
                    .required(true),
                ),
            CreateCommand::new("giveaway-start")
                .description("Start a durable giveaway (staff)")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "prize",
                        "Prize",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "duration",
                        "Examples: 10m, 2h, 1d",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "winners",
                        "Number of winners",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Role,
                        "required_role",
                        "Optional required role",
                    )
                    .required(false),
                ),
            CreateCommand::new("gstart")
                .description("Legacy alias for giveaway-start")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "prize",
                        "Prize",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "duration",
                        "Examples: 10m, 2h, 1d",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "winners",
                        "Number of winners",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Role,
                        "required_role",
                        "Optional required role",
                    )
                    .required(false),
                ),
            CreateCommand::new("giveaway-end")
                .description("End a giveaway now (staff)")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "id",
                        "Giveaway ID",
                    )
                    .required(true),
                ),
            CreateCommand::new("gend")
                .description("Legacy alias for giveaway-end")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "id",
                        "Giveaway ID",
                    )
                    .required(true),
                ),
            CreateCommand::new("giveaway-list").description("List active giveaways"),
            CreateCommand::new("glist").description("Legacy alias for giveaway-list"),
            CreateCommand::new("greroll")
                .description("Reroll a finished giveaway")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "id",
                        "Giveaway ID",
                    )
                    .required(true),
                ),
            CreateCommand::new("poll")
                .description("Create a multi-choice poll")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "question",
                        "Question",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "option1",
                        "Option 1",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "option2",
                        "Option 2",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "option3",
                        "Option 3",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "option4",
                        "Option 4",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "option5",
                        "Option 5",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "duration",
                        "Examples: 10m, 2h, 1d",
                    )
                    .required(false),
                ),
            CreateCommand::new("workflow-create")
                .description("Create a bounded message automation (staff)")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "name",
                        "Workflow name",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "reply",
                        "Reply text; use {user} and {message}",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "contains",
                        "Only run when message contains this text",
                    )
                    .required(false),
                ),
            CreateCommand::new("workflow-list").description("List message automations"),
            CreateCommand::new("workflow-dry-run")
                .description("Preview a workflow without executing it (staff)")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "id",
                        "Workflow ID",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "message",
                        "Sample message",
                    )
                    .required(true),
                ),
            CreateCommand::new("workflow-toggle")
                .description("Enable or disable a workflow immediately (staff)")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "id",
                        "Workflow ID",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Boolean,
                        "enabled",
                        "Enable workflow",
                    )
                    .required(true),
                ),
            CreateCommand::new("workflow-delete")
                .description("Delete a message automation (staff)")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "id",
                        "Workflow ID",
                    )
                    .required(true),
                ),
            CreateCommand::new("ticket-panel").description("Create a support ticket panel"),
            CreateCommand::new("ticket-config")
                .description("Configure ticket routing, transcripts and SLA (staff)")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Role,
                        "staff_role",
                        "Role that can see tickets",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Channel,
                        "transcript_channel",
                        "Transcript channel",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "sla_minutes",
                        "SLA reminder in minutes",
                    )
                    .required(false),
                ),
            CreateCommand::new("ticket-update")
                .description("Update ticket category, priority or internal note")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "priority",
                        "Ticket priority",
                    )
                    .add_string_choice("Baixa", "low")
                    .add_string_choice("Normal", "normal")
                    .add_string_choice("Alta", "high")
                    .add_string_choice("Urgente", "urgent")
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "category",
                        "Ticket category",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "note",
                        "Internal note (empty clears it)",
                    )
                    .required(false),
                ),
            CreateCommand::new("ticket-rate")
                .description("Rate a closed support ticket")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "score",
                        "Satisfaction score from 1 to 5",
                    )
                    .required(true),
                ),
            CreateCommand::new("rolepanel")
                .description("Create a self-role panel")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "title",
                        "Panel title",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Role,
                        "role1",
                        "Role 1",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Role,
                        "role2",
                        "Role 2",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Role,
                        "role3",
                        "Role 3",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Role,
                        "role4",
                        "Role 4",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Role,
                        "role5",
                        "Role 5",
                    )
                    .required(false),
                ),
            CreateCommand::new("slowmode")
                .description("Set channel slowmode")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "seconds",
                        "Seconds from 0 to 21600",
                    )
                    .required(true),
                ),
            CreateCommand::new("userinfo")
                .description("Show basic user information")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::User,
                        "user",
                        "User",
                    )
                    .required(true),
                ),
            CreateCommand::new("quarantine")
                .description("Remove a member's roles and quarantine them")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::User,
                        "user",
                        "Member",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "reason",
                        "Reason",
                    )
                    .required(false),
                ),
            CreateCommand::new("unquarantine")
                .description("Restore a quarantined member's roles")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::User,
                        "user",
                        "Member",
                    )
                    .required(true),
                ),
            CreateCommand::new("join-gate")
                .description("Configure the bounded new-member join gate")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Boolean,
                        "enabled",
                        "Enable or disable the gate",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Role,
                        "role",
                        "Role applied to new members while they verify",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "min_age_days",
                        "Flag accounts younger than this many days (0-365)",
                    )
                    .required(false),
                ),
            CreateCommand::new("verify-panel")
                .description("Post the verification panel using the configured join gate"),
            CreateCommand::new("lockdown")
                .description("Lock text channels for @everyone")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "reason",
                        "Reason",
                    )
                    .required(false),
                ),
            CreateCommand::new("unlock")
                .description("Remove the Helper lockdown from text channels"),
            CreateCommand::new("anti-raid")
                .description("Configure the bounded join-burst anti-raid response")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Boolean,
                        "enabled",
                        "Enable or disable the response",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "joins",
                        "Join count that arms the response (2-100)",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "window_seconds",
                        "Sliding window in seconds (3-60)",
                    )
                    .required(false),
                ),
            CreateCommand::new("security-mode")
                .description("Enable or disable shadow mode for high-risk security responses")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Boolean,
                        "shadow",
                        "Record and alert without automatic containment",
                    )
                    .required(true),
                ),
            CreateCommand::new("anti-nuke")
                .description("Configure the audit-log destructive-action guard")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Boolean,
                        "enabled",
                        "Enable or disable the guard",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "actions",
                        "Destructive actions that arm the guard (2-25)",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "window_seconds",
                        "Sliding window in seconds (3-60)",
                    )
                    .required(false),
                ),
            CreateCommand::new("event-create")
                .description("Create a native Discord scheduled event")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "name",
                        "Event name",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "start",
                        "RFC3339 start, for example 2026-07-24T20:00:00Z",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "end",
                        "RFC3339 end",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "location",
                        "External event location",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "description",
                        "Optional event description",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "capacity",
                        "Optional attendee limit (1-100000)",
                    )
                    .required(false),
                ),
            CreateCommand::new("event-list").description("List native Discord scheduled events"),
            CreateCommand::new("event-edit")
                .description("Edit a native Discord scheduled event")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "event_id",
                        "Discord scheduled event ID",
                    )
                    .required(true),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "name",
                        "New event name",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "start",
                        "New RFC3339 start",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "end",
                        "New RFC3339 end",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "location",
                        "New external event location",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "description",
                        "New event description (empty clears it)",
                    )
                    .required(false),
                ),
            CreateCommand::new("event-register")
                .description("Register yourself for a native Discord scheduled event")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "event_id",
                        "Discord scheduled event ID",
                    )
                    .required(true),
                ),
            CreateCommand::new("event-unregister")
                .description("Remove yourself from a scheduled event")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "event_id",
                        "Discord scheduled event ID",
                    )
                    .required(true),
                ),
            CreateCommand::new("event-checkin")
                .description("Check yourself in to a registered scheduled event")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "event_id",
                        "Discord scheduled event ID",
                    )
                    .required(true),
                ),
            CreateCommand::new("event-attendees")
                .description("List registrations for a scheduled event")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "event_id",
                        "Discord scheduled event ID",
                    )
                    .required(true),
                ),
            CreateCommand::new("event-cancel")
                .description("Cancel a native Discord scheduled event")
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::Integer,
                        "event_id",
                        "Discord scheduled event ID",
                    )
                    .required(true),
                ),
        ];
        match Command::set_global_commands(&ctx.http, commands).await {
            Ok(registered) => info!(count = registered.len(), "global commands registered"),
            Err(error) => tracing::error!(%error, "global command registration failed"),
        }
    }

    async fn voice_state_update(
        &self,
        ctx: Context,
        old: Option<serenity::all::VoiceState>,
        new: serenity::all::VoiceState,
    ) {
        let Some(guild_id) = new
            .guild_id
            .or_else(|| old.as_ref().and_then(|state| state.guild_id))
        else {
            return;
        };
        let old_channel_id = old.as_ref().and_then(|state| state.channel_id);
        let new_channel_id = new.channel_id;
        let guild_text = guild_id.to_string();
        let user_text = new.user_id.to_string();

        // Voice XP is session based: joins start a durable snapshot, moves
        // close the old snapshot before starting the new one, and leaves close
        // it without relying on a background timer. The store operation is
        // idempotent so duplicate gateway deliveries cannot double-award XP.
        if old_channel_id != new_channel_id
            && feature_enabled(&self.store, &guild_text, "community.levels", None)
            && setting_bool(
                &self.store,
                &guild_text,
                "community.levels.voice_xp_enabled",
                false,
            )
        {
            let now = Utc::now().timestamp();
            if old_channel_id.is_some()
                && let Ok(Some(minutes)) =
                    self.store
                        .finish_voice_session(&guild_text, &user_text, now)
            {
                let per_minute = setting_i64(
                    &self.store,
                    &guild_text,
                    "community.levels.voice_xp_per_minute",
                    2,
                )
                .clamp(0, 30);
                let xp = minutes.min(24 * 60).saturating_mul(per_minute);
                if xp > 0 {
                    let before = self.store.level_for(&guild_text, &user_text).unwrap_or(0);
                    if let Err(error) = self.store.add_xp(&guild_text, &user_text, xp) {
                        warn!(%guild_id, user = %new.user_id, %error, "failed to award voice XP");
                    } else {
                        let after = self
                            .store
                            .level_for(&guild_text, &user_text)
                            .unwrap_or(before);
                        self.apply_level_rewards(&ctx, guild_id, &user_text, after / 100 + 1)
                            .await;
                        if after / 100 > before / 100 {
                            info!(
                                %guild_id,
                                user = %new.user_id,
                                xp,
                                before,
                                after,
                                "voice XP awarded with level-up"
                            );
                        }
                    }
                }
            }
            if let Some(channel_id) = new_channel_id
                && let Err(error) = self.store.start_voice_session(
                    &guild_text,
                    &user_text,
                    &channel_id.to_string(),
                    now,
                )
            {
                warn!(%guild_id, user = %new.user_id, %error, "failed to start voice XP session");
            }
        }

        // Temporary channel ownership cleanup remains independent from XP. A
        // member can leave/move through a normal voice channel without there
        // being a temp-channel record.
        if let Some(old_channel_id) = old_channel_id {
            let channel_id = old_channel_id.to_string();
            if let Ok(Some(record)) = self.store.temp_channel(&channel_id)
                && user_text == record.owner_id
            {
                if let Err(error) = old_channel_id.delete(&ctx.http).await {
                    warn!(%guild_id, %old_channel_id, %error, "failed to remove ownerless temporary voice channel");
                } else if let Err(error) = self.store.remove_temp_channel(&channel_id) {
                    warn!(%guild_id, %old_channel_id, %error, "failed to remove temporary channel record");
                }
            }
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match interaction {
            Interaction::Command(command) => {
                if let Err(error) = self.handle_command(&ctx, &command).await {
                    tracing::error!(%error, command = %command.data.name, "command failed");
                    let _ = command
                        .create_response(
                            &ctx,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("Something went wrong while running that command.")
                                    .ephemeral(true),
                            ),
                        )
                        .await;
                }
            }
            Interaction::Component(component) => {
                if let Err(error) = self.handle_component(&ctx, &component).await {
                    tracing::error!(%error, "component failed");
                    let _ = component
                        .create_response(
                            &ctx,
                            CreateInteractionResponse::Message(
                                CreateInteractionResponseMessage::new()
                                    .content("Unable to complete this action.")
                                    .ephemeral(true),
                            ),
                        )
                        .await;
                }
            }
            _ => {}
        }
    }

    async fn guild_member_addition(&self, ctx: Context, new_member: serenity::all::Member) {
        let guild_id = new_member.guild_id;
        let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let _ = self.store.record_join(&guild_id.to_string(), &day);
        let guild_text = guild_id.to_string();
        let anti_raid_enabled = self
            .store
            .get_setting(&guild_text, "security.anti_raid.enabled")
            .ok()
            .flatten()
            .is_some_and(|value| value == "true");
        if anti_raid_enabled {
            let threshold = self
                .store
                .get_setting(&guild_text, "security.anti_raid.joins")
                .ok()
                .flatten()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(10)
                .clamp(2, 100);
            let window_seconds = self
                .store
                .get_setting(&guild_text, "security.anti_raid.window_seconds")
                .ok()
                .flatten()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(10)
                .clamp(3, 60);
            let armed = {
                let mut joins = self.joins.lock().expect("join mutex poisoned");
                join_burst_armed(
                    &mut joins,
                    &guild_text,
                    Instant::now(),
                    Duration::from_secs(window_seconds),
                    threshold,
                )
            };
            if armed {
                let shadow_mode = shadow_mode_enabled(
                    self.store
                        .get_setting(&guild_text, "security.shadow_mode")
                        .ok()
                        .flatten()
                        .as_deref(),
                ) || setting_bool(
                    &self.store,
                    &guild_text,
                    "security.anti_raid.alert_only",
                    false,
                );
                // Bounded response: latch the existing gate and alert moderators.
                // Shadow mode records and alerts but deliberately does not contain.
                if !shadow_mode {
                    let _ =
                        self.store
                            .set_setting(&guild_text, "security.join_gate.enabled", "true");
                }
                let reason = format!(
                    "Anti-raid: {threshold} joins dentro de {window_seconds}s; {}",
                    if shadow_mode {
                        "shadow mode, sem contenção automática"
                    } else {
                        "join gate ativado"
                    }
                );
                let _ = self.store.record_case(
                    &guild_text,
                    if shadow_mode {
                        "anti_raid_shadow"
                    } else {
                        "anti_raid"
                    },
                    &new_member.user.id.to_string(),
                    "helper",
                    &reason,
                    None,
                );
                let alert_channel = setting_u64_optional(
                    &self.store,
                    &guild_text,
                    "security.anti_raid.alert_channel",
                )
                .map(ChannelId::new);
                let fallback_channel = guild_id
                    .to_partial_guild(&ctx.http)
                    .await
                    .ok()
                    .and_then(|guild| guild.system_channel_id);
                if let Some(channel_id) = alert_channel.or(fallback_channel) {
                    let _ = channel_id
                        .say(
                            &ctx.http,
                            format!(
                                "⚠️ Possible raid detected: {threshold} joins in {window_seconds}s. {} Review the recent cases.",
                                if shadow_mode {
                                    "Shadow mode is active; no automatic containment was applied."
                                } else {
                                    "The join gate was enabled."
                                }
                            ),
                        )
                        .await;
                }
            }
        }
        let gate_enabled = self
            .store
            .get_setting(&guild_text, "security.join_gate.enabled")
            .ok()
            .flatten()
            .is_some_and(|value| value == "true");
        if gate_enabled {
            if let Ok(Some(raw_role)) = self
                .store
                .get_setting(&guild_text, "security.join_gate.role_id")
                && let Ok(role_id) = raw_role.parse::<u64>()
            {
                let _ = new_member.add_role(&ctx.http, RoleId::new(role_id)).await;
            }
            let minimum_age = self
                .store
                .get_setting(&guild_text, "security.join_gate.min_age_days")
                .ok()
                .flatten()
                .and_then(|value| value.parse::<i64>().ok())
                .unwrap_or(0)
                .clamp(0, 365);
            let account_age_days = account_age_days(
                chrono::Utc::now().timestamp(),
                new_member.user.created_at().unix_timestamp(),
            );
            if account_age_days < minimum_age {
                let reason = format!(
                    "Join gate: account is {account_age_days} day(s) old; configured minimum is {minimum_age}"
                );
                let _ = self.store.record_case(
                    &guild_text,
                    "join_gate",
                    &new_member.user.id.to_string(),
                    "helper",
                    &reason,
                    None,
                );
                if let Ok(guild) = guild_id.to_partial_guild(&ctx.http).await
                    && let Some(channel_id) = guild.system_channel_id
                {
                    let _ = channel_id
                        .say(
                            &ctx.http,
                            format!(
                                "⚠️ <@{}> needs verification: this account is too new.",
                                new_member.user.id
                            ),
                        )
                        .await;
                }
            }
        }
        if feature_enabled(&self.store, &guild_text, "support.welcome", None) {
            let member_mention = format!("<@{}>", new_member.user.id);
            if let Some(role_id) =
                setting_u64_optional(&self.store, &guild_text, "support.welcome.auto_role")
            {
                let _ = new_member.add_role(&ctx.http, RoleId::new(role_id)).await;
            }
            let message = setting_string(&self.store, &guild_text, "support.welcome.message")
                .unwrap_or_else(|| "👋 Welcome to the server, {member}!".to_string())
                .replace("{member}", &member_mention)
                .replace("{server}", "this server");
            let fallback_channel = guild_id
                .to_partial_guild(&ctx.http)
                .await
                .ok()
                .and_then(|guild| guild.system_channel_id);
            let channel =
                setting_u64_optional(&self.store, &guild_text, "support.welcome.channel_id")
                    .map(ChannelId::new)
                    .or(fallback_channel);
            if let Some(channel_id) = channel {
                let _ = channel_id.say(&ctx.http, message).await;
            }
            if setting_bool(&self.store, &guild_text, "support.welcome.send_dm", false) {
                let dm = setting_string(&self.store, &guild_text, "support.welcome.dm_message")
                    .unwrap_or_else(|| "Hello {member}, welcome to the server!".to_string())
                    .replace("{member}", &member_mention)
                    .replace("{server}", "this server");
                let _ = new_member
                    .user
                    .direct_message(&ctx.http, serenity::all::CreateMessage::new().content(dm))
                    .await;
            }
        }
        if feature_enabled(&self.store, &guild_text, "support.welcome_channel", None)
            && let Some(channel_id) = setting_u64_optional(
                &self.store,
                &guild_text,
                "support.welcome_channel.channel_id",
            )
        {
            let member_mention = format!("<@{}>", new_member.user.id);
            let guide = setting_string(
                &self.store,
                &guild_text,
                "support.welcome_channel.message",
            )
            .unwrap_or_else(|| {
                "Welcome {member}! Start with the rules, introduce yourself and check the server channels.".to_string()
            })
            .replace("{member}", &member_mention)
            .replace("{server}", "this server");
            let _ = ChannelId::new(channel_id).say(&ctx.http, guide).await;
        }
    }

    async fn guild_member_removal(
        &self,
        _ctx: Context,
        guild_id: serenity::all::GuildId,
        user: serenity::all::User,
        _member_data_if_available: Option<serenity::all::Member>,
    ) {
        let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let _ = self.store.record_leave(&guild_id.to_string(), &day);
        if let Ok(deleted) = self
            .store
            .delete_member_voluntary_data(&guild_id.to_string(), &user.id.to_string())
        {
            tracing::debug!(
                guild_id = %guild_id,
                user_id = %user.id,
                ?deleted,
                "removed member voluntary state"
            );
        }
    }

    async fn auto_moderation_action_execution(
        &self,
        _ctx: Context,
        execution: serenity::all::ActionExecution,
    ) {
        let reason = format!(
            "Discord AutoMod rule {} ({:?}){}",
            execution.rule_id,
            execution.trigger_type,
            execution
                .matched_keyword
                .as_deref()
                .map(|keyword| format!("; matched keyword: {keyword}"))
                .unwrap_or_default()
        );
        let _ = self.store.record_case(
            &execution.guild_id.to_string(),
            "automod",
            &execution.user_id.to_string(),
            "discord-automod",
            &reason,
            None,
        );
    }

    async fn guild_audit_log_entry_create(
        &self,
        ctx: Context,
        entry: serenity::all::AuditLogEntry,
        guild_id: serenity::all::GuildId,
    ) {
        let guild_text = guild_id.to_string();
        if !feature_enabled(&self.store, &guild_text, "management.audit", None) {
            return;
        }
        let enabled = self
            .store
            .get_setting(&guild_text, "security.anti_nuke.enabled")
            .ok()
            .flatten()
            .is_some_and(|value| value == "true");
        if !enabled || !is_destructive_audit_action(entry.action) {
            return;
        }
        let threshold = self
            .store
            .get_setting(&guild_text, "security.anti_nuke.actions")
            .ok()
            .flatten()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(3)
            .clamp(2, 25);
        let window_seconds = self
            .store
            .get_setting(&guild_text, "security.anti_nuke.window_seconds")
            .ok()
            .flatten()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(10)
            .clamp(3, 60);
        let executor = entry.user_id.to_string();
        let armed = {
            let mut events = self.nuke_events.lock().expect("anti-nuke mutex poisoned");
            join_burst_armed(
                &mut events,
                &format!("{guild_text}:{executor}"),
                Instant::now(),
                Duration::from_secs(window_seconds),
                threshold,
            )
        };
        let action = format!("audit action {}", entry.action.num());
        let reason = format!("Anti-nuke audit: {action} by <@{executor}>");
        let _ = self.store.record_case(
            &guild_text,
            "anti_nuke_audit",
            &executor,
            "discord-audit-log",
            &reason,
            None,
        );
        if !armed {
            return;
        }
        let shadow_mode = shadow_mode_enabled(
            self.store
                .get_setting(&guild_text, "security.shadow_mode")
                .ok()
                .flatten()
                .as_deref(),
        );
        if shadow_mode {
            let _ = self.store.record_case(
                &guild_text,
                "anti_nuke_shadow",
                &executor,
                "helper",
                &format!(
                    "{threshold} destructive actions in {window_seconds}s; shadow mode, no automatic containment"
                ),
                None,
            );
            if let Ok(guild) = guild_id.to_partial_guild(&ctx.http).await
                && let Some(channel_id) = guild.system_channel_id
            {
                let _ = channel_id
                    .say(
                        &ctx.http,
                        format!(
                            "🚨 Possible nuke attack: <@{executor}> performed {threshold} destructive actions in {window_seconds}s. Shadow mode is active; no automatic containment was applied."
                        ),
                    )
                    .await;
            }
            return;
        }
        // Containment is intentionally reversible and non-destructive: latch the
        // join gate and ask staff to review the executor before any role removal.
        let _ = self
            .store
            .set_setting(&guild_text, "security.join_gate.enabled", "true");
        let _ = self.store.record_case(
            &guild_text,
            "anti_nuke",
            &executor,
            "helper",
            &format!(
                "{threshold} destructive actions in {window_seconds}s; join gate enabled for review"
            ),
            None,
        );
        if let Ok(guild) = guild_id.to_partial_guild(&ctx.http).await
            && let Some(channel_id) = guild.system_channel_id
        {
            let _ = channel_id
                .say(
                    &ctx.http,
                    format!(
                        "🚨 Possible nuke attack: <@{executor}> performed {threshold} destructive actions in {window_seconds}s. The join gate was enabled; review the Audit Log before removing roles."
                    ),
                )
                .await;
        }
    }

    async fn reaction_add(&self, ctx: Context, reaction: serenity::all::Reaction) {
        let Some(guild_id) = reaction.guild_id else {
            return;
        };
        if self
            .store
            .get_setting(&guild_id.to_string(), "feature.community.starboard")
            .ok()
            .flatten()
            .is_some_and(|value| value != "true")
        {
            return;
        }
        if reaction.user_id == ctx.http.get_current_user().await.ok().map(|user| user.id) {
            return;
        }
        let serenity::all::ReactionType::Unicode(emoji) = &reaction.emoji else {
            return;
        };
        let configured_emoji = setting_string(
            &self.store,
            &guild_id.to_string(),
            "community.starboard.emoji",
        )
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "⭐".to_string());
        if emoji != &configured_emoji && emoji != "🌟" {
            return;
        }
        let ignored_channels = setting_string(
            &self.store,
            &guild_id.to_string(),
            "community.starboard.ignored_channels",
        )
        .unwrap_or_default();
        if ignored_channels
            .split(',')
            .map(str::trim)
            .any(|channel| channel == reaction.channel_id.to_string())
        {
            return;
        }
        let Ok(Some(raw_board_id)) = self
            .store
            .get_setting(&guild_id.to_string(), "community.starboard.channel_id")
        else {
            return;
        };
        let Ok(board_id) = raw_board_id.parse::<u64>() else {
            return;
        };
        if board_id == reaction.channel_id.get() {
            return;
        }
        let Ok(users) = reaction
            .channel_id
            .reaction_users(
                &ctx.http,
                reaction.message_id,
                reaction.emoji.clone(),
                Some(100),
                None,
            )
            .await
        else {
            return;
        };
        let count = users.len() as i64;
        let original = match reaction
            .channel_id
            .message(&ctx.http, reaction.message_id)
            .await
        {
            Ok(message) => message,
            Err(_) => return,
        };
        if !setting_bool(
            &self.store,
            &guild_id.to_string(),
            "community.starboard.allow_self_star",
            false,
        ) && reaction.user_id == Some(original.author.id)
        {
            return;
        }
        if !setting_bool(
            &self.store,
            &guild_id.to_string(),
            "community.starboard.include_images",
            true,
        ) && !original.attachments.is_empty()
        {
            return;
        }
        let link = format!(
            "https://discord.com/channels/{}/{}/{}",
            guild_id, reaction.channel_id, reaction.message_id
        );
        let threshold = setting_i64(
            &self.store,
            &guild_id.to_string(),
            "community.starboard.threshold",
            3,
        )
        .clamp(1, 100);
        if count < threshold {
            if let Ok(Some(entry)) = self
                .store
                .star_entry(&guild_id.to_string(), &reaction.message_id.to_string())
            {
                if let Ok(message_id) = entry.starboard_message_id.parse::<u64>() {
                    let _ = serenity::all::ChannelId::new(board_id)
                        .delete_message(&ctx.http, serenity::all::MessageId::new(message_id))
                        .await;
                }
                let _ = self
                    .store
                    .delete_star_entry(&guild_id.to_string(), &reaction.message_id.to_string());
            }
            return;
        }
        let content = format!(
            "⭐ **{} estrelas** em <@{}>\n{}\n{}",
            count, original.author.id, original.content, link
        );
        if let Ok(Some(entry)) = self
            .store
            .star_entry(&guild_id.to_string(), &reaction.message_id.to_string())
        {
            if let Ok(message_id) = entry.starboard_message_id.parse::<u64>() {
                let _ = serenity::all::ChannelId::new(board_id)
                    .edit_message(
                        &ctx.http,
                        serenity::all::MessageId::new(message_id),
                        serenity::all::EditMessage::new().content(content),
                    )
                    .await;
                let _ = self.store.upsert_star_entry(
                    &guild_id.to_string(),
                    &reaction.message_id.to_string(),
                    &entry.starboard_message_id,
                    count,
                );
            }
        } else if let Ok(message) = serenity::all::ChannelId::new(board_id)
            .say(&ctx.http, content)
            .await
        {
            let _ = self.store.upsert_star_entry(
                &guild_id.to_string(),
                &reaction.message_id.to_string(),
                &message.id.to_string(),
                count,
            );
        }
    }

    async fn message_delete(
        &self,
        ctx: Context,
        _channel_id: ChannelId,
        deleted_message_id: serenity::all::MessageId,
        guild_id: Option<serenity::all::GuildId>,
    ) {
        let Some(guild_id) = guild_id else {
            return;
        };
        let guild_text = guild_id.to_string();
        let Ok(Some(entry)) = self
            .store
            .star_entry(&guild_text, &deleted_message_id.to_string())
        else {
            return;
        };
        if let Some(board_id) =
            setting_string(&self.store, &guild_text, "community.starboard.channel_id")
                .and_then(|value| value.parse::<u64>().ok())
            && let Ok(starboard_message_id) = entry.starboard_message_id.parse::<u64>()
        {
            let _ = ChannelId::new(board_id)
                .delete_message(
                    &ctx.http,
                    serenity::all::MessageId::new(starboard_message_id),
                )
                .await;
        }
        let _ = self
            .store
            .delete_star_entry(&guild_text, &deleted_message_id.to_string());
    }

    async fn message_update(
        &self,
        _ctx: Context,
        _old: Option<serenity::all::Message>,
        _new: Option<serenity::all::Message>,
        event: MessageUpdateEvent,
    ) {
        let Some(guild_id) = event.guild_id else {
            return;
        };
        let guild_text = guild_id.to_string();
        if !feature_enabled(&self.store, &guild_text, "management.audit", None) {
            return;
        }
        let author_id = event
            .author
            .as_ref()
            .map(|author| author.id.to_string())
            .unwrap_or_else(|| "unknown".into());
        let author_tag = event.author.as_ref().map(|author| author.tag());
        let detail = serde_json::json!({
            "messageId": event.id.to_string(),
            "channelId": event.channel_id.to_string(),
            "contentAvailable": event.content.is_some(),
            "embedsChanged": event.embeds.is_some(),
            "attachmentsChanged": event.attachments.is_some(),
            "editedTimestamp": event.edited_timestamp.as_ref().map(ToString::to_string),
        })
        .to_string();
        let _ = self.store.record_activity(
            &guild_text,
            "message_edit",
            &author_id,
            author_tag.as_deref(),
            Some(&author_id),
            &detail,
        );
    }

    async fn reaction_remove(&self, ctx: Context, reaction: serenity::all::Reaction) {
        // The board reconciler reads the current reaction users, so the same
        // path safely handles additions, removals and Discord retries.
        self.reaction_add(ctx, reaction).await;
    }

    async fn message(&self, ctx: Context, message: serenity::all::Message) {
        if message.author.bot {
            return;
        }
        let Some(guild_id) = message.guild_id else {
            return;
        };
        let guild_text = guild_id.to_string();
        let user_text = message.author.id.to_string();
        let _ = self.store.record_message(
            &guild_text,
            &chrono::Utc::now().format("%Y-%m-%d").to_string(),
        );
        if feature_enabled(&self.store, &guild_text, "community.levels", None) {
            let ignored_channels = setting_string(
                &self.store,
                &guild_text,
                "community.levels.ignored_channels",
            )
            .unwrap_or_default();
            let channel_ignored = ignored_channels
                .split(',')
                .any(|channel| channel.trim() == message.channel_id.to_string());
            if !channel_ignored {
                let cooldown = setting_u64(
                    &self.store,
                    &guild_text,
                    "community.levels.cooldown_seconds",
                    60,
                )
                .clamp(0, 3_600);
                let xp_key = format!("{guild_text}:{user_text}");
                let now = Instant::now();
                let should_award = {
                    let mut awarded = self.xp_awarded_at.lock().expect("xp mutex poisoned");
                    let ready = awarded
                        .get(&xp_key)
                        .is_none_or(|at| now.duration_since(*at) >= Duration::from_secs(cooldown));
                    if ready {
                        awarded.insert(xp_key, now);
                    }
                    ready
                };
                if should_award {
                    let minimum =
                        setting_i64(&self.store, &guild_text, "community.levels.xp_min", 15)
                            .clamp(1, 1_000);
                    let maximum =
                        setting_i64(&self.store, &guild_text, "community.levels.xp_max", 30)
                            .clamp(minimum, 2_000);
                    let span = (maximum - minimum + 1) as u64;
                    let stable = message.id.to_string().bytes().fold(0_u64, |total, byte| {
                        total.wrapping_mul(33).wrapping_add(byte as u64)
                    });
                    let amount = minimum + (stable % span) as i64;
                    let before = self.store.level_for(&guild_text, &user_text).unwrap_or(0);
                    let _ = self.store.add_xp(&guild_text, &user_text, amount);
                    let after = self
                        .store
                        .level_for(&guild_text, &user_text)
                        .unwrap_or(before);
                    let before_level = before / 100 + 1;
                    let after_level = after / 100 + 1;
                    if after_level > before_level {
                        self.apply_level_rewards(&ctx, guild_id, &user_text, after_level)
                            .await;
                        let text = setting_string(
                            &self.store,
                            &guild_text,
                            "community.levels.announce_template",
                        )
                        .unwrap_or_else(|| "{member} reached level {level}!".to_string())
                        .replace("{member}", &format!("<@{}>", message.author.id))
                        .replace("{level}", &after_level.to_string())
                        .replace("{server}", "this server");
                        let channel = setting_u64_optional(
                            &self.store,
                            &guild_text,
                            "community.levels.announce_channel",
                        )
                        .map(ChannelId::new)
                        .unwrap_or(message.channel_id);
                        let _ = channel.say(&ctx.http, text).await;
                    }
                }
            }
        }
        if let Ok(Some(afk)) = self.store.get_afk(&guild_text, &user_text) {
            let _ = self.store.clear_afk(&guild_text, &user_text);
            let _ = message
                .channel_id
                .say(
                    &ctx.http,
                    format!(
                        "Welcome back, <@{}>. Your AFK status was removed.",
                        afk.user_id
                    ),
                )
                .await;
        }
        if feature_enabled(&self.store, &guild_text, "protection.antispam", None) {
            let policy = anti_spam_policy_for_store(&self.store, &guild_text);
            let window_seconds = policy.window_seconds;
            let now = Instant::now();
            let key = format!("{}:{}", guild_id, message.author.id);
            let count = {
                let mut states = self.spam.lock().expect("spam mutex poisoned");
                let window = states.entry(key.clone()).or_default();
                while window
                    .front()
                    .is_some_and(|at| now.duration_since(*at) > Duration::from_secs(window_seconds))
                {
                    window.pop_front();
                }
                window.push_back(now);
                window.len()
            };
            let normalized = message.content.trim().to_lowercase();
            let duplicate_count = {
                let mut messages = self
                    .duplicate_messages
                    .lock()
                    .expect("duplicate spam mutex poisoned");
                let recent = messages.entry(key.clone()).or_default();
                while recent.front().is_some_and(|(at, _)| {
                    now.duration_since(*at) > Duration::from_secs(window_seconds)
                }) {
                    recent.pop_front();
                }
                recent.push_back((now, normalized.clone()));
                recent
                    .iter()
                    .filter(|(_, content)| content == &normalized)
                    .count()
            };
            let role_ids = message
                .member
                .as_ref()
                .map(|member| member.roles.iter().map(ToString::to_string).collect())
                .unwrap_or_default();
            let decision = evaluate_anti_spam(
                &policy,
                &AntiSpamObservation {
                    channel_id: message.channel_id.to_string(),
                    role_ids,
                    message_count: count as u32,
                    duplicate_count: duplicate_count as u32,
                    mention_count: message.mentions.len() as u32,
                },
            );
            if !decision.ignored && !decision.matched.is_empty() {
                let should_emit = {
                    let mut actions = self
                        .spam_action_at
                        .lock()
                        .expect("spam action mutex poisoned");
                    actions.retain(|_, at| {
                        now.duration_since(*at)
                            < Duration::from_secs(window_seconds.saturating_mul(2))
                    });
                    let allowed = actions.get(&key).is_none_or(|at| {
                        now.duration_since(*at) >= Duration::from_secs(window_seconds)
                    });
                    if allowed {
                        actions.insert(key.clone(), now);
                    }
                    allowed
                };
                if should_emit {
                    let _ = self.store.record_case(
                        &guild_id.to_string(),
                        "anti-spam",
                        &message.author.id.to_string(),
                        &message.author.id.to_string(),
                        &format!(
                            "Spam detected ({signals}): {count} messages/{window_seconds}s, {duplicate_count} duplicates, {mention_count} mentions",
                            signals = decision.matched.join(", "),
                            mention_count = message.mentions.len(),
                        ),
                        None,
                    );
                    if let Some(raw_channel) =
                        setting_string(&self.store, &guild_text, "security.antispam.log_channel")
                            .filter(|value| !value.trim().is_empty())
                        && let Ok(channel) = raw_channel.parse::<u64>()
                    {
                        let _ = ChannelId::new(channel)
                            .say(
                                &ctx.http,
                                format!(
                                    "Anti-spam: <@{}> — {} ({})",
                                    message.author.id,
                                    decision.reason,
                                    if decision.should_act {
                                        "action"
                                    } else {
                                        "monitoring"
                                    },
                                ),
                            )
                            .await;
                    }
                    if decision.should_act {
                        if decision.timeout_seconds > 0 {
                            let until = (chrono::Utc::now()
                                + chrono::Duration::seconds(decision.timeout_seconds as i64))
                            .to_rfc3339();
                            let _ = guild_id
                                .edit_member(
                                    &ctx.http,
                                    message.author.id,
                                    serenity::all::EditMember::new()
                                        .disable_communication_until(until),
                                )
                                .await;
                        }
                        let _ = message
                            .channel_id
                            .say(
                                &ctx.http,
                                format!(
                                    "<@{}>, please slow down — anti-spam recorded this incident.",
                                    message.author.id
                                ),
                            )
                            .await;
                    }
                }
            }
        }
        if feature_enabled(&self.store, &guild_text, "protection.antiscam", None) {
            let policy = scam_policy_for_store(&self.store, &guild_text);
            let decision =
                evaluate_scam(&policy, &message.channel_id.to_string(), &message.content);
            if !decision.ignored && !decision.matched.is_empty() {
                let reason = format!("Scam protection matched: {}", decision.matched.join(", "));
                let _ = self.store.record_case(
                    &guild_text,
                    "anti-scam",
                    &message.author.id.to_string(),
                    &message.author.id.to_string(),
                    &reason,
                    None,
                );
                if let Some(raw_channel) =
                    setting_string(&self.store, &guild_text, "security.antiscam.log_channel")
                        .filter(|value| !value.trim().is_empty())
                    && let Ok(channel) = raw_channel.parse::<u64>()
                {
                    let _ = ChannelId::new(channel)
                        .say(
                            &ctx.http,
                            format!(
                                "Scam protection: <@{}> — {} ({})",
                                message.author.id,
                                reason,
                                if decision.should_act {
                                    "action"
                                } else {
                                    "monitoring"
                                }
                            ),
                        )
                        .await;
                }
                if decision.should_act {
                    let _ = message.delete(&ctx.http).await;
                    if decision.timeout_seconds > 0 {
                        let until = (chrono::Utc::now()
                            + chrono::Duration::seconds(decision.timeout_seconds as i64))
                        .to_rfc3339();
                        let _ = guild_id
                            .edit_member(
                                &ctx.http,
                                message.author.id,
                                serenity::all::EditMember::new().disable_communication_until(until),
                            )
                            .await;
                    }
                }
            }
        }
        if feature_enabled(&self.store, &guild_text, "management.workflows", None)
            && let Ok(workflows) = self.store.active_workflows(&guild_text, "message")
        {
            let lower = message.content.to_lowercase();
            let max_reply_length = setting_u64(
                &self.store,
                &guild_text,
                "management.workflows.max_reply_length",
                1_000,
            )
            .clamp(1, 1_500) as usize;
            let allow_mentions = setting_bool(
                &self.store,
                &guild_text,
                "management.workflows.allow_mentions",
                false,
            );
            for workflow in workflows {
                if !workflow.condition.is_empty()
                    && !lower.contains(&workflow.condition.to_lowercase())
                {
                    continue;
                }
                if workflow.action != "reply" {
                    continue;
                }
                let Ok(true) = self.store.record_workflow_run(
                    workflow.id,
                    &guild_text,
                    &message.id.to_string(),
                ) else {
                    continue;
                };
                let mut reply = workflow
                    .payload
                    .replace("{user}", &format!("<@{}>", message.author.id))
                    .replace("{message}", &truncate(&message.content, 500));
                if !allow_mentions {
                    reply = reply
                        .replace("@everyone", "@\u{200b}everyone")
                        .replace("@here", "@\u{200b}here");
                }
                let _ = message
                    .channel_id
                    .say(&ctx.http, truncate(&reply, max_reply_length))
                    .await;
            }
        }
    }
}

pub async fn run(config: &Config) -> Result<()> {
    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MEMBERS
        | GatewayIntents::GUILD_MODERATION
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::GUILD_MESSAGE_REACTIONS
        | GatewayIntents::GUILD_INVITES
        | GatewayIntents::GUILD_VOICE_STATES
        | GatewayIntents::GUILD_WEBHOOKS
        | GatewayIntents::AUTO_MODERATION_CONFIGURATION
        | GatewayIntents::AUTO_MODERATION_EXECUTION
        | GatewayIntents::MESSAGE_CONTENT;
    let store = Store::open(&config.database_url)?;
    let mut client = Client::builder(&config.discord_token, intents)
        .event_handler(Handler {
            store,
            spam: Arc::new(Mutex::new(HashMap::new())),
            duplicate_messages: Arc::new(Mutex::new(HashMap::new())),
            spam_action_at: Arc::new(Mutex::new(HashMap::new())),
            joins: Arc::new(Mutex::new(HashMap::new())),
            nuke_events: Arc::new(Mutex::new(HashMap::new())),
            xp_awarded_at: Arc::new(Mutex::new(HashMap::new())),
            scheduler_started: Arc::new(AtomicBool::new(false)),
            entitlements: EntitlementClient::new(
                config.entitlement_url.clone(),
                config.entitlement_secret.clone(),
            ),
            youtube: YouTubeClient::from_env(),
            rss: Some(RssClient::new()),
            twitch: TwitchClient::from_env(),
        })
        .application_id(config.discord_application_id.into())
        .await?;
    client.start().await?;
    Ok(())
}

async fn run_youtube_worker(http: Arc<serenity::http::Http>, store: Store, youtube: YouTubeClient) {
    let mut interval = tokio::time::interval(Duration::from_secs(15));
    loop {
        interval.tick().await;
        let due = match store.due_youtube_subscriptions(Utc::now().timestamp_millis(), 25) {
            Ok(rows) => rows,
            Err(error) => {
                tracing::error!(%error, "youtube worker could not load subscriptions");
                continue;
            }
        };
        for subscription in due {
            if !feature_enabled(&store, &subscription.guild_id, "social.youtube", None) {
                let next = Utc::now().timestamp_millis() + subscription.interval_seconds * 1_000;
                let _ = store.update_youtube_poll(
                    subscription.id,
                    subscription.last_video_id.as_deref(),
                    next,
                    subscription.failure_count,
                    Some("feature_disabled"),
                );
                continue;
            }
            if let Err(error) =
                process_youtube_subscription(&http, &store, &youtube, &subscription).await
            {
                tracing::warn!(%error, subscription_id = subscription.id, "youtube subscription failed");
            }
        }
    }
}

async fn process_youtube_subscription(
    http: &serenity::http::Http,
    store: &Store,
    youtube: &YouTubeClient,
    subscription: &YouTubeSubscriptionRecord,
) -> Result<()> {
    let now = Utc::now().timestamp_millis();
    let interval_ms = subscription.interval_seconds.clamp(300, 86_400) * 1_000;
    let next = || now + interval_ms;
    let latest = match youtube.latest_video(&subscription.source_channel_id).await {
        Ok(video) => video,
        Err(error) => {
            let failures = subscription.failure_count.saturating_add(1).min(8);
            let backoff = (subscription.interval_seconds * (1_i64 << failures.min(4))).min(3_600);
            store.update_youtube_poll(
                subscription.id,
                subscription.last_video_id.as_deref(),
                now + backoff * 1_000,
                failures,
                Some(&provider_error_code(&error)),
            )?;
            return Err(error);
        }
    };
    let Some(video) = latest else {
        store.update_youtube_poll(
            subscription.id,
            subscription.last_video_id.as_deref(),
            next(),
            0,
            None,
        )?;
        return Ok(());
    };
    if subscription.last_video_id.is_none() {
        // Establish a baseline on first poll. Existing videos must not flood
        // a server when an administrator enables an alert.
        store.update_youtube_poll(subscription.id, Some(&video.id), next(), 0, None)?;
        return Ok(());
    }
    if subscription.last_video_id.as_deref() == Some(video.id.as_str()) {
        store.update_youtube_poll(subscription.id, Some(&video.id), next(), 0, None)?;
        return Ok(());
    }
    let content = format_youtube_message(
        &subscription.message_template,
        &subscription.mention,
        &video,
        &subscription.source_channel_id,
    );
    let channel_id = subscription
        .target_channel_id
        .parse::<u64>()
        .map_err(|_| anyhow::anyhow!("invalid_discord_channel_id"))?;
    ChannelId::new(channel_id).say(http, content).await?;
    store.update_youtube_poll(subscription.id, Some(&video.id), next(), 0, None)?;
    Ok(())
}

fn provider_error_code(error: &anyhow::Error) -> String {
    let value = error.to_string();
    if value.starts_with("youtube_api_error:") || value.starts_with("rss_http_error:") {
        value
    } else {
        "provider_request_failed".into()
    }
}

fn format_youtube_message(
    template: &str,
    mention: &str,
    video: &YouTubeVideo,
    channel_id: &str,
) -> String {
    let rendered = template
        .replace("{title}", &video.title)
        .replace("{url}", &video.url)
        .replace(
            "{channel}",
            if video.channel_title.is_empty() {
                channel_id
            } else {
                &video.channel_title
            },
        )
        .replace("{published_at}", &video.published_at)
        .replace("{description}", &video.description);
    let rendered = if mention.is_empty() {
        rendered
    } else {
        format!("{mention} {rendered}")
    };
    // Discord rejects messages above 2,000 Unicode scalar values. Templates
    // are bounded in the API, but provider fields such as descriptions are
    // not, so enforce the platform limit after substitutions as well.
    rendered.chars().take(2_000).collect()
}

async fn run_rss_worker(http: Arc<serenity::http::Http>, store: Store, rss: RssClient) {
    let mut interval = tokio::time::interval(Duration::from_secs(15));
    loop {
        interval.tick().await;
        let due = match store.due_rss_subscriptions(Utc::now().timestamp_millis(), 25) {
            Ok(rows) => rows,
            Err(error) => {
                tracing::error!(%error, "rss worker could not load subscriptions");
                continue;
            }
        };
        for subscription in due {
            // Podcast alerts are the same bounded RSS/Atom transport. A
            // subscription is active when either product surface is enabled;
            // this keeps the worker single-purpose and prevents duplicate
            // polling or delivery.
            let rss_enabled = feature_enabled(&store, &subscription.guild_id, "social.rss", None);
            let podcasts_enabled =
                feature_enabled(&store, &subscription.guild_id, "social.podcasts", None);
            if !rss_enabled && !podcasts_enabled {
                let next = Utc::now().timestamp_millis() + subscription.interval_seconds * 1_000;
                let _ = store.update_rss_poll(
                    subscription.id,
                    subscription.last_item_id.as_deref(),
                    next,
                    subscription.failure_count,
                    Some("feature_disabled"),
                );
                continue;
            }
            if let Err(error) = process_rss_subscription(&http, &store, &rss, &subscription).await {
                tracing::warn!(%error, subscription_id = subscription.id, "rss subscription failed");
            }
        }
    }
}

async fn process_rss_subscription(
    http: &serenity::http::Http,
    store: &Store,
    rss: &RssClient,
    subscription: &RssSubscriptionRecord,
) -> Result<()> {
    let now = Utc::now().timestamp_millis();
    let interval_ms = subscription.interval_seconds.clamp(300, 86_400) * 1_000;
    let next = || now + interval_ms;
    let feed = match rss.fetch(&subscription.feed_url).await {
        Ok(feed) => feed,
        Err(error) => {
            let failures = subscription.failure_count.saturating_add(1).min(8);
            let backoff = (subscription.interval_seconds * (1_i64 << failures.min(4))).min(3_600);
            store.update_rss_poll(
                subscription.id,
                subscription.last_item_id.as_deref(),
                now + backoff * 1_000,
                failures,
                Some(&provider_error_code(&error)),
            )?;
            return Err(error);
        }
    };
    let Some(item) = feed.and_then(|value| value.latest) else {
        store.update_rss_poll(
            subscription.id,
            subscription.last_item_id.as_deref(),
            next(),
            0,
            None,
        )?;
        return Ok(());
    };
    if subscription.last_item_id.is_none() {
        store.update_rss_poll(subscription.id, Some(&item.id), next(), 0, None)?;
        return Ok(());
    }
    if subscription.last_item_id.as_deref() == Some(item.id.as_str()) {
        store.update_rss_poll(subscription.id, Some(&item.id), next(), 0, None)?;
        return Ok(());
    }
    let content = format_rss_message(&subscription.message_template, &subscription.mention, &item);
    let channel_id = subscription
        .target_channel_id
        .parse::<u64>()
        .map_err(|_| anyhow::anyhow!("invalid_discord_channel_id"))?;
    ChannelId::new(channel_id).say(http, content).await?;
    store.update_rss_poll(subscription.id, Some(&item.id), next(), 0, None)?;
    Ok(())
}

fn format_rss_message(template: &str, mention: &str, item: &RssItem) -> String {
    let rendered = template
        .replace("{feed}", &item.feed_title)
        .replace("{title}", &item.title)
        .replace("{url}", &item.url)
        .replace("{published_at}", &item.published_at)
        .replace("{description}", &item.description);
    let rendered = if mention.is_empty() {
        rendered
    } else {
        format!("{mention} {rendered}")
    };
    rendered.chars().take(2_000).collect()
}

async fn run_twitch_worker(http: Arc<serenity::http::Http>, store: Store) {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;
        let due = match store.due_twitch_subscriptions(Utc::now().timestamp_millis(), 25) {
            Ok(rows) => rows,
            Err(error) => {
                tracing::error!(%error, "twitch worker could not load subscriptions");
                continue;
            }
        };
        for subscription in due {
            if !feature_enabled(&store, &subscription.guild_id, "social.twitch", None) {
                if let Some(event_id) = subscription.pending_event_id.as_deref() {
                    let _ = store.ack_twitch_event(
                        subscription.id,
                        event_id,
                        i64::MAX,
                        subscription.failure_count,
                        Some("feature_disabled"),
                    );
                }
                continue;
            }
            if let Err(error) = process_twitch_subscription(&http, &store, &subscription).await {
                tracing::warn!(%error, subscription_id = subscription.id, "twitch notification failed");
            }
        }
    }
}

async fn process_twitch_subscription(
    http: &serenity::http::Http,
    store: &Store,
    subscription: &TwitchSubscriptionRecord,
) -> Result<()> {
    let Some(event_id) = subscription.pending_event_id.as_deref() else {
        return Ok(());
    };
    let content = format_twitch_message(
        &subscription.message_template,
        &subscription.mention,
        &subscription.source_login,
        subscription
            .pending_stream_id
            .as_deref()
            .unwrap_or_default(),
        subscription
            .pending_started_at
            .as_deref()
            .unwrap_or_default(),
    );
    let channel_id = subscription
        .target_channel_id
        .parse::<u64>()
        .map_err(|_| anyhow::anyhow!("invalid_discord_channel_id"))?;
    if let Err(error) = ChannelId::new(channel_id).say(http, content).await {
        let failures = subscription.failure_count.saturating_add(1).min(8);
        let backoff = 5_i64 * (1_i64 << failures.min(6));
        store.retry_twitch_event(
            subscription.id,
            event_id,
            Utc::now().timestamp_millis() + backoff * 1_000,
            failures,
            Some("discord_delivery_failed"),
        )?;
        return Err(error.into());
    }
    store.ack_twitch_event(subscription.id, event_id, i64::MAX, 0, None)?;
    Ok(())
}

fn format_twitch_message(
    template: &str,
    mention: &str,
    login: &str,
    stream_id: &str,
    started_at: &str,
) -> String {
    let url = format!("https://twitch.tv/{login}");
    let rendered = template
        .replace("{broadcaster}", login)
        .replace("{login}", login)
        .replace("{stream_id}", stream_id)
        .replace("{started_at}", started_at)
        .replace("{url}", &url);
    let rendered = if mention.is_empty() {
        rendered
    } else {
        format!("{mention} {rendered}")
    };
    rendered.chars().take(2_000).collect()
}

impl Handler {
    async fn apply_level_rewards(
        &self,
        ctx: &Context,
        guild_id: serenity::all::GuildId,
        user_id: &str,
        level: i64,
    ) {
        let mut rewards = setting_string(
            &self.store,
            &guild_id.to_string(),
            "community.levels.level_roles",
        )
        .unwrap_or_default()
        .split(',')
        .filter_map(|entry| {
            let (raw_level, raw_role) = entry.trim().split_once('=')?;
            Some((
                raw_level.trim().parse::<i64>().ok()?,
                raw_role.trim().parse::<u64>().ok()?,
            ))
        })
        .filter(|(reward_level, _)| *reward_level > 0 && *reward_level <= level)
        .collect::<Vec<_>>();
        if rewards.is_empty() {
            return;
        }
        rewards.sort_unstable_by_key(|(reward_level, _)| *reward_level);
        let Ok(user_id) = user_id.parse::<u64>() else {
            return;
        };
        let Ok(member) = guild_id
            .member(&ctx.http, serenity::all::UserId::new(user_id))
            .await
        else {
            return;
        };
        let stack_roles = setting_bool(
            &self.store,
            &guild_id.to_string(),
            "community.levels.stack_roles",
            true,
        );
        let desired = if stack_roles {
            rewards
                .iter()
                .map(|(_, role_id)| *role_id)
                .collect::<Vec<_>>()
        } else {
            vec![
                rewards
                    .last()
                    .map(|(_, role_id)| *role_id)
                    .unwrap_or_default(),
            ]
        };
        if !stack_roles {
            for (_, role_id) in &rewards {
                if !desired.contains(role_id) {
                    let _ = member.remove_role(&ctx.http, RoleId::new(*role_id)).await;
                }
            }
        }
        for role_id in desired {
            if role_id != 0
                && !member
                    .roles
                    .iter()
                    .any(|existing| existing.get() == role_id)
            {
                let _ = member.add_role(&ctx.http, RoleId::new(role_id)).await;
            }
        }
    }

    async fn effective_plan(&self, user_id: &str, guild_id: Option<&str>) -> Plan {
        let Some(client) = &self.entitlements else {
            return Plan::Free;
        };
        match client.resolve(user_id, guild_id).await {
            Ok(snapshot)
                if snapshot.active
                    && snapshot
                        .expires_at
                        .is_none_or(|expires_at| expires_at > Utc::now()) =>
            {
                snapshot.plan
            }
            Ok(_) | Err(_) => Plan::Free,
        }
    }

    async fn send_rank_card(&self, ctx: &Context, command: &CommandInteraction) -> Result<()> {
        let Some(guild_id) = command.guild_id else {
            return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
        };
        if !feature_enabled(&self.store, &guild_id.to_string(), "studio.rank_card", None) {
            return respond(
                ctx,
                command,
                "O XP card está desativado neste servidor. Ativa-o no painel primeiro.",
            )
            .await;
        }
        let user_id = command
            .data
            .options
            .iter()
            .find_map(|option| match option.value {
                CommandDataOptionValue::User(user) => Some(user),
                _ => None,
            })
            .unwrap_or(command.user.id);
        let profile = if user_id == command.user.id {
            command.user.clone()
        } else {
            ctx.http.get_user(user_id).await?
        };
        let guild_text = guild_id.to_string();
        let user_text = user_id.to_string();
        let xp = self.store.level_for(&guild_text, &user_text)?;
        let level = (xp / 100) + 1;
        let rank = self.store.level_rank(&guild_text, &user_text)?;
        let config =
            rank_card::parse_config(self.store.get_setting(&guild_text, "community.rank_card")?);
        let avatar_url = profile.face();
        let svg =
            rank_card::render_rank_card(&config, &profile.name, Some(&avatar_url), rank, level, xp);
        command
            .create_response(
                ctx,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content(format!("{} · level {} · {} XP", profile.name, level, xp))
                        .add_file(CreateAttachment::bytes(svg.into_bytes(), "rank-card.svg")),
                ),
            )
            .await?;
        Ok(())
    }

    async fn handle_command(&self, ctx: &Context, command: &CommandInteraction) -> Result<()> {
        if let Some(required) = required_permission(command.data.name.as_str()) {
            let permissions = command
                .member
                .as_deref()
                .and_then(|member| member.permissions)
                .unwrap_or_else(Permissions::empty);
            if !permissions.contains(required) && !permissions.contains(Permissions::ADMINISTRATOR)
            {
                return respond(
                    ctx,
                    command,
                    "Não tens a permissão necessária para este comando.",
                )
                .await;
            }
        }
        if let Some(guild_id) = command.guild_id
            && is_moderation_command(command.data.name.as_str())
            && feature_explicitly_disabled(
                &self.store,
                &guild_id.to_string(),
                "management.moderation",
            )
        {
            return respond(
                ctx,
                command,
                "Manual moderation is disabled in this server. Enable Moderation in the dashboard first.",
            )
            .await;
        }
        if command.data.name == "embed" {
            let Some(guild_id) = command.guild_id else {
                return respond(ctx, command, "This command can only be used in a server.").await;
            };
            let guild_text = guild_id.to_string();
            if !feature_enabled(&self.store, &guild_text, "utility.embeds", None) {
                return respond(
                    ctx,
                    command,
                    "Embeds are disabled in this server. Enable them in the dashboard.",
                )
                .await;
            }
            let title = option_string(command, "title")
                .unwrap_or_default()
                .trim()
                .to_string();
            let max_description = setting_u64(
                &self.store,
                &guild_text,
                "utility.embeds.max_description",
                2_000,
            )
            .clamp(1, 4_000) as usize;
            let description = option_string(command, "description")
                .unwrap_or_default()
                .trim()
                .to_string();
            if title.is_empty()
                || title.chars().count() > 256
                || description.is_empty()
                || description.chars().count() > max_description
            {
                return respond(
                    ctx,
                    command,
                    "The title or description is outside the configured limits.",
                )
                .await;
            }
            command
                .channel_id
                .send_message(
                    &ctx.http,
                    CreateMessage::new()
                        .embed(CreateEmbed::new().title(title).description(description)),
                )
                .await?;
            return respond(ctx, command, "Embed published.").await;
        }
        let content = match command.data.name.as_str() {
            "ping" => "Pong — Vozen Helper está online.".to_string(),
            "help" => {
                let guild_text = command.guild_id.map(|guild_id| guild_id.to_string());
                let show_modules = guild_text.as_deref().is_none_or(|guild_id| {
                    setting_bool(&self.store, guild_id, "utility.help.show_modules", true)
                });
                let show_dashboard = guild_text.as_deref().is_none_or(|guild_id| {
                    setting_bool(&self.store, guild_id, "utility.help.show_dashboard", true)
                });
                let mut message = "Vozen Helper: Core, Studio, Security, Support, Events, Community, Automate and Insights.".to_string();
                if show_modules {
                    message.push_str(" Use /modules to see what is enabled.");
                }
                if show_dashboard {
                    message.push_str(" Use /dashboard to configure your server.");
                }
                message
            }
            "setup" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let modules = ["security", "support", "events", "community", "automate", "insights"];
                let selected = modules
                    .iter()
                    .filter_map(|module| {
                        option_bool(command, module)
                            .filter(|enabled| *enabled)
                            .map(|_| *module)
                    })
                    .collect::<Vec<_>>();
                for module in &selected {
                    self.store.set_setting(
                        &guild_id.to_string(),
                        &format!("core.module.{module}.enabled"),
                        "true",
                    )?;
                }
                self.store.set_setting(
                    &guild_id.to_string(),
                    "core.setup.completed",
                    if selected.is_empty() { "false" } else { "true" },
                )?;
                if selected.is_empty() {
                    "Setup guiado: escolhe pelo menos um módulo (Security, Support, Events, Community, Automate ou Insights) e executa novamente. Depois confirma as permissões com `/permissions`.".to_string()
                } else {
                    format!("Setup guardado para: **{}**. Próximo passo: `/permissions` e depois configura cada módulo.", selected.join(", "))
                }
            }
            "modules" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let modules = ["core", "studio", "security", "support", "events", "community", "automate", "insights"];
                modules
                    .iter()
                    .map(|module| {
                        let enabled = self
                            .store
                            .get_setting(&guild_id.to_string(), &format!("core.module.{module}.enabled"))
                            .ok()
                            .flatten()
                            .is_some_and(|value| value == "true")
                            || *module == "core";
                        format!("{} **{}**", if enabled { "✅" } else { "◻️" }, module)
                    })
                    .collect::<Vec<_>>()
                    .join(" · ")
            }
            "status" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let setup = self
                    .store
                    .get_setting(&guild_id.to_string(), "core.setup.completed")?
                    .is_some_and(|value| value == "true");
                let configured = ["security", "support", "events", "community", "automate", "insights"]
                    .iter()
                    .filter(|module| {
                        self.store
                            .get_setting(&guild_id.to_string(), &format!("core.module.{module}.enabled"))
                            .ok()
                            .flatten()
                            .is_some_and(|value| value == "true")
                    })
                    .copied()
                    .collect::<Vec<_>>();
                format!("Setup: **{}** · Módulos: **{}** · Isolamento por guild ativo · Privacy export/delete disponível", if setup { "concluído" } else { "pendente" }, if configured.is_empty() { "nenhum".to_string() } else { configured.join(", ") })
            }
            "dashboard" => "Painel: https://helper.vozen.org (o endpoint permanece desligado até o rollout aprovado).".to_string(),
            "plan" => {
                if let Some(client) = &self.entitlements {
                    match client.resolve(&command.user.id.to_string(), command.guild_id.map(|id| id.to_string()).as_deref()).await {
                        Ok(snapshot) => { let label = match &snapshot.plan { helper_contracts::Plan::Free => "Free", helper_contracts::Plan::Plus => "Plus", helper_contracts::Plan::Premium { .. } => "Premium" }; format!("Plano {label} · {} guild(s) · entitlements v{}.", snapshot.plan.guild_limit(), snapshot.version) },
                        Err(error) => { tracing::warn!(%error, "central entitlement lookup failed"); "Não foi possível consultar o plano agora; o Helper mantém o último snapshot seguro.".to_string() }
                    }
                } else { "Entitlements central ainda não estão configurados nesta instalação.".to_string() }
            }
            "privacy" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando sÃ³ pode ser usado num servidor.").await;
                };
                let guild_text = guild_id.to_string();
                let allow_export = setting_bool(
                    &self.store,
                    &guild_text,
                    "management.privacy.allow_member_export",
                    true,
                );
                let allow_erase = setting_bool(
                    &self.store,
                    &guild_text,
                    "management.privacy.allow_member_erase",
                    true,
                );
                let max_export_bytes = setting_u64(
                    &self.store,
                    &guild_text,
                    "management.privacy.max_export_bytes",
                    1_000_000,
                )
                .clamp(65_536, 10_000_000) as usize;
                let subcommand = command.data.options.first().map(|option| option.name.as_str());
                match subcommand {
                    Some("erase") => {
                        if !allow_erase {
                            return respond(ctx, command, "Member erasure is disabled in this server.").await;
                        }
                        let result = self.store.purge_user(&guild_text, &command.user.id.to_string())?;
                        format!("Dados voluntÃ¡rios apagados. Registos de moderaÃ§Ã£o, infraÃ§Ãµes e quarantine foram mantidos por auditoria. Resultado: {result}")
                    }
                    _ => {
                        if !allow_export {
                            return respond(ctx, command, "Member data exports are disabled in this server.").await;
                        }
                        let export = self.store.export_user(&guild_text, &command.user.id.to_string())?;
                        let bytes = serde_json::to_vec_pretty(&export)?;
                        if bytes.len() > max_export_bytes {
                            return respond(ctx, command, "Your export is larger than this server's configured limit.").await;
                        }
                        let dm = command.user.create_dm_channel(&ctx.http).await;
                        match dm {
                            Ok(channel) => {
                                channel.send_message(
                                    &ctx.http,
                                    CreateMessage::new()
                                        .content("Here is your Vozen Helper data export.")
                                        .add_file(CreateAttachment::bytes(bytes, "my-vozen-data.json")),
                                ).await?;
                                "Enviei os teus dados por mensagem privada.".to_string()
                            }
                            Err(_) => "NÃ£o consegui enviar mensagem privada. Ativa as DMs e tenta novamente.".to_string(),
                        }
                    }
                }
            }
            "permissions" => permission_passport_message(),
            "cases" => {
                if let Some(guild_id) = command.guild_id {
                    let cases = self.store.recent_cases(&guild_id.to_string(), 10)?;
                    if cases.is_empty() { "Ainda não existem casos neste servidor.".to_string() } else { cases.into_iter().map(|case_record| format!("#{} {} <@{}>: {}", case_record.id, case_record.kind, case_record.target_id, case_record.reason)).collect::<Vec<_>>().join("\n") }
                } else { "Este comando só pode ser usado num servidor.".to_string() }
            }
            "modlogs" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando sÃ³ pode ser usado num servidor.").await;
                };
                let Some(target) = command.data.options.iter().find_map(|option| match option.value {
                    CommandDataOptionValue::User(user) => Some(user),
                    _ => None,
                }) else {
                    return respond(ctx, command, "Indica um membro.").await;
                };
                let cases = self.store.cases_for_target(&guild_id.to_string(), &target.to_string(), 50)?;
                if cases.is_empty() {
                    format!("NÃ£o existem casos para <@{}>.", target)
                } else {
                    cases.into_iter().map(|case_record| format!("#{} {}: {}", case_record.id, case_record.kind, case_record.reason)).collect::<Vec<_>>().join("\n")
                }
            }
            "warn" => {
                if let Some(guild_id) = command.guild_id {
                    let target = command.data.options.iter().find_map(|option| match &option.value { CommandDataOptionValue::User(user) => Some(*user), _ => None });
                    if let Some(target) = target {
                        let reason = command.data.options.iter().find_map(|option| match &option.value { CommandDataOptionValue::String(value) => Some(value.as_str()), _ => None }).unwrap_or("");
                        if setting_bool(&self.store, &guild_id.to_string(), "management.moderation.require_reason", true) && reason.trim().is_empty() {
                            return respond(ctx, command, "Provide a reason so the warning can be audited.").await;
                        }
                        let reason = if reason.trim().is_empty() { "No reason provided" } else { reason };
                        let case_id = self.store.record_case(&guild_id.to_string(), "warn", &target.to_string(), &command.user.id.to_string(), reason, None)?;
                        format!("Aviso criado como caso #{case_id} para <@{}>.", target)
                    } else { "Indica um membro.".to_string() }
                } else { "Este comando só pode ser usado num servidor.".to_string() }
            }
            "slowmode" => {
                let raw_seconds = command.data.options.iter().find_map(|option| match option.value {
                    CommandDataOptionValue::Integer(value) if option.name == "seconds" => Some(value),
                    _ => None,
                }).unwrap_or(0);
                let seconds = raw_seconds.clamp(0, 21_600) as u16;
                command
                    .channel_id
                    .edit(&ctx.http, EditChannel::new().rate_limit_per_user(seconds))
                    .await?;
                format!("Slowmode definido para {} segundos.", seconds)
            }
            "userinfo" => {
                let Some(user_id) = command.data.options.iter().find_map(|option| match option.value {
                    CommandDataOptionValue::User(user) => Some(user),
                    _ => None,
                }) else {
                    return respond(ctx, command, "Indica um utilizador.").await;
                };
                let user = ctx.http.get_user(user_id).await?;
                format!("{} (<@{}>) · conta criada em {}.", user.name, user.id, user.id.created_at().unix_timestamp())
            }
            "violation" | "note" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let Some(target) = command.data.options.iter().find_map(|option| match option.value {
                    CommandDataOptionValue::User(user) => Some(user),
                    _ => None,
                }) else {
                    return respond(ctx, command, "Indica um membro.").await;
                };
                let reason = if command.data.name == "violation" {
                    let rule = option_string(command, "rule").unwrap_or("unspecified");
                    let details = option_string(command, "details").unwrap_or("Sem detalhes");
                    format!("{rule}: {details}")
                } else {
                    option_string(command, "content").unwrap_or("Sem conteúdo").to_string()
                };
                if reason.len() > 500 {
                    return respond(ctx, command, "O conteúdo não pode exceder 500 caracteres.").await;
                }
                let case_id = self.store.record_case(&guild_id.to_string(), &command.data.name, &target.to_string(), &command.user.id.to_string(), &reason, None)?;
                format!("Registo #{case_id} criado para <@{}>.", target)
            }
            "reason" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let case_id = command.data.options.iter().find_map(|option| match option.value {
                    CommandDataOptionValue::Integer(value) if option.name == "case" => Some(value),
                    _ => None,
                }).unwrap_or(0);
                let reason = option_string(command, "reason").unwrap_or("Sem motivo");
                if self.store.update_case_reason(&guild_id.to_string(), case_id, reason)? {
                    format!("Motivo do caso #{case_id} atualizado.")
                } else {
                    "Caso não encontrado neste servidor.".to_string()
                }
            }
            "untimeout" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let Some(target) = command.data.options.iter().find_map(|option| match option.value {
                    CommandDataOptionValue::User(user) => Some(user),
                    _ => None,
                }) else {
                    return respond(ctx, command, "Indica um membro.").await;
                };
                let result = guild_id
                    .edit_member(&ctx.http, target, serenity::all::EditMember::new().enable_communication())
                    .await;
                match result {
                    Ok(_) => format!("Timeout removido para <@{}>.", target),
                    Err(error) => {
                        tracing::warn!(%error, "untimeout failed");
                        "Não foi possível remover o timeout; confirma as permissões.".to_string()
                    }
                }
            }
            "unban" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let Some(raw_user_id) = option_string(command, "user_id") else {
                    return respond(ctx, command, "Indica um ID de utilizador.").await;
                };
                let Ok(user_id) = raw_user_id.parse::<u64>() else {
                    return respond(ctx, command, "ID de utilizador inválido.").await;
                };
                match guild_id.unban(&ctx.http, serenity::all::UserId::new(user_id)).await {
                    Ok(()) => "Utilizador desbanido.".to_string(),
                    Err(error) => {
                        tracing::warn!(%error, "unban failed");
                        "Não foi possível remover o ban.".to_string()
                    }
                }
            }
            "purge" => {
                let raw_count = command.data.options.iter().find_map(|option| match option.value {
                    CommandDataOptionValue::Integer(value) if option.name == "count" => Some(value),
                    _ => None,
                }).unwrap_or(0);
                let max_purge = setting_u64(&self.store, &command.guild_id.map(|id| id.to_string()).unwrap_or_default(), "management.moderation.max_purge", 100).clamp(1, 100);
                let count = raw_count.clamp(1, max_purge as i64) as u8;
                let messages = command.channel_id.messages(&ctx.http, serenity::all::GetMessages::new().limit(count)).await?;
                if messages.is_empty() {
                    "Não encontrei mensagens para apagar.".to_string()
                } else {
                    let ids: Vec<_> = messages.iter().map(|message| message.id).collect();
                    command.channel_id.delete_messages(&ctx.http, ids).await?;
                    format!("{} mensagens apagadas.", messages.len())
                }
            }
            "tempban" | "softban" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando sÃ³ pode ser usado num servidor.").await;
                };
                let Some(target) = command.data.options.iter().find_map(|option| match option.value {
                    CommandDataOptionValue::User(user) => Some(user),
                    _ => None,
                }) else {
                    return respond(ctx, command, "Indica um membro.").await;
                };
                let reason = option_string(command, "reason").unwrap_or("");
                if setting_bool(&self.store, &guild_id.to_string(), "management.moderation.require_reason", true) && reason.trim().is_empty() {
                    return respond(ctx, command, "Provide a reason so the action can be audited.").await;
                }
                let reason = if reason.trim().is_empty() { "No reason provided" } else { reason };
                let action = guild_id.ban_with_reason(&ctx.http, target, 0, reason).await;
                if let Err(error) = action {
                    tracing::warn!(%error, action = %command.data.name, "ban action failed");
                    return respond(ctx, command, "Nao foi possivel executar o ban; confirma as permissoes e a hierarquia.").await;
                }
                if command.data.name == "softban" {
                    let _ = guild_id.unban(&ctx.http, target).await;
                    let case_id = self.store.record_case(&guild_id.to_string(), "softban", &target.to_string(), &command.user.id.to_string(), reason, None)?;
                    format!("Softban concluÃ­do como caso #{case_id} para <@{}>.", target)
                } else {
                    let Some(delay) = parse_duration(option_string(command, "duration").unwrap_or_default()) else {
                        let _ = guild_id.unban(&ctx.http, target).await;
                        return respond(ctx, command, "DuraÃ§Ã£o invÃ¡lida. Usa 10m, 2h ou 1d.").await;
                    };
                    let case_id = self.store.record_case(&guild_id.to_string(), "tempban", &target.to_string(), &command.user.id.to_string(), reason, Some(delay))?;
                    self.store.schedule_typed(&guild_id.to_string(), "unban", &target.to_string(), chrono::Utc::now().timestamp_millis() + delay, "")?;
                    format!("Tempban concluÃ­do como caso #{case_id} para <@{}>; expira em {}.", target, format_duration(delay))
                }
            }
            "kick" | "ban" | "timeout" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let Some(target) = command.data.options.iter().find_map(|option| match &option.value {
                    CommandDataOptionValue::User(user) => Some(*user),
                    _ => None,
                }) else {
                    return respond(ctx, command, "Indica um membro.").await;
                };
                let reason = command.data.options.iter().find_map(|option| match &option.value {
                    CommandDataOptionValue::String(value) => Some(value.as_str()),
                    _ => None,
                }).unwrap_or("");
                if setting_bool(&self.store, &guild_id.to_string(), "management.moderation.require_reason", true) && reason.trim().is_empty() {
                    return respond(ctx, command, "Provide a reason so the action can be audited.").await;
                }
                let reason = if reason.trim().is_empty() { "No reason provided" } else { reason };
                let action = if command.data.name == "kick" {
                    guild_id.kick_with_reason(&ctx.http, target, reason).await
                } else if command.data.name == "ban" {
                    guild_id.ban_with_reason(&ctx.http, target, 0, reason).await
                } else {
                    let seconds = command.data.options.iter().find_map(|option| match option.value {
                        CommandDataOptionValue::Integer(value) => Some(value),
                        _ => None,
                    }).unwrap_or(600).clamp(1, 28 * 24 * 60 * 60);
                    let until = (chrono::Utc::now() + chrono::Duration::seconds(seconds)).to_rfc3339();
                    guild_id.edit_member(&ctx.http, target, serenity::all::EditMember::new().disable_communication_until(until)).await.map(|_| ())
                };
                match action {
                    Ok(()) => {
                        let case_id = self.store.record_case(&guild_id.to_string(), &command.data.name, &target.to_string(), &command.user.id.to_string(), reason, None)?;
                        format!("Ação {} concluída como caso #{case_id} para <@{}>.", command.data.name, target)
                    }
                    Err(error) => {
                        tracing::warn!(%error, action = %command.data.name, "discord moderation action failed");
                        "Não foi possível executar a ação; confirma as permissões e a hierarquia de cargos.".to_string()
                    }
                }
            }
            "afk" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let reason = command.data.options.iter().find_map(|option| match &option.value {
                    CommandDataOptionValue::String(value) => Some(value.as_str()),
                    _ => None,
                });
                if let Some(reason) = reason.filter(|value| !value.trim().is_empty()) {
                    self.store.set_afk(&guild_id.to_string(), &command.user.id.to_string(), reason)?;
                    format!("AFK definido: {reason}")
                } else if self.store.clear_afk(&guild_id.to_string(), &command.user.id.to_string())? {
                    "AFK removido.".to_string()
                } else {
                    "Não tinhas AFK definido.".to_string()
                }
            }
            "remind" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let guild_text = guild_id.to_string();
                if !feature_enabled(&self.store, &guild_text, "utility.reminders", None) {
                    return respond(
                        ctx,
                        command,
                        "Reminders are disabled in this server. Enable them in the dashboard.",
                    )
                    .await;
                }
                let time = option_string(command, "time").unwrap_or_default();
                let text = option_string(command, "text").unwrap_or_default();
                let Some(delay) = parse_duration(time) else {
                    return respond(ctx, command, "Duração inválida. Usa formatos como 10m, 2h ou 1d.").await;
                };
                let max_delay_hours = setting_u64(
                    &self.store,
                    &guild_text,
                    "utility.reminders.max_delay_hours",
                    168,
                )
                .clamp(1, 8_760);
                let max_delay_ms = max_delay_hours
                    .saturating_mul(3_600_000)
                    .min(i64::MAX as u64) as i64;
                if delay > max_delay_ms {
                    return respond(
                        ctx,
                        command,
                        "That reminder is beyond the server's configured maximum delay.",
                    )
                    .await;
                }
                let max_text_length = setting_u64(
                    &self.store,
                    &guild_text,
                    "utility.reminders.max_text_length",
                    500,
                )
                .clamp(50, 500) as usize;
                if text.len() > max_text_length {
                    return respond(
                        ctx,
                        command,
                        "The reminder is longer than the server's configured limit.",
                    )
                    .await;
                }
                let id = self.store.schedule(
                    &guild_text,
                    &command.user.id.to_string(),
                    chrono::Utc::now().timestamp_millis() + delay,
                    &serde_json::json!({"channel_id": command.channel_id.to_string(), "text": text}).to_string(),
                )?;
                format!("Lembrete #{id} agendado.")
            }
            "birthday-set" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "This command can only be used in a server.").await;
                };
                let guild_text = guild_id.to_string();
                if !feature_enabled(&self.store, &guild_text, "community.birthdays", None) {
                    return respond(ctx, command, "Birthdays are disabled in this server. Enable them in the dashboard.").await;
                }
                let month = option_i64(command, "month").unwrap_or_default();
                let day = option_i64(command, "day").unwrap_or_default();
                if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
                    return respond(ctx, command, "Use a month from 1-12 and a day from 1-31.").await;
                }
                self.store.set_birthday(&guild_text, &command.user.id.to_string(), month as u32, day as u32)?;
                "Birthday saved privately as day and month. You can remove it at any time.".to_string()
            }
            "birthday-remove" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "This command can only be used in a server.").await;
                };
                let guild_text = guild_id.to_string();
                if !feature_enabled(&self.store, &guild_text, "community.birthdays", None) {
                    return respond(ctx, command, "Birthdays are disabled in this server. Enable them in the dashboard.").await;
                }
                if self.store.remove_birthday(&guild_text, &command.user.id.to_string())? {
                    "Your birthday was removed.".to_string()
                } else {
                    "You do not have a birthday saved in this server.".to_string()
                }
            }
            "tag" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                if !feature_enabled(&self.store, &guild_id.to_string(), "management.custom_commands", None) {
                    return respond(ctx, command, "Custom commands are disabled in this server. Enable them in the dashboard.").await;
                }
                let name = option_string(command, "name").unwrap_or_default().to_lowercase();
                match self.store.get_tag(&guild_id.to_string(), &name)? {
                    Some(tag) => tag.content.replace("{user}", &format!("<@{}>", command.user.id)),
                    None => "Tag não encontrada.".to_string(),
                }
            }
            "tags" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                if !feature_enabled(&self.store, &guild_id.to_string(), "management.custom_commands", None) {
                    return respond(ctx, command, "Custom commands are disabled in this server. Enable them in the dashboard.").await;
                }
                let max_tags = setting_u64(&self.store, &guild_id.to_string(), "management.custom_commands.max_tags", 100).clamp(1, 100) as u32;
                let names = self.store.list_tags(&guild_id.to_string(), max_tags)?;
                if names.is_empty() { "Ainda não existem tags.".to_string() } else { names.join(", ") }
            }
            "tag-set" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let guild_text = guild_id.to_string();
                if !feature_enabled(&self.store, &guild_text, "management.custom_commands", None) {
                    return respond(ctx, command, "Custom commands are disabled in this server. Enable them in the dashboard.").await;
                }
                let name = option_string(command, "name").unwrap_or_default().trim().to_lowercase();
                let content = option_string(command, "content").unwrap_or_default();
                let max_response_length = setting_u64(&self.store, &guild_text, "management.custom_commands.max_response_length", 1_000).clamp(1, 2_000) as usize;
                if !(1..=32).contains(&name.len()) || content.is_empty() || content.len() > max_response_length {
                    return respond(ctx, command, "Nome ou conteúdo inválido.").await;
                }
                let existing = self.store.get_tag(&guild_text, &name)?.is_some();
                let max_tags = setting_u64(&self.store, &guild_text, "management.custom_commands.max_tags", 100).clamp(1, 100) as u32;
                if !existing && self.store.list_tags(&guild_text, max_tags.saturating_add(1))?.len() as u64 >= max_tags as u64 {
                    return respond(ctx, command, "This server has reached its custom command limit.").await;
                }
                self.store.upsert_tag(&guild_text, &name, content, &command.user.id.to_string())?;
                format!("Tag `{name}` guardada.")
            }
            "tag-delete" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let guild_text = guild_id.to_string();
                if !feature_enabled(&self.store, &guild_text, "management.custom_commands", None) {
                    return respond(ctx, command, "Custom commands are disabled in this server. Enable them in the dashboard.").await;
                }
                let name = option_string(command, "name").unwrap_or_default().trim().to_lowercase();
                if self.store.delete_tag(&guild_text, &name)? { format!("Tag `{name}` eliminada.") } else { "Tag não encontrada.".to_string() }
            }
            "rank" => return self.send_rank_card(ctx, command).await,
            "leaderboard" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let guild_text = guild_id.to_string();
                if !feature_enabled(&self.store, &guild_text, "community.leaderboard", None) {
                    return respond(
                        ctx,
                        command,
                        "The XP leaderboard is disabled in this server. Enable it in the dashboard.",
                    )
                    .await;
                }
                let max_entries = setting_u64(
                    &self.store,
                    &guild_text,
                    "community.leaderboard.max_entries",
                    10,
                )
                .clamp(1, 100) as u32;
                let rows = self.store.top_levels(&guild_text, max_entries)?;
                if rows.is_empty() { "Ainda não existem dados de XP.".to_string() } else { rows.into_iter().enumerate().map(|(index, row)| format!("{}. <@{}> — {} XP", index + 1, row.user_id, row.xp)).collect::<Vec<_>>().join("\n") }
            }
            "achievements" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "This command can only be used in a server.").await;
                };
                let guild_text = guild_id.to_string();
                if !feature_enabled(&self.store, &guild_text, "community.achievements", None) {
                    return respond(ctx, command, "Achievements are disabled in this server. Enable them in the dashboard.").await;
                }
                let xp = self.store.level_for(&guild_text, &command.user.id.to_string())?;
                let mut achievements = Vec::new();
                for (threshold, label) in [(100_i64, "First steps"), (1_000, "Regular"), (10_000, "Community pillar")] {
                    if xp >= threshold { achievements.push(format!("✅ {label} ({threshold} XP)")); }
                }
                if achievements.is_empty() { "No achievements yet. Keep participating to unlock the first one at 100 XP.".to_string() } else { achievements.join("\n") }
            }
            "serverstats" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let guild_text = guild_id.to_string();
                if !feature_enabled(&self.store, &guild_text, "insights.stats", None) {
                    return respond(
                        ctx,
                        command,
                        "Server statistics are disabled in this server. Enable them in the dashboard.",
                    )
                    .await;
                }
                let window_days = setting_u64(
                    &self.store,
                    &guild_text,
                    "insights.stats.window_days",
                    7,
                )
                .clamp(1, 30) as u32;
                let rows = self.store.stats_for(&guild_text, window_days)?;
                let messages: i64 = rows.iter().map(|(_, messages, _, _)| messages).sum();
                format!("Mensagens registadas nos últimos {} dias: {}.", rows.len(), messages)
            }
            "emojis" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "This command can only be used in a server.").await;
                };
                let guild_text = guild_id.to_string();
                if !feature_enabled(&self.store, &guild_text, "utility.emojis", None) {
                    return respond(ctx, command, "Emoji inventory is disabled in this server. Enable it in the dashboard.").await;
                }
                let emojis = guild_id.emojis(&ctx.http).await?;
                if emojis.is_empty() { "No custom emojis are configured in this server.".to_string() } else {
                    emojis.into_iter().take(50).map(|emoji| format!("{} `<:{}:{}>`", if emoji.animated { "🎞️" } else { "😀" }, emoji.name, emoji.id)).collect::<Vec<_>>().join("\n")
                }
            }
            "invites" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "This command can only be used in a server.").await;
                };
                let guild_text = guild_id.to_string();
                if !feature_enabled(&self.store, &guild_text, "management.invite_tracker", None) {
                    return respond(ctx, command, "Invite tracking is disabled in this server. Enable it in the dashboard.").await;
                }
                let mut invites = guild_id.invites(&ctx.http).await?;
                invites.sort_by_key(|invite| std::cmp::Reverse(invite.uses));
                if invites.is_empty() { "No active invites were found. The bot needs Manage Server to read them.".to_string() } else {
                    invites
                        .into_iter()
                        .take(10)
                        .map(|invite| {
                            let inviter = invite
                                .inviter
                                .map(|user| user.name)
                                .unwrap_or_else(|| "Discord system".into());
                            format!("`{}` — {} uses — {}", invite.code, invite.uses, inviter)
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            }
            "balance" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "This command can only be used in a server.").await;
                };
                let guild_text = guild_id.to_string();
                if !feature_enabled(&self.store, &guild_text, "community.economy", None) {
                    return respond(ctx, command, "Economy is disabled in this server. Enable it in the dashboard.").await;
                }
                let account = self.store.economy_account(&guild_text, &command.user.id.to_string())?;
                format!("Your balance is **{}** credits.", account.balance)
            }
            "daily" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "This command can only be used in a server.").await;
                };
                let guild_text = guild_id.to_string();
                if !feature_enabled(&self.store, &guild_text, "community.economy", None) {
                    return respond(ctx, command, "Economy is disabled in this server. Enable it in the dashboard.").await;
                }
                let reward = setting_u64(&self.store, &guild_text, "community.economy.daily_reward", 100).clamp(1, 10_000) as i64;
                match self.store.claim_daily(&guild_text, &command.user.id.to_string(), reward)? {
                    Some(account) => format!("Daily reward claimed: **{}** credits. Your balance is **{}**.", reward, account.balance),
                    None => "You already claimed your daily reward. Try again after the 24-hour cooldown.".to_string(),
                }
            }
            "temp-channel" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "This command can only be used in a server.").await;
                };
                let guild_text = guild_id.to_string();
                if !feature_enabled(&self.store, &guild_text, "utility.temp_channels", None) {
                    return respond(ctx, command, "Temporary channels are disabled in this server. Enable them in the dashboard.").await;
                }
                let channel = guild_id
                    .create_channel(
                        &ctx.http,
                        CreateChannel::new(format!("{}'s room", command.user.name))
                            .kind(serenity::all::ChannelType::Voice),
                    )
                    .await?;
                self.store.register_temp_channel(
                    &guild_text,
                    &channel.id.to_string(),
                    &command.user.id.to_string(),
                )?;
                format!("Temporary voice channel created: <#{}>. It will be removed when everyone leaves.", channel.id)
            }
            "starboard-set" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let Some(channel_id) = command.data.options.iter().find_map(|option| match option.value {
                    CommandDataOptionValue::Channel(channel) if option.name == "channel" => Some(channel),
                    _ => None,
                }) else {
                    return respond(ctx, command, "Indica um canal válido.").await;
                };
                self.store.set_setting(&guild_id.to_string(), "community.starboard.channel_id", &channel_id.to_string())?;
                format!("Starboard configurado no canal <#{}>. Requer 3 ⭐ para publicar.", channel_id)
            }
            "suggest" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                if !feature_enabled(&self.store, &guild_id.to_string(), "community.suggestions", None) {
                    return respond(ctx, command, "As sugestões estão desativadas neste servidor. Ativa-as no painel.").await;
                }
                let text = option_string(command, "text").unwrap_or_default().trim();
                if !(3..=1_000).contains(&text.len()) {
                    return respond(ctx, command, "A sugestão deve ter entre 3 e 1000 caracteres.").await;
                }
                let id = self.store.create_suggestion(&guild_id.to_string(), &command.user.id.to_string(), text)?;
                let message = command.channel_id.send_message(&ctx.http, serenity::all::CreateMessage::new()
                    .content(format!("**Suggestion #{id}** by <@{}>\n{text}\n\nVote on this suggestion:", command.user.id))
                    .components(vec![CreateActionRow::Buttons(vec![
                        CreateButton::new(format!("suggest:up:{id}")).label("Support").style(ButtonStyle::Success),
                        CreateButton::new(format!("suggest:down:{id}")).label("Against").style(ButtonStyle::Danger),
                    ])])).await?;
                self.store.set_suggestion_message(id, &message.id.to_string())?;
                format!("Sugestão #{id} publicada.")
            }
            "suggestion" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                if !feature_enabled(&self.store, &guild_id.to_string(), "community.suggestions", None) {
                    return respond(ctx, command, "As sugestões estão desativadas neste servidor. Ativa-as no painel.").await;
                }
                let id = option_i64(command, "id").unwrap_or(0);
                let status = option_string(command, "status").unwrap_or_default().to_ascii_lowercase();
                if !matches!(status.as_str(), "pending" | "approved" | "denied" | "considered") {
                    return respond(ctx, command, "Estado inválido: pending, approved, denied ou considered.").await;
                }
                if self.store.set_suggestion_status(&guild_id.to_string(), id, &status)? {
                    format!("Sugestão #{id} marcada como {status}.")
                } else {
                    "Sugestão não encontrada neste servidor.".to_string()
                }
            }
            "giveaway-start" | "gstart" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                if !feature_enabled(&self.store, &guild_id.to_string(), "community.giveaways", None) {
                    return respond(ctx, command, "Os giveaways estão desativados neste servidor. Ativa-os no painel.").await;
                }
                let prize = option_string(command, "prize").unwrap_or_default().trim();
                let Some(delay) = parse_duration(option_string(command, "duration").unwrap_or_default()) else {
                    return respond(ctx, command, "Duração inválida. Usa 10m, 2h ou 1d.").await;
                };
                if prize.is_empty() || prize.len() > 200 {
                    return respond(ctx, command, "O prémio deve ter entre 1 e 200 caracteres.").await;
                }
                let winners = option_i64(command, "winners").unwrap_or(1).clamp(1, 20);
                let required_role = command.data.options.iter().find_map(|option| match option.value {
                    CommandDataOptionValue::Role(role) if option.name == "required_role" => Some(role.to_string()),
                    _ => None,
                });
                let end_at = chrono::Utc::now().timestamp_millis() + delay;
                let id = self.store.create_giveaway(&guild_id.to_string(), &command.channel_id.to_string(), prize, winners, end_at, required_role.as_deref(), &command.user.id.to_string())?;
                let message = command.channel_id.send_message(&ctx.http, serenity::all::CreateMessage::new()
                    .content(format!("🎉 **Giveaway #{id}**\nPrize: **{prize}**\nWinners: **{winners}**\nEnds <t:{}:R>\nClick the button to join.", end_at / 1_000))
                    .components(vec![CreateActionRow::Buttons(vec![CreateButton::new(format!("giveaway:join:{id}")).label("Join").style(ButtonStyle::Primary)])])).await?;
                self.store.set_giveaway_message(id, &message.id.to_string())?;
                self.store.schedule_typed(&guild_id.to_string(), "giveaway_end", &command.user.id.to_string(), end_at, &serde_json::json!({"channel_id": command.channel_id.to_string(), "giveaway_id": id}).to_string())?;
                format!("Giveaway #{id} criado.")
            }
            "giveaway-end" | "gend" => {
                let id = option_i64(command, "id").unwrap_or(0);
                if finish_giveaway(&ctx.http, &self.store, id).await? {
                    format!("Giveaway #{id} terminado.")
                } else {
                    "Giveaway não encontrado ou já terminado.".to_string()
                }
            }
            "giveaway-list" | "glist" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                if !feature_enabled(&self.store, &guild_id.to_string(), "community.giveaways", None) {
                    return respond(ctx, command, "Os giveaways estão desativados neste servidor. Ativa-os no painel.").await;
                }
                let rows = self.store.active_giveaways(&guild_id.to_string(), 20)?;
                if rows.is_empty() { "Não existem giveaways ativos.".to_string() } else {
                    rows.into_iter().map(|row| format!("#{} — {} — termina <t:{}:R>", row.id, row.prize, row.end_at / 1_000)).collect::<Vec<_>>().join("\n")
                }
            }
            "greroll" => {
                let id = option_i64(command, "id").unwrap_or(0);
                match reroll_giveaway(&ctx.http, &self.store, id).await? {
                    Some(winner) => format!("Giveaway #{id} rerolled: <@{winner}>."),
                    None => "Giveaway nÃ£o encontrado, ainda ativo ou sem participantes.".to_string(),
                }
            }
            "poll" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                if !feature_enabled(&self.store, &guild_id.to_string(), "management.polls", None) {
                    return respond(ctx, command, "As enquetes estão desativadas neste servidor. Ativa-as no painel.").await;
                }
                let question = option_string(command, "question").unwrap_or_default().trim();
                let mut options = Vec::new();
                for name in ["option1", "option2", "option3", "option4", "option5"] {
                    if let Some(value) = option_string(command, name).map(str::trim).filter(|value| !value.is_empty()) {
                        options.push(value.to_string());
                    }
                }
                if question.is_empty() || options.len() < 2 {
                    return respond(ctx, command, "Indica uma pergunta e pelo menos duas opções.").await;
                }
                let delay = parse_duration(option_string(command, "duration").unwrap_or("1d")).unwrap_or(86_400_000);
                let end_at = chrono::Utc::now().timestamp_millis() + delay;
                let id = self.store.create_poll(&guild_id.to_string(), &command.channel_id.to_string(), question, &options, end_at)?;
                let labels = options.iter().enumerate().map(|(index, value)| CreateButton::new(format!("poll:{id}:{index}")).label(format!("{}: {}", index + 1, truncate(value, 70))).style(ButtonStyle::Secondary)).collect::<Vec<_>>();
                let message = command.channel_id.send_message(&ctx.http, serenity::all::CreateMessage::new()
                    .content(format!("🗳️ **Poll #{id}: {question}**\n{}\nEnds <t:{}:R>", options.iter().enumerate().map(|(i, v)| format!("{}️⃣ {}", i + 1, v)).collect::<Vec<_>>().join("\n"), end_at / 1_000))
                    .components(vec![CreateActionRow::Buttons(labels)])).await?;
                self.store.set_poll_message(id, &message.id.to_string())?;
                self.store.schedule_typed(&guild_id.to_string(), "poll_end", &command.user.id.to_string(), end_at, &serde_json::json!({"channel_id": command.channel_id.to_string(), "poll_id": id}).to_string())?;
                format!("Poll #{id} criada.")
            }
            "quarantine" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let Some(target) = command.data.options.iter().find_map(|option| match option.value {
                    CommandDataOptionValue::User(user) if option.name == "user" => Some(user),
                    _ => None,
                }) else {
                    return respond(ctx, command, "Indica um membro.").await;
                };
                let member = guild_id.member(&ctx.http, target).await?;
                let role_ids = member.roles.iter().map(|role| role.to_string()).collect::<Vec<_>>();
                let reason = option_string(command, "reason").unwrap_or("Quarantine manual");
                self.store.save_quarantine(&guild_id.to_string(), &target.to_string(), &role_ids, reason)?;
                for role in &member.roles {
                    let _ = member.remove_role(&ctx.http, *role).await;
                }
                let case_id = self.store.record_case(&guild_id.to_string(), "quarantine", &target.to_string(), &command.user.id.to_string(), reason, None)?;
                format!("<@{}> colocado em quarantine como caso #{case_id}. Os cargos foram guardados para restauro.", target)
            }
            "unquarantine" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let Some(target) = command.data.options.iter().find_map(|option| match option.value {
                    CommandDataOptionValue::User(user) if option.name == "user" => Some(user),
                    _ => None,
                }) else {
                    return respond(ctx, command, "Indica um membro.").await;
                };
                let Some(record) = self.store.get_quarantine(&guild_id.to_string(), &target.to_string())? else {
                    return respond(ctx, command, "Esse membro não está em quarantine.").await;
                };
                let member = guild_id.member(&ctx.http, target).await?;
                let mut restored = 0;
                for raw_role in record.role_ids {
                    if let Ok(role_id) = raw_role.parse::<u64>()
                        && member.add_role(&ctx.http, RoleId::new(role_id)).await.is_ok()
                    {
                        restored += 1;
                    }
                }
                self.store.clear_quarantine(&guild_id.to_string(), &target.to_string())?;
                format!("Quarantine removida de <@{}>; {} cargo(s) restaurado(s).", target, restored)
            }
            "join-gate" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let enabled = option_bool(command, "enabled").unwrap_or(false);
                let minimum_age = option_i64(command, "min_age_days").unwrap_or(0).clamp(0, 365);
                self.store.set_setting(
                    &guild_id.to_string(),
                    "security.join_gate.enabled",
                    if enabled { "true" } else { "false" },
                )?;
                self.store.set_setting(
                    &guild_id.to_string(),
                    "security.join_gate.min_age_days",
                    &minimum_age.to_string(),
                )?;
                if let Some(role_id) = command.data.options.iter().find_map(|option| {
                    (option.name == "role").then_some(match option.value {
                        CommandDataOptionValue::Role(role) => role,
                        _ => return None,
                    })
                }) {
                    self.store.set_setting(
                        &guild_id.to_string(),
                        "security.join_gate.role_id",
                        &role_id.to_string(),
                    )?;
                }
                if enabled {
                    let role_note = if option_role(command, "role").is_some() {
                        "; cargo de verificação atualizado"
                    } else {
                        "; configura um cargo para restringir canais"
                    };
                    format!("Join gate ativado para contas com menos de {minimum_age} dia(s){role_note}.")
                } else {
                    "Join gate desativado; as definições guardadas podem ser reativadas.".to_string()
                }
            }
            "verify-panel" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando sÃ³ pode ser usado num servidor.").await;
                };
                let guild_text = guild_id.to_string();
                let enabled = self.store.get_setting(&guild_text, "security.join_gate.enabled")?
                    .is_some_and(|value| value == "true");
                let Some(role_id) = self.store.get_setting(&guild_text, "security.join_gate.role_id")? else {
                    return respond(ctx, command, "Configura primeiro `/join-gate` com um cargo de verificaÃ§Ã£o.").await;
                };
                if !enabled {
                    return respond(ctx, command, "Ativa primeiro o `/join-gate`; o painel nÃ£o deve ficar exposto enquanto o gate estÃ¡ desligado.").await;
                }
                command.channel_id.send_message(
                    &ctx.http,
                    serenity::all::CreateMessage::new()
                        .content("Click the button to receive the verified member role.")
                        .components(vec![CreateActionRow::Buttons(vec![
                            CreateButton::new(format!("verify:{guild_text}:{role_id}"))
                                .label("Verify")
                                .style(ButtonStyle::Success),
                        ])]),
                ).await?;
                "Painel de verificaÃ§Ã£o publicado neste canal.".to_string()
            }
            "lockdown" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando sÃ³ pode ser usado num servidor.").await;
                };
                let reason = option_string(command, "reason").unwrap_or("Lockdown manual");
                let count = apply_lockdown(&ctx.http, &self.store, guild_id, true).await?;
                self.store.set_setting(&guild_id.to_string(), "security.lockdown.reason", reason)?;
                format!("Lockdown aplicado em {count} canal(is) de texto. Motivo: {reason}.")
            }
            "unlock" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando sÃ³ pode ser usado num servidor.").await;
                };
                let count = apply_lockdown(&ctx.http, &self.store, guild_id, false).await?;
                format!("Lockdown removido de {count} canal(is); overwrites anteriores restaurados quando estavam guardados.")
            }
            "security-mode" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let shadow = option_bool(command, "shadow").unwrap_or(false);
                self.store.set_setting(
                    &guild_id.to_string(),
                    "security.shadow_mode",
                    if shadow { "true" } else { "false" },
                )?;
                if shadow {
                    "Shadow mode ativado: respostas anti-raid/anti-nuke ficam em observação, com casos e alertas, sem contenção automática.".to_string()
                } else {
                    "Shadow mode desativado: as respostas de segurança configuradas podem aplicar contenção limitada.".to_string()
                }
            }
            "anti-raid" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let enabled = option_bool(command, "enabled").unwrap_or(false);
                let joins = option_i64(command, "joins").unwrap_or(10).clamp(2, 100);
                let window_seconds = option_i64(command, "window_seconds").unwrap_or(10).clamp(3, 60);
                let guild_text = guild_id.to_string();
                self.store.set_setting(
                    &guild_text,
                    "security.anti_raid.enabled",
                    if enabled { "true" } else { "false" },
                )?;
                self.store.set_setting(
                    &guild_text,
                    "security.anti_raid.joins",
                    &joins.to_string(),
                )?;
                self.store.set_setting(
                    &guild_text,
                    "security.anti_raid.window_seconds",
                    &window_seconds.to_string(),
                )?;
                if enabled {
                    format!("Anti-raid ativado: {joins} entradas em {window_seconds}s ativam o join gate.")
                } else {
                    "Anti-raid desativado; nenhuma resposta automática a bursts de joins será aplicada.".to_string()
                }
            }
            "anti-nuke" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let enabled = option_bool(command, "enabled").unwrap_or(false);
                let actions = option_i64(command, "actions").unwrap_or(3).clamp(2, 25);
                let window_seconds = option_i64(command, "window_seconds").unwrap_or(10).clamp(3, 60);
                let guild_text = guild_id.to_string();
                self.store.set_setting(
                    &guild_text,
                    "security.anti_nuke.enabled",
                    if enabled { "true" } else { "false" },
                )?;
                self.store.set_setting(
                    &guild_text,
                    "security.anti_nuke.actions",
                    &actions.to_string(),
                )?;
                self.store.set_setting(
                    &guild_text,
                    "security.anti_nuke.window_seconds",
                    &window_seconds.to_string(),
                )?;
                self.store.set_setting(
                    &guild_text,
                    "feature.management.audit",
                    if enabled { "true" } else { "false" },
                )?;
                if enabled {
                    format!("Anti-nuke ativado: {actions} ações destrutivas em {window_seconds}s ativam contenção e alerta.")
                } else {
                    "Anti-nuke desativado; os eventos de Audit Log não ativam contenção automática.".to_string()
                }
            }
            "event-create" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                if !feature_enabled(&self.store, &guild_id.to_string(), "community.events", None) {
                    return respond(ctx, command, "Os eventos estão desativados neste servidor. Ativa-os no painel.").await;
                }
                let name = option_string(command, "name").unwrap_or_default().trim();
                let start_raw = option_string(command, "start").unwrap_or_default().trim();
                let end_raw = option_string(command, "end").unwrap_or_default().trim();
                let location = option_string(command, "location").unwrap_or_default().trim();
                let description = option_string(command, "description").unwrap_or_default().trim();
                let capacity = option_i64(command, "capacity");
                if let Some(capacity) = capacity
                    && !(1..=100_000).contains(&capacity)
                {
                    return respond(ctx, command, "A lotação deve estar entre 1 e 100000.").await;
                }
                if !(1..=100).contains(&name.len())
                    || !(1..=100).contains(&location.len())
                    || description.len() > 1_000
                {
                    return respond(ctx, command, "Nome, local ou descrição inválidos.").await;
                }
                let (start, end) = match parse_scheduled_event_window(
                    start_raw,
                    end_raw,
                    serenity::all::Timestamp::now().unix_timestamp(),
                ) {
                    Ok(window) => window,
                    Err(reason) => {
                        let message = match reason {
                            "invalid_start" => "A data de início não é RFC3339 válida.",
                            "invalid_end" => "A data de fim não é RFC3339 válida.",
                            "start_must_be_in_future" => "O início tem de estar no futuro.",
                            "end_must_follow_start" => "O fim tem de ser depois do início.",
                            "event_too_long" => "O evento não pode durar mais de 365 dias.",
                            _ => "A janela do evento é inválida.",
                        };
                        return respond(ctx, command, message).await;
                    }
                };
                let mut builder = serenity::all::CreateScheduledEvent::new(
                    serenity::all::ScheduledEventType::External,
                    name.to_string(),
                    start,
                )
                .end_time(end)
                .location(location.to_string())
                .audit_log_reason("Vozen Helper event-create");
                if !description.is_empty() {
                    builder = builder.description(description.to_string());
                }
                let event = guild_id.create_scheduled_event(&ctx.http, builder).await?;
                if let Some(capacity) = capacity {
                    self.store.set_setting(
                        &guild_id.to_string(),
                        &format!("events.capacity.{}", event.id),
                        &capacity.to_string(),
                    )?;
                }
                format!(
                    "Evento nativo #{} criado: **{}** (<t:{}:F>–<t:{}:F>).",
                    event.id,
                    event.name,
                    start.unix_timestamp(),
                    end.unix_timestamp()
                )
            }
            "event-list" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let events = guild_id.scheduled_events(&ctx.http, false).await?;
                if events.is_empty() {
                    "Não existem eventos agendados neste servidor.".to_string()
                } else {
                    events
                        .into_iter()
                        .take(25)
                        .map(|event| {
                            format!(
                                "#{} **{}** · {:?} · <t:{}:F>",
                                event.id,
                                event.name,
                                event.status,
                                event.start_time.unix_timestamp()
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            }
            "event-edit" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                if !feature_enabled(&self.store, &guild_id.to_string(), "community.events", None) {
                    return respond(ctx, command, "Os eventos estão desativados neste servidor. Ativa-os no painel.").await;
                }
                let event_id = option_i64(command, "event_id").unwrap_or(0);
                if event_id <= 0 {
                    return respond(ctx, command, "Indica um ID de evento válido.").await;
                }
                let event_id = serenity::all::ScheduledEventId::new(event_id as u64);
                let events = guild_id.scheduled_events(&ctx.http, false).await?;
                let Some(existing) = events.into_iter().find(|event| event.id == event_id) else {
                    return respond(ctx, command, "Não encontrei esse evento neste servidor.").await;
                };
                if matches!(
                    existing.status,
                    serenity::all::ScheduledEventStatus::Completed
                        | serenity::all::ScheduledEventStatus::Canceled
                ) {
                    return respond(ctx, command, "Eventos concluídos ou cancelados não podem ser editados.").await;
                }

                let name = option_string(command, "name").map(str::trim);
                if let Some(name) = name
                    && !(1..=100).contains(&name.len())
                {
                    return respond(ctx, command, "O nome tem de ter entre 1 e 100 caracteres.").await;
                }

                let location = option_string(command, "location").map(str::trim);
                if let Some(location) = location
                    && (existing.kind != serenity::all::ScheduledEventType::External
                        || !(1..=100).contains(&location.len()))
                {
                    return respond(ctx, command, "A localização só pode ser alterada em eventos externos e deve ter 1–100 caracteres.").await;
                }

                let description = option_string(command, "description").map(str::trim);
                if let Some(description) = description
                    && description.len() > 1_000
                {
                    return respond(ctx, command, "A descrição não pode exceder 1000 caracteres.").await;
                }

                let start_raw = option_string(command, "start").map(str::trim);
                let end_raw = option_string(command, "end").map(str::trim);
                let (start, end) = if start_raw.is_some() || end_raw.is_some() {
                    let start = match start_raw {
                        Some(raw) => serenity::all::Timestamp::parse(raw)
                            .map_err(|_| anyhow::anyhow!("A nova data de início não é RFC3339 válida."))?,
                        None => existing.start_time,
                    };
                    let end = match end_raw {
                        Some(raw) => serenity::all::Timestamp::parse(raw)
                            .map_err(|_| anyhow::anyhow!("A nova data de fim não é RFC3339 válida."))?,
                        None => existing.end_time.ok_or_else(|| {
                            anyhow::anyhow!("Este evento não tem data de fim; indica `end` para o editar.")
                        })?,
                    };
                    if start_raw.is_some()
                        && start.unix_timestamp()
                            <= serenity::all::Timestamp::now().unix_timestamp()
                    {
                        return respond(ctx, command, "A nova data de início tem de estar no futuro.").await;
                    }
                    if end.unix_timestamp() <= start.unix_timestamp() {
                        return respond(ctx, command, "O fim tem de ser depois do início.").await;
                    }
                    if end.unix_timestamp() - start.unix_timestamp() > 365 * 86_400 {
                        return respond(ctx, command, "O evento não pode durar mais de 365 dias.").await;
                    }
                    (Some(start), Some(end))
                } else {
                    (None, None)
                };

                if name.is_none()
                    && start.is_none()
                    && end.is_none()
                    && location.is_none()
                    && description.is_none()
                {
                    return respond(ctx, command, "Indica pelo menos um campo para alterar.").await;
                }

                let mut builder = serenity::all::EditScheduledEvent::new()
                    .audit_log_reason("Vozen Helper event-edit");
                if let Some(name) = name {
                    builder = builder.name(name.to_string());
                }
                if let Some(start) = start {
                    builder = builder.start_time(start);
                }
                if let Some(end) = end {
                    builder = builder.end_time(end);
                }
                if let Some(location) = location {
                    builder = builder.location(location.to_string());
                }
                if let Some(description) = description {
                    builder = builder.description(description.to_string());
                }
                let edited = guild_id.edit_scheduled_event(&ctx.http, event_id, builder).await?;
                format!("Evento nativo #{} atualizado: **{}**.", edited.id, edited.name)
            }
            "event-register" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let raw_event_id = option_i64(command, "event_id").unwrap_or(0);
                if raw_event_id <= 0 {
                    return respond(ctx, command, "Indica um ID de evento válido.").await;
                }
                let event_id = serenity::all::ScheduledEventId::new(raw_event_id as u64);
                let events = guild_id.scheduled_events(&ctx.http, false).await?;
                let Some(event) = events.into_iter().find(|event| event.id == event_id) else {
                    return respond(ctx, command, "Não encontrei esse evento neste servidor.").await;
                };
                if matches!(
                    event.status,
                    serenity::all::ScheduledEventStatus::Completed
                        | serenity::all::ScheduledEventStatus::Canceled
                ) {
                    return respond(ctx, command, "Este evento já terminou ou foi cancelado.").await;
                }
                let guild_text = guild_id.to_string();
                let capacity = self
                    .store
                    .get_setting(&guild_text, &format!("events.capacity.{}", event_id))?
                    .and_then(|value| value.parse::<u64>().ok());
                let inserted = self
                    .store
                    .register_event_with_capacity(
                        &guild_text,
                        &event_id.to_string(),
                        &command.user.id.to_string(),
                        capacity,
                    )?
                    .is_some();
                if inserted {
                    let waitlisted = self
                        .store
                        .event_registration(
                            &guild_text,
                            &event_id.to_string(),
                            &command.user.id.to_string(),
                        )?
                        .is_some_and(|registration| registration.status == "waitlisted");
                    if waitlisted {
                        format!(
                            "O evento **{}** está cheio; ficaste na lista de espera.",
                            event.name
                        )
                    } else {
                        format!("Inscrição confirmada para **{}**.", event.name)
                    }
                } else {
                    "Já estás inscrito neste evento.".to_string()
                }
            }
            "event-unregister" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let raw_event_id = option_i64(command, "event_id").unwrap_or(0);
                if raw_event_id <= 0 {
                    return respond(ctx, command, "Indica um ID de evento válido.").await;
                }
                let (removed, promoted) = self.store.remove_event_registration(
                    &guild_id.to_string(),
                    &raw_event_id.to_string(),
                    &command.user.id.to_string(),
                )?;
                if !removed {
                    return respond(ctx, command, "Não tens uma inscrição neste evento.").await;
                }
                match promoted {
                    Some(user_id) => format!(
                        "Inscrição removida; <@{}> foi promovido da lista de espera.",
                        user_id
                    ),
                    None => "Inscrição removida do evento.".to_string(),
                }
            }
            "event-checkin" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let raw_event_id = option_i64(command, "event_id").unwrap_or(0);
                if raw_event_id <= 0 {
                    return respond(ctx, command, "Indica um ID de evento válido.").await;
                }
                let event_id = serenity::all::ScheduledEventId::new(raw_event_id as u64);
                let events = guild_id.scheduled_events(&ctx.http, false).await?;
                let Some(event) = events.into_iter().find(|event| event.id == event_id) else {
                    return respond(ctx, command, "Não encontrei esse evento neste servidor.").await;
                };
                if matches!(
                    event.status,
                    serenity::all::ScheduledEventStatus::Completed
                        | serenity::all::ScheduledEventStatus::Canceled
                ) {
                    return respond(ctx, command, "Este evento já terminou ou foi cancelado.").await;
                }
                let user_id = command.user.id.to_string();
                let Some(registration) = self.store.event_registration(
                    &guild_id.to_string(),
                    &event_id.to_string(),
                    &user_id,
                )? else {
                    return respond(ctx, command, "Inscreve-te primeiro com `/event-register`.").await;
                };
                if registration.status == "checked_in" {
                    return respond(ctx, command, "O teu check-in já está registado.").await;
                }
                if self.store.check_in_event(&guild_id.to_string(), &event_id.to_string(), &user_id)? {
                    format!("Check-in registado para **{}**.", event.name)
                } else {
                    "Não foi possível registar o check-in.".to_string()
                }
            }
            "event-attendees" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let raw_event_id = option_i64(command, "event_id").unwrap_or(0);
                if raw_event_id <= 0 {
                    return respond(ctx, command, "Indica um ID de evento válido.").await;
                }
                let event_id = serenity::all::ScheduledEventId::new(raw_event_id as u64);
                let events = guild_id.scheduled_events(&ctx.http, false).await?;
                let Some(event) = events.into_iter().find(|event| event.id == event_id) else {
                    return respond(ctx, command, "Não encontrei esse evento neste servidor.").await;
                };
                let registrations = self.store.event_registrations(
                    &guild_id.to_string(),
                    &event_id.to_string(),
                    100,
                )?;
                if registrations.is_empty() {
                    format!("**{}** ainda não tem inscrições.", event.name)
                } else {
                    let lines = registrations
                        .into_iter()
                        .take(25)
                        .map(|registration| {
                            format!(
                                "<@{}> · {}",
                                registration.user_id,
                                if registration.status == "checked_in" {
                                    "check-in"
                                } else {
                                    "inscrito"
                                }
                            )
                        })
                        .collect::<Vec<_>>();
                    format!("**{}** — {} inscrição(ões)\n{}", event.name, lines.len(), lines.join("\n"))
                }
            }
            "event-cancel" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                if !feature_enabled(&self.store, &guild_id.to_string(), "community.events", None) {
                    return respond(ctx, command, "Os eventos estão desativados neste servidor. Ativa-os no painel.").await;
                }
                let event_id = option_i64(command, "event_id").unwrap_or(0);
                if event_id <= 0 {
                    return respond(ctx, command, "Indica um ID de evento válido.").await;
                }
                guild_id
                    .delete_scheduled_event(
                        &ctx.http,
                        serenity::all::ScheduledEventId::new(event_id as u64),
                    )
                    .await?;
                format!("Evento nativo #{} cancelado.", event_id)
            }
            "workflow-create" => {
                let guild_text = match command.guild_id {
                    Some(guild_id) => guild_id.to_string(),
                    None => return respond(ctx, command, "This command can only be used in a server.").await,
                };
                if !feature_enabled(&self.store, &guild_text, "management.workflows", None) {
                    return respond(
                        ctx,
                        command,
                        "Automations are disabled in this server. Enable them in the dashboard.",
                    )
                    .await;
                }
                let name = option_string(command, "name").unwrap_or_default().trim();
                let condition = option_string(command, "contains").unwrap_or_default().trim();
                let reply = option_string(command, "reply").unwrap_or_default().trim();
                let max_reply_length = setting_u64(
                    &self.store,
                    &guild_text,
                    "management.workflows.max_reply_length",
                    1_000,
                )
                .clamp(1, 1_500) as usize;
                if !(1..=50).contains(&name.len())
                    || !(1..=max_reply_length).contains(&reply.len())
                    || condition.len() > 200
                {
                    return respond(ctx, command, "Nome, condição ou resposta inválidos.").await;
                }
                let user_text = command.user.id.to_string();
                let plan = self.effective_plan(&user_text, Some(&guild_text)).await;
                let plan_limit = quota_limit(&plan, "workflows");
                let configured_limit = setting_u64(
                    &self.store,
                    &guild_text,
                    "management.workflows.max_workflows",
                    plan_limit,
                )
                .clamp(1, 100);
                let workflow_limit = plan_limit.min(configured_limit);
                let Some(id) = self.store.create_workflow_bounded(
                    &guild_text,
                    name,
                    "message",
                    condition,
                    "reply",
                    reply,
                    workflow_limit,
                )? else {
                    return respond(
                        ctx,
                        command,
                        &format!(
                            "A quota de workflows deste plano foi atingida ({workflow_limit}). Consulta `/plan` para saber como aumentar a capacidade da guild."
                        ),
                    )
                    .await;
                };
                format!("Workflow #{id} criado. É executado quando uma mensagem corresponde à condição.")
            }
            "workflow-list" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let workflows = self.store.workflows(&guild_id.to_string(), 25)?;
                if workflows.is_empty() { "Não existem workflows configurados.".to_string() } else {
                    workflows.into_iter().map(|workflow| format!("#{} **{}** · {} · {}", workflow.id, workflow.name, workflow.trigger, if workflow.enabled { "ativo" } else { "desligado" })).collect::<Vec<_>>().join("\n")
                }
            }
            "workflow-dry-run" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let id = option_i64(command, "id").unwrap_or(0);
                let sample = option_string(command, "message").unwrap_or_default().trim();
                if sample.len() > 2_000 {
                    return respond(ctx, command, "A mensagem de teste não pode exceder 2000 caracteres.").await;
                }
                let Some(workflow) = self.store.workflow(&guild_id.to_string(), id)? else {
                    return respond(ctx, command, "Workflow não encontrado neste servidor.").await;
                };
                let matches = workflow.trigger == "message"
                    && (workflow.condition.is_empty()
                        || sample
                            .to_lowercase()
                            .contains(&workflow.condition.to_lowercase()));
                if !matches {
                    format!("Dry-run: workflow **{}** não seria executado.", workflow.name)
                } else if workflow.action == "reply" {
                    let preview = workflow
                        .payload
                        .replace("{user}", &format!("<@{}>", command.user.id))
                        .replace("{message}", &truncate(sample, 500));
                    format!("Dry-run: workflow **{}** faria reply:\n> {}", workflow.name, truncate(&preview, 1_500))
                } else {
                    format!("Dry-run: workflow **{}** correspondeu, mas a action `{}` não é suportada.", workflow.name, workflow.action)
                }
            }
            "workflow-toggle" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let id = option_i64(command, "id").unwrap_or(0);
                let enabled = option_bool(command, "enabled").unwrap_or(false);
                if self.store.set_workflow_enabled(&guild_id.to_string(), id, enabled)? {
                    format!("Workflow #{id} {}.", if enabled { "ativado" } else { "desativado imediatamente" })
                } else {
                    "Workflow não encontrado neste servidor.".to_string()
                }
            }
            "workflow-delete" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let id = option_i64(command, "id").unwrap_or(0);
                if self.store.delete_workflow(&guild_id.to_string(), id)? { format!("Workflow #{id} eliminado.") } else { "Workflow não encontrado neste servidor.".to_string() }
            }
            "ticket-config" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let guild_text = guild_id.to_string();
                if let Some(role_id) = command.data.options.iter().find_map(|option| match option.value {
                    CommandDataOptionValue::Role(role) if option.name == "staff_role" => Some(role),
                    _ => None,
                }) {
                    self.store.set_setting(&guild_text, "support.ticket.staff_role_id", &role_id.to_string())?;
                }
                if let Some(channel_id) = command.data.options.iter().find_map(|option| match option.value {
                    CommandDataOptionValue::Channel(channel) if option.name == "transcript_channel" => Some(channel),
                    _ => None,
                }) {
                    self.store.set_setting(&guild_text, "support.ticket.transcript_channel_id", &channel_id.to_string())?;
                }
                if let Some(minutes) = option_i64(command, "sla_minutes") {
                    if !(5..=1_440).contains(&minutes) {
                        return respond(ctx, command, "O SLA deve estar entre 5 e 1440 minutos.").await;
                    }
                    self.store.set_setting(&guild_text, "support.ticket.sla_ms", &(minutes * 60_000).to_string())?;
                }
                "Configuração de tickets guardada.".to_string()
            }
            "ticket-panel" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let guild_text = guild_id.to_string();
                let plan = self
                    .effective_plan(&command.user.id.to_string(), Some(&guild_text))
                    .await;
                let limit = quota_limit(&plan, "panels");
                let used = self
                    .store
                    .count_settings_prefix(&guild_text, "support.panel.")?;
                if used >= limit {
                    return respond(
                        ctx,
                        command,
                        "A quota de painéis de tickets deste servidor foi atingida.",
                    )
                    .await;
                }
                let message = command
                    .channel_id
                    .send_message(
                        &ctx.http,
                        serenity::all::CreateMessage::new()
                            .content("Need help? Open a private ticket.")
                            .components(vec![CreateActionRow::Buttons(vec![
                                CreateButton::new("ticket:open")
                                    .label("Open ticket")
                                    .style(ButtonStyle::Primary),
                            ])]),
                    )
                    .await?;
                self.store.set_setting(
                    &guild_text,
                    &format!("support.panel.{}", message.id),
                    &serde_json::json!({
                        "channel_id": command.channel_id,
                        "message_id": message.id
                    })
                    .to_string(),
                )?;
                format!("Painel de tickets criado em <#{}>.", command.channel_id)
            }
            "ticket-update" => {
                let Some(_guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let Some(ticket) = self.store.ticket_by_channel(&command.channel_id.to_string())? else {
                    return respond(ctx, command, "Este canal não é um ticket do Helper.").await;
                };
                let priority = option_string(command, "priority").map(str::trim);
                let category = option_string(command, "category").map(str::trim);
                let note = option_string(command, "note").map(str::trim);
                if priority.is_none() && category.is_none() && note.is_none() {
                    return respond(ctx, command, "Indica prioridade, categoria ou nota.").await;
                }
                if let Some(priority) = priority
                    && !matches!(priority, "low" | "normal" | "high" | "urgent")
                {
                    return respond(ctx, command, "Prioridade inválida.").await;
                }
                if let Some(category) = category
                    && !(1..=50).contains(&category.len())
                {
                    return respond(ctx, command, "A categoria tem de ter entre 1 e 50 caracteres.").await;
                }
                if let Some(note) = note
                    && note.len() > 2_000
                {
                    return respond(ctx, command, "A nota não pode exceder 2000 caracteres.").await;
                }
                if let Some(priority) = priority {
                    self.store.set_ticket_priority(&ticket.channel_id, priority)?;
                }
                if let Some(category) = category {
                    self.store.set_ticket_category(&ticket.channel_id, category)?;
                }
                if let Some(note) = note {
                    self.store.set_ticket_notes(&ticket.channel_id, note)?;
                }
                let updated = self.store.ticket_by_channel(&ticket.channel_id)?.ok_or_else(|| anyhow::anyhow!("ticket disappeared"))?;
                format!("Ticket atualizado: categoria **{}**, prioridade **{}**{}.", updated.category, updated.priority, if updated.notes.is_empty() { String::new() } else { " · nota interna guardada".to_string() })
            }
            "ticket-rate" => {
                let Some(_guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let Some(ticket) = self.store.ticket_by_channel(&command.channel_id.to_string())? else {
                    return respond(ctx, command, "Este canal não é um ticket do Helper.").await;
                };
                if ticket.user_id != command.user.id.to_string() {
                    return respond(ctx, command, "Só o autor do ticket pode avaliá-lo.").await;
                }
                let score = option_i64(command, "score").unwrap_or(0);
                if !(1..=5).contains(&score) {
                    return respond(ctx, command, "A avaliação tem de ser entre 1 e 5.").await;
                }
                if ticket.status != "closed" {
                    return respond(ctx, command, "Só podes avaliar um ticket fechado.").await;
                }
                if !self.store.set_ticket_csat(&ticket.channel_id, score)? {
                    return respond(ctx, command, "Não foi possível guardar a avaliação.").await;
                }
                format!("Obrigado pela avaliação: **{score}/5**.")
            }
            "rolepanel" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                if !feature_enabled(&self.store, &guild_id.to_string(), "community.role_panels", None) {
                    return respond(ctx, command, "Os painéis de cargos estão desativados neste servidor. Ativa-os no painel.").await;
                }
                let guild_text = guild_id.to_string();
                let plan = self
                    .effective_plan(&command.user.id.to_string(), Some(&guild_text))
                    .await;
                let limit = quota_limit(&plan, "role_panels");
                let used = self
                    .store
                    .count_settings_prefix(&guild_text, "community.role_panel.")?;
                if used >= limit {
                    return respond(
                        ctx,
                        command,
                        "A quota de painéis de cargos deste servidor foi atingida.",
                    )
                    .await;
                }
                let title = option_string(command, "title").unwrap_or("Choose your roles");
                let mut buttons = Vec::new();
                let mut role_ids = Vec::new();
                for name in ["role1", "role2", "role3", "role4", "role5"] {
                    if let Some(role_id) = command.data.options.iter().find_map(|option| {
                        (option.name == name).then_some(match option.value {
                            CommandDataOptionValue::Role(role) => role,
                            _ => return None,
                        })
                    }) {
                        role_ids.push(role_id.get());
                        buttons.push(
                            CreateButton::new(format!("role:toggle:{}", role_id.get()))
                                .label(format!("Role {}", buttons.len() + 1))
                                .style(ButtonStyle::Secondary),
                        );
                    }
                }
                if buttons.is_empty() {
                    return respond(ctx, command, "Indica pelo menos um cargo válido.").await;
                }
                let message = command
                    .channel_id
                    .send_message(
                        &ctx.http,
                        serenity::all::CreateMessage::new()
                            .content(title)
                            .components(vec![CreateActionRow::Buttons(buttons)]),
                    )
                    .await?;
                self.store.set_setting(
                    &guild_text,
                    &format!("community.role_panel.{}", message.id),
                    &serde_json::json!({
                        "channel_id": command.channel_id,
                        "message_id": message.id,
                        "title": title,
                        "role_ids": role_ids,
                    })
                    .to_string(),
                )?;
                "Painel de cargos criado.".to_string()
            }
            _ => "Comando desconhecido.".to_string(),
        };
        let public_leaderboard = command.data.name == "leaderboard"
            && command.guild_id.is_some_and(|guild_id| {
                setting_bool(
                    &self.store,
                    &guild_id.to_string(),
                    "community.leaderboard.public",
                    true,
                )
            });
        let public_stats = command.data.name == "serverstats"
            && command.guild_id.is_some_and(|guild_id| {
                setting_bool(
                    &self.store,
                    &guild_id.to_string(),
                    "insights.stats.public",
                    false,
                )
            });
        command
            .create_response(
                ctx,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content(english_bot_text(&content))
                        .ephemeral(
                            command.data.name != "ping" && !(public_leaderboard || public_stats),
                        ),
                ),
            )
            .await?;
        Ok(())
    }

    async fn handle_component(
        &self,
        ctx: &Context,
        component: &serenity::all::ComponentInteraction,
    ) -> Result<()> {
        let Some(guild_id) = component.guild_id else {
            return respond_component(ctx, component, "Este botão só funciona num servidor.").await;
        };
        if let Some((kind, raw_id)) =
            component
                .data
                .custom_id
                .split_once(':')
                .and_then(|(prefix, rest)| {
                    let (action, id) = rest.split_once(':')?;
                    (prefix == "suggest" && (action == "up" || action == "down"))
                        .then_some((action, id))
                })
        {
            if !feature_enabled(
                &self.store,
                &guild_id.to_string(),
                "community.suggestions",
                None,
            ) {
                return respond_component(
                    ctx,
                    component,
                    "As sugestões estão desativadas neste servidor.",
                )
                .await;
            }
            let id = raw_id
                .parse::<i64>()
                .map_err(|_| anyhow::anyhow!("invalid suggestion button"))?;
            let vote = if kind == "up" { 1 } else { -1 };
            self.store
                .vote_suggestion(id, &component.user.id.to_string(), vote)?;
            let Some(suggestion) = self.store.suggestion(id)? else {
                return respond_component(ctx, component, "Sugestão não encontrada.").await;
            };
            let (up, down) = self.store.suggestion_votes(id)?;
            let content = format!(
                "**Sugestão #{}** por <@{}>\n{}\n\nEstado: **{}** · Apoio: {} · Contra: {}",
                suggestion.id,
                suggestion.author_id,
                suggestion.content,
                suggestion.status,
                up,
                down
            );
            let _ = ctx
                .http
                .edit_message(
                    component.channel_id,
                    component.message.id,
                    &serde_json::json!({"content": content}),
                    Vec::new(),
                )
                .await;
            return respond_component(ctx, component, "Voto registado.").await;
        }
        if let Some(raw_id) = component.data.custom_id.strip_prefix("giveaway:join:") {
            if !feature_enabled(
                &self.store,
                &guild_id.to_string(),
                "community.giveaways",
                None,
            ) {
                return respond_component(
                    ctx,
                    component,
                    "Os giveaways estão desativados neste servidor.",
                )
                .await;
            }
            let id = raw_id
                .parse::<i64>()
                .map_err(|_| anyhow::anyhow!("invalid giveaway button"))?;
            let Some(giveaway) = self.store.giveaway(id)? else {
                return respond_component(ctx, component, "Giveaway não encontrado.").await;
            };
            if giveaway.ended {
                return respond_component(ctx, component, "Este giveaway já terminou.").await;
            }
            if let Some(role_id) = giveaway
                .required_role_id
                .as_deref()
                .and_then(|raw| raw.parse::<u64>().ok())
            {
                let member = guild_id.member(&ctx.http, component.user.id).await?;
                if !member.roles.iter().any(|role| role.get() == role_id) {
                    return respond_component(
                        ctx,
                        component,
                        "Não tens o cargo necessário para participar.",
                    )
                    .await;
                }
            }
            if self
                .store
                .add_giveaway_entry(id, &component.user.id.to_string())?
            {
                return respond_component(ctx, component, "Entrada registada. Boa sorte!").await;
            }
            self.store
                .remove_giveaway_entry(id, &component.user.id.to_string())?;
            return respond_component(ctx, component, "Saíste do giveaway.").await;
        }
        if let Some(raw) = component.data.custom_id.strip_prefix("poll:") {
            if !feature_enabled(&self.store, &guild_id.to_string(), "management.polls", None) {
                return respond_component(
                    ctx,
                    component,
                    "As enquetes estão desativadas neste servidor.",
                )
                .await;
            }
            let mut parts = raw.split(':');
            let id = parts
                .next()
                .and_then(|value| value.parse::<i64>().ok())
                .ok_or_else(|| anyhow::anyhow!("invalid poll button"))?;
            let choice = parts
                .next()
                .and_then(|value| value.parse::<usize>().ok())
                .ok_or_else(|| anyhow::anyhow!("invalid poll choice"))?;
            let Some(poll) = self.store.poll(id)? else {
                return respond_component(ctx, component, "Poll não encontrada.").await;
            };
            if poll.closed || choice >= poll.options.len() {
                return respond_component(
                    ctx,
                    component,
                    "Esta poll já terminou ou a opção é inválida.",
                )
                .await;
            }
            self.store
                .vote_poll(id, &component.user.id.to_string(), choice)?;
            return respond_component(
                ctx,
                component,
                "Voto registado; podes votar novamente para alterar.",
            )
            .await;
        }
        if let Some(raw_role_id) = component.data.custom_id.strip_prefix("role:toggle:") {
            let role_id = raw_role_id
                .parse::<u64>()
                .ok()
                .map(RoleId::new)
                .ok_or_else(|| anyhow::anyhow!("invalid role button"))?;
            let panel_key = format!("community.role_panel.{}", component.message.id);
            let panel = self
                .store
                .get_setting(&guild_id.to_string(), &panel_key)?
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok());
            let configured = panel
                .as_ref()
                .and_then(|value| value.get("role_ids"))
                .and_then(serde_json::Value::as_array)
                .is_some_and(|roles| {
                    roles
                        .iter()
                        .any(|value| value.as_u64().is_some_and(|id| id == role_id.get()))
                });
            if !configured {
                return respond_component(ctx, component, "This role panel is no longer valid.")
                    .await;
            }
            let roles = guild_id.roles(&ctx.http).await?;
            let Some(role) = roles.get(&role_id) else {
                return respond_component(ctx, component, "That role no longer exists.").await;
            };
            if role.managed {
                return respond_component(
                    ctx,
                    component,
                    "Managed Discord roles cannot be assigned.",
                )
                .await;
            }
            let bot_user = ctx.http.get_current_user().await?;
            let bot_member = guild_id.member(&ctx.http, bot_user.id).await?;
            let bot_top_position = bot_member
                .roles
                .iter()
                .filter_map(|id| roles.get(id).map(|item| item.position))
                .max()
                .unwrap_or(0);
            if role.position >= bot_top_position {
                return respond_component(
                    ctx,
                    component,
                    "That role is above the Helper's highest role.",
                )
                .await;
            }
            let member = guild_id.member(&ctx.http, component.user.id).await?;
            if member.roles.contains(&role_id) {
                member.remove_role(&ctx.http, role_id).await?;
                return respond_component(ctx, component, "Cargo removido.").await;
            }
            member.add_role(&ctx.http, role_id).await?;
            return respond_component(ctx, component, "Cargo atribuído.").await;
        }
        if let Some(raw) = component.data.custom_id.strip_prefix("verify:") {
            let mut parts = raw.split(':');
            let expected_guild = parts.next().unwrap_or_default();
            let role_id = parts.next().and_then(|value| value.parse::<u64>().ok());
            if expected_guild != guild_id.to_string() {
                return respond_component(ctx, component, "Este painel pertence a outro servidor.")
                    .await;
            }
            let Some(role_id) = role_id else {
                return respond_component(ctx, component, "Painel de verificaÃ§Ã£o invÃ¡lido.")
                    .await;
            };
            let member = guild_id.member(&ctx.http, component.user.id).await?;
            let role = RoleId::new(role_id);
            if member.roles.contains(&role) {
                return respond_component(ctx, component, "JÃ¡ estÃ¡s verificado.").await;
            }
            member.add_role(&ctx.http, role).await?;
            return respond_component(ctx, component, "Verificacao concluida; cargo atribuido.")
                .await;
        }
        match component.data.custom_id.as_str() {
            "ticket:open" => {
                if self
                    .store
                    .get_setting(&guild_id.to_string(), "feature.support.tickets")?
                    .is_some_and(|value| value != "true")
                {
                    return respond_component(
                        ctx,
                        component,
                        "Os tickets estão desativados neste servidor.",
                    )
                    .await;
                }
                if let Some(ticket) = self
                    .store
                    .active_ticket_for_user(&guild_id.to_string(), &component.user.id.to_string())?
                {
                    return respond_component(
                        ctx,
                        component,
                        &format!("You already have an open ticket: <#{}>.", ticket.channel_id),
                    )
                    .await;
                }
                let bot_id = ctx.http.get_current_user().await?.id;
                let visible = Permissions::VIEW_CHANNEL
                    | Permissions::SEND_MESSAGES
                    | Permissions::READ_MESSAGE_HISTORY;
                let mut overwrites = vec![
                    PermissionOverwrite {
                        allow: Permissions::empty(),
                        deny: Permissions::VIEW_CHANNEL,
                        kind: PermissionOverwriteType::Role(serenity::all::RoleId::new(
                            guild_id.get(),
                        )),
                    },
                    PermissionOverwrite {
                        allow: visible,
                        deny: Permissions::empty(),
                        kind: PermissionOverwriteType::Member(component.user.id),
                    },
                    PermissionOverwrite {
                        allow: visible | Permissions::MANAGE_CHANNELS,
                        deny: Permissions::empty(),
                        kind: PermissionOverwriteType::Member(bot_id),
                    },
                ];
                if let Ok(Some(raw_role)) = self
                    .store
                    .get_setting(&guild_id.to_string(), "support.ticket.staff_role_id")
                    && let Ok(role_id) = raw_role.parse::<u64>()
                {
                    overwrites.push(PermissionOverwrite {
                        allow: visible,
                        deny: Permissions::empty(),
                        kind: PermissionOverwriteType::Role(RoleId::new(role_id)),
                    });
                }
                let channel = guild_id
                    .create_channel(
                        &ctx.http,
                        CreateChannel::new(format!("ticket-{}", component.user.name))
                            .permissions(overwrites),
                    )
                    .await?;
                self.store.open_ticket(
                    &guild_id.to_string(),
                    &component.user.id.to_string(),
                    &channel.id.to_string(),
                )?;
                let sla_ms = self
                    .store
                    .get_setting(&guild_id.to_string(), "support.ticket.sla_ms")?
                    .and_then(|value| value.parse::<i64>().ok())
                    .unwrap_or(3_600_000);
                self.store.schedule_typed(
                    &guild_id.to_string(),
                    "ticket_sla",
                    &component.user.id.to_string(),
                    chrono::Utc::now().timestamp_millis() + sla_ms,
                    &serde_json::json!({"channel_id": channel.id.to_string()}).to_string(),
                )?;
                channel
                    .id
                    .send_message(
                        &ctx.http,
                        serenity::all::CreateMessage::new()
                            .content(format!(
                                "Hello <@{}>. Tell us what you need.",
                                component.user.id
                            ))
                            .components(vec![CreateActionRow::Buttons(vec![
                                CreateButton::new("ticket:claim")
                                    .label("Claim")
                                    .style(ButtonStyle::Secondary),
                                CreateButton::new("ticket:close")
                                    .label("Close")
                                    .style(ButtonStyle::Danger),
                            ])]),
                    )
                    .await?;
                respond_component(
                    ctx,
                    component,
                    &format!("Ticket criado: <#{}>.", channel.id),
                )
                .await
            }
            "ticket:claim" => {
                if self.store.claim_ticket(
                    &component.channel_id.to_string(),
                    &component.user.id.to_string(),
                )? {
                    component
                        .channel_id
                        .say(
                            &ctx.http,
                            format!("Ticket claimed by <@{}>.", component.user.id),
                        )
                        .await?;
                    respond_component(ctx, component, "Ticket claimed.").await
                } else {
                    respond_component(ctx, component, "This ticket is already closed.").await
                }
            }
            "ticket:reopen" => {
                let Some(ticket) = self
                    .store
                    .ticket_by_channel(&component.channel_id.to_string())?
                else {
                    return respond_component(ctx, component, "Ticket not found.").await;
                };
                let is_opener = ticket.user_id == component.user.id.to_string();
                let is_staff = if let Ok(Some(raw_role)) = self
                    .store
                    .get_setting(&guild_id.to_string(), "support.ticket.staff_role_id")
                {
                    raw_role.parse::<u64>().ok().is_some_and(|role_id| {
                        component.member.as_ref().is_some_and(|member| {
                            member.roles.iter().any(|role| role.get() == role_id)
                        })
                    })
                } else {
                    false
                };
                if !is_opener && !is_staff {
                    return respond_component(
                        ctx,
                        component,
                        "Only the ticket author or support team can reopen this ticket.",
                    )
                    .await;
                }
                if !self
                    .store
                    .reopen_ticket(&component.channel_id.to_string())?
                {
                    return respond_component(
                        ctx,
                        component,
                        "Este ticket já está aberto ou não existe.",
                    )
                    .await;
                }
                component
                    .channel_id
                    .edit(
                        &ctx.http,
                        EditChannel::new().name(format!("ticket-{}", ticket.user_id)),
                    )
                    .await?;
                component
                    .channel_id
                    .send_message(
                        &ctx.http,
                        serenity::all::CreateMessage::new()
                            .content("Ticket reopened. The team can continue responding.")
                            .components(vec![CreateActionRow::Buttons(vec![
                                CreateButton::new("ticket:claim")
                                    .label("Claim")
                                    .style(ButtonStyle::Secondary),
                                CreateButton::new("ticket:close")
                                    .label("Close")
                                    .style(ButtonStyle::Danger),
                            ])]),
                    )
                    .await?;
                respond_component(ctx, component, "Ticket reopened.").await
            }
            "ticket:close" => {
                let ticket = self
                    .store
                    .ticket_by_channel(&component.channel_id.to_string())?;
                let Some(ticket_for_auth) = ticket.as_ref() else {
                    return respond_component(ctx, component, "Ticket not found.").await;
                };
                let is_opener = ticket_for_auth.user_id == component.user.id.to_string();
                let is_staff = if let Ok(Some(raw_role)) = self
                    .store
                    .get_setting(&guild_id.to_string(), "support.ticket.staff_role_id")
                {
                    raw_role.parse::<u64>().ok().is_some_and(|role_id| {
                        component.member.as_ref().is_some_and(|member| {
                            member.roles.iter().any(|role| role.get() == role_id)
                        })
                    })
                } else {
                    false
                };
                if !is_opener && !is_staff {
                    return respond_component(
                        ctx,
                        component,
                        "Only the ticket author or support team can close this ticket.",
                    )
                    .await;
                }
                if self.store.close_ticket(&component.channel_id.to_string())? {
                    if let Some(raw_channel) = self.store.get_setting(
                        &guild_id.to_string(),
                        "support.ticket.transcript_channel_id",
                    )? && let Ok(transcript_channel) = raw_channel.parse::<u64>()
                    {
                        let messages = component
                            .channel_id
                            .messages(&ctx.http, serenity::all::GetMessages::new().limit(100))
                            .await
                            .unwrap_or_default();
                        let opener = ticket
                            .as_ref()
                            .map(|ticket| format!("<@{}>", ticket.user_id))
                            .unwrap_or_else(|| "unknown".to_string());
                        let mut transcript = format!("Ticket transcript for {opener}\n");
                        for message in messages.iter().rev() {
                            transcript.push_str(&format!(
                                "{}: {}\n",
                                message.author.name, message.content
                            ));
                        }
                        for chunk in transcript.as_bytes().chunks(1_800) {
                            let text = String::from_utf8_lossy(chunk);
                            let _ = ChannelId::new(transcript_channel)
                                .say(&ctx.http, text)
                                .await;
                        }
                    }
                    component
                        .channel_id
                        .edit(
                            &ctx.http,
                            EditChannel::new()
                                .name(format!("closed-ticket-{}", ticket_for_auth.user_id)),
                        )
                        .await?;
                    component
                        .channel_id
                        .send_message(
                            &ctx.http,
                            serenity::all::CreateMessage::new()
                                .content("Ticket closed. The history was preserved; the author or team can reopen it.")
                                .components(vec![CreateActionRow::Buttons(vec![
                                    CreateButton::new("ticket:reopen")
                                        .label("Reopen")
                                        .style(ButtonStyle::Success),
                                ])]),
                        )
                        .await?;
                    respond_component(
                        ctx,
                        component,
                        "Ticket closed and archived. The channel was not deleted.",
                    )
                    .await?;
                } else {
                    respond_component(ctx, component, "This ticket is already closed.").await?;
                }
                Ok(())
            }
            _ => respond_component(ctx, component, "Botão desconhecido.").await,
        }
    }
}

async fn respond(ctx: &Context, command: &CommandInteraction, content: &str) -> Result<()> {
    command
        .create_response(
            ctx,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(english_bot_text(content))
                    .ephemeral(true),
            ),
        )
        .await?;
    Ok(())
}

async fn respond_component(
    ctx: &Context,
    component: &serenity::all::ComponentInteraction,
    content: &str,
) -> Result<()> {
    component
        .create_response(
            ctx,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(english_bot_text(content))
                    .ephemeral(true),
            ),
        )
        .await?;
    Ok(())
}

/// Translate legacy Portuguese system text at the Discord boundary.
///
/// Configuration values, tag bodies and user supplied content remain untouched;
/// only known Helper-owned phrases are translated. This keeps existing guild
/// settings compatible while ensuring command/component feedback is English.
fn english_bot_text(input: &str) -> String {
    const REPLACEMENTS: &[(&str, &str)] = &[
        ("Olá ", "Hello "),
        ("Explica aqui o que precisas.", "Tell us what you need."),
        (
            "Este comando só pode ser usado num servidor.",
            "This command can only be used in a server.",
        ),
        (
            "Este comando sÃ³ pode ser usado num servidor.",
            "This command can only be used in a server.",
        ),
        ("Indica um membro.", "Specify a member."),
        ("Indica um utilizador.", "Specify a user."),
        ("Indica um ID de utilizador.", "Specify a user ID."),
        ("ID de utilizador inválido.", "Invalid user ID."),
        ("Indica um canal válido.", "Specify a valid channel."),
        (
            "Indica pelo menos um cargo válido.",
            "Specify at least one valid role.",
        ),
        (
            "Indica uma pergunta e pelo menos duas opções.",
            "Specify a question and at least two options.",
        ),
        (
            "Indica prioridade, categoria ou nota.",
            "Specify a priority, category or note.",
        ),
        (
            "Indica pelo menos um campo para alterar.",
            "Specify at least one field to change.",
        ),
        (
            "Não foi possível concluir esta ação.",
            "Unable to complete this action.",
        ),
        (
            "Nao foi possivel executar o ban; confirma as permissoes e a hierarquia.",
            "Unable to ban this member; check permissions and role hierarchy.",
        ),
        (
            "Não tens a permissão necessária para este comando.",
            "You do not have the required permission for this command.",
        ),
        (
            "O conteúdo não pode exceder 500 caracteres.",
            "Content cannot exceed 500 characters.",
        ),
        (
            "A mensagem de teste não pode exceder 2000 caracteres.",
            "The test message cannot exceed 2,000 characters.",
        ),
        (
            "O lembrete não pode exceder 500 caracteres.",
            "The reminder cannot exceed 500 characters.",
        ),
        ("Nome ou conteúdo inválido.", "Invalid name or content."),
        (
            "Nome, condição ou resposta inválidos.",
            "Invalid name, condition or response.",
        ),
        (
            "Nome, local ou descrição inválidos.",
            "Invalid name, location or description.",
        ),
        (
            "Duração inválida. Usa formatos como 10m, 2h ou 1d.",
            "Invalid duration. Use formats such as 10m, 2h or 1d.",
        ),
        (
            "Duração inválida. Usa 10m, 2h ou 1d.",
            "Invalid duration. Use 10m, 2h or 1d.",
        ),
        (
            "DuraÃ§Ã£o invÃ¡lida. Usa 10m, 2h ou 1d.",
            "Invalid duration. Use 10m, 2h or 1d.",
        ),
        (
            "O prémio deve ter entre 1 e 200 caracteres.",
            "The prize must be between 1 and 200 characters.",
        ),
        (
            "As sugestões estão desativadas neste servidor. Ativa-as no painel.",
            "Suggestions are disabled in this server. Enable them in the dashboard.",
        ),
        (
            "Os giveaways estão desativados neste servidor. Ativa-os no painel.",
            "Giveaways are disabled in this server. Enable them in the dashboard.",
        ),
        (
            "As enquetes estão desativadas neste servidor. Ativa-as no painel.",
            "Polls are disabled in this server. Enable them in the dashboard.",
        ),
        (
            "Os eventos estão desativados neste servidor. Ativa-os no painel.",
            "Events are disabled in this server. Enable them in the dashboard.",
        ),
        (
            "Workflow não encontrado neste servidor.",
            "Workflow not found in this server.",
        ),
        (
            "Não encontrei esse evento neste servidor.",
            "That event was not found in this server.",
        ),
        (
            "Não encontrei esse evento neste servidor.",
            "That event was not found in this server.",
        ),
        (
            "Giveaway não encontrado ou já terminado.",
            "Giveaway not found or already ended.",
        ),
        (
            "Esse membro não está em quarantine.",
            "That member is not quarantined.",
        ),
        (
            "Configura primeiro `/join-gate` com um cargo de verificação.",
            "Configure `/join-gate` first with a verification role.",
        ),
        (
            "Configura primeiro `/join-gate` com um cargo de verificaÃ§Ã£o.",
            "Configure `/join-gate` first with a verification role.",
        ),
        (
            "Ativa primeiro o `/join-gate`; o painel não deve ficar exposto enquanto o gate está desligado.",
            "Enable `/join-gate` first; do not expose the panel while the gate is disabled.",
        ),
        (
            "Ativa primeiro o `/join-gate`; o painel nÃ£o deve ficar exposto enquanto o gate estÃ¡ desligado.",
            "Enable `/join-gate` first; do not expose the panel while the gate is disabled.",
        ),
        (
            "Carrega no botão para receber o cargo de membro verificado.",
            "Click the button to receive the verified member role.",
        ),
        (
            "Carrega no botÃ£o para receber o cargo de membro verificado.",
            "Click the button to receive the verified member role.",
        ),
        (
            "Painel de verificação publicado neste canal.",
            "Verification panel posted in this channel.",
        ),
        (
            "Painel de verificaÃ§Ã£o publicado neste canal.",
            "Verification panel posted in this channel.",
        ),
        ("Painel de cargos criado.", "Role panel created."),
        ("Painel criado.", "Panel created."),
        ("Comando desconhecido.", "Unknown command."),
        ("Dados voluntários apagados.", "Voluntary data deleted."),
        ("Dados voluntÃ¡rios apagados.", "Voluntary data deleted."),
        ("Ticket criado:", "Ticket created:"),
        ("Ticket reaberto.", "Ticket reopened."),
        (
            "Ticket fechado e arquivado. O canal não foi apagado.",
            "Ticket closed and archived. The channel was not deleted.",
        ),
        (
            "Ticket fechado e arquivado. O canal nÃ£o foi apagado.",
            "Ticket closed and archived. The channel was not deleted.",
        ),
        (
            "Este ticket já está aberto ou não existe.",
            "This ticket is already open or does not exist.",
        ),
        (
            "Este ticket jÃ¡ estÃ¡ aberto ou nÃ£o existe.",
            "This ticket is already open or does not exist.",
        ),
        (
            "Este canal não é um ticket do Helper.",
            "This channel is not a Helper ticket.",
        ),
        (
            "Precisas de ajuda? Abre um ticket privado.",
            "Need help? Open a private ticket.",
        ),
        ("Abrir ticket", "Open ticket"),
        ("Reabrir", "Reopen"),
        ("Fechar", "Close"),
        ("Assumir", "Claim"),
        ("Verificar", "Verify"),
        ("Participar", "Join"),
        ("Vota nesta sugestão:", "Vote on this suggestion:"),
        ("Sugestão", "Suggestion"),
        ("Sugestão publicada.", "Suggestion published."),
        ("Giveaway criado.", "Giveaway created."),
        ("Giveaway terminado.", "Giveaway ended."),
        ("Poll #", "Poll #"),
        ("Poll #{} criada.", "Poll #{} created."),
        ("Evento criado.", "Event created."),
        ("Evento atualizado.", "Event updated."),
        ("Evento cancelado.", "Event cancelled."),
        (
            "Inscreve-te primeiro com `/event-register`.",
            "Register first with `/event-register`.",
        ),
        (
            "O teu check-in já está registado.",
            "Your check-in is already recorded.",
        ),
        (
            "O teu check-in jÃ¡ estÃ¡ registado.",
            "Your check-in is already recorded.",
        ),
        (
            "Não tens uma inscrição neste evento.",
            "You are not registered for this event.",
        ),
        (
            "Não tens uma inscriÃ§Ã£o neste evento.",
            "You are not registered for this event.",
        ),
        (
            "Eventos concluídos ou cancelados não podem ser editados.",
            "Completed or cancelled events cannot be edited.",
        ),
        (
            "O evento não pode durar mais de 365 dias.",
            "The event cannot last longer than 365 days.",
        ),
        (
            "A nova data de início tem de estar no futuro.",
            "The new start date must be in the future.",
        ),
        (
            "O fim tem de ser depois do início.",
            "The end must be after the start.",
        ),
        (
            "A descrição não pode exceder 1000 caracteres.",
            "The description cannot exceed 1,000 characters.",
        ),
        (
            "A localização só pode ser alterada em eventos externos e deve ter 1–100 caracteres.",
            "Location can only be changed for external events and must be 1–100 characters.",
        ),
        (
            "O nome tem de ter entre 1 e 100 caracteres.",
            "The name must be between 1 and 100 characters.",
        ),
        (
            "O SLA deve estar entre 5 e 1440 minutos.",
            "SLA must be between 5 and 1,440 minutes.",
        ),
        (
            "A quota de painéis de tickets deste servidor foi atingida.",
            "This server has reached its ticket panel quota.",
        ),
        (
            "A lotação deve estar entre 1 e 100000.",
            "Capacity must be between 1 and 100,000.",
        ),
        (
            "A sugestão deve ter entre 3 e 1000 caracteres.",
            "The suggestion must be between 3 and 1,000 characters.",
        ),
        (
            "Pong — Vozen Helper está online.",
            "Pong — Vozen Helper is online.",
        ),
        ("Vozen Helper público:", "Public Vozen Helper:"),
        ("Setup guiado:", "Guided setup:"),
        ("Setup guardado para:", "Setup saved for:"),
        ("Próximo passo:", "Next step:"),
        ("Setup:", "Setup:"),
        ("Módulos:", "Modules:"),
        ("concluído", "complete"),
        ("pendente", "pending"),
        ("nenhum", "none"),
        ("Painel:", "Dashboard:"),
        (
            "O XP card está desativado neste servidor. Ativa-o no painel primeiro.",
            "The XP card is disabled in this server. Enable it in the dashboard first.",
        ),
        ("Plano ", "Plan "),
        (
            "Não foi possível consultar o plano agora; o Helper mantém o último snapshot seguro.",
            "Unable to check the plan right now; Helper is keeping the last safe snapshot.",
        ),
        (
            "Entitlements central ainda não estão configurados nesta instalação.",
            "Central entitlements are not configured in this installation yet.",
        ),
        (
            "Ainda não existem casos neste servidor.",
            "There are no cases in this server yet.",
        ),
        ("NÃ£o existem casos para", "There are no cases for"),
        ("Não existem casos para", "There are no cases for"),
        ("Aviso criado como caso", "Warning created as case"),
        ("Registo #", "Record #"),
        ("criado para", "created for"),
        (
            "Caso não encontrado neste servidor.",
            "Case not found in this server.",
        ),
        ("NÃ£o existem casos para", "There are no cases for"),
        ("Sem motivo", "No reason provided"),
        ("Sem conteúdo", "No content"),
        ("Sem conteÃºdo", "No content"),
        (
            "Não foi possível remover o timeout; confirma as permissões.",
            "Unable to remove the timeout; check permissions.",
        ),
        (
            "Não foi possível remover o ban.",
            "Unable to remove the ban.",
        ),
        (
            "Não encontrei mensagens para apagar.",
            "No messages found to delete.",
        ),
        ("Softban concluído como caso", "Softban completed as case"),
        ("Tempban concluído como caso", "Tempban completed as case"),
        ("expira em", "expires in"),
        ("Ação ", "Action "),
        ("Ação concluída como caso", "Action completed as case"),
        (
            "Não foi possível executar a ação; confirma as permissões e a hierarquia de cargos.",
            "Unable to perform the action; check permissions and role hierarchy.",
        ),
        (
            "Não tinhas AFK definido.",
            "You did not have an AFK status set.",
        ),
        ("Tag não encontrada.", "Tag not found."),
        ("Ainda não existem tags.", "There are no tags yet."),
        ("Tag `", "Tag `"),
        ("` eliminada.", "` deleted."),
        ("Ainda não existem dados de XP.", "There is no XP data yet."),
        (
            "Mensagens registadas nos últimos",
            "Messages recorded in the last",
        ),
        ("dias:", "days:"),
        (
            "Starboard configurado no canal",
            "Starboard configured in channel",
        ),
        ("Requer 3 ⭐ para publicar.", "Requires 3 ⭐ to publish."),
        ("Estado inválido:", "Invalid status:"),
        ("Sugestão #", "Suggestion #"),
        ("marcada como", "marked as"),
        (
            "Sugestão não encontrada neste servidor.",
            "Suggestion not found in this server.",
        ),
        ("Giveaway #", "Giveaway #"),
        (
            "Giveaway nÃ£o encontrado, ainda ativo ou sem participantes.",
            "Giveaway not found, still active or without participants.",
        ),
        (
            "Giveaway não encontrado, ainda ativo ou sem participantes.",
            "Giveaway not found, still active or without participants.",
        ),
        (
            "Não existem giveaways ativos.",
            "There are no active giveaways.",
        ),
        ("colocado em quarantine como caso", "quarantined as case"),
        (
            "Os cargos foram guardados para restauro.",
            "Roles were saved for restoration.",
        ),
        ("Quarantine removida de", "Quarantine removed from"),
        ("cargo(s) restaurado(s).", "role(s) restored."),
        (
            "Join gate desativado; as definições guardadas podem ser reativadas.",
            "Join gate disabled; saved settings can be re-enabled.",
        ),
        ("Lockdown aplicado em", "Lockdown applied to"),
        ("canal(is) de texto.", "text channel(s)."),
        ("Motivo:", "Reason:"),
        ("Lockdown removido de", "Lockdown removed from"),
        (
            "overwrites anteriores restaurados quando estavam guardados.",
            "previous overwrites restored when they were saved.",
        ),
        ("Shadow mode ativado:", "Shadow mode enabled:"),
        ("Shadow mode desativado:", "Shadow mode disabled:"),
        (
            "respostas anti-raid/anti-nuke ficam em observação, com casos e alertas, sem contenção automática.",
            "anti-raid/anti-nuke responses are monitored with cases and alerts, without automatic containment.",
        ),
        (
            "as respostas de segurança configuradas podem aplicar contenção limitada.",
            "configured security responses may apply limited containment.",
        ),
        (
            "Anti-raid desativado; nenhuma resposta automática a bursts de joins será aplicada.",
            "Anti-raid disabled; no automatic response to join bursts will be applied.",
        ),
        ("Anti-nuke ativado:", "Anti-nuke enabled:"),
        ("ações destrutivas em", "destructive actions in"),
        (
            "ativam contenção e alerta.",
            "trigger containment and an alert.",
        ),
        (
            "Anti-nuke desativado; os eventos de Audit Log não ativam contenção automática.",
            "Anti-nuke disabled; Audit Log events do not trigger automatic containment.",
        ),
        (
            "A data de início não é RFC3339 válida.",
            "The start date is not valid RFC3339.",
        ),
        (
            "A data de fim não é RFC3339 válida.",
            "The end date is not valid RFC3339.",
        ),
        (
            "O início tem de estar no futuro.",
            "The start must be in the future.",
        ),
        (
            "A janela do evento é inválida.",
            "The event window is invalid.",
        ),
        (
            "Não existem eventos agendados neste servidor.",
            "There are no scheduled events in this server.",
        ),
        (
            "Este evento já terminou ou foi cancelado.",
            "This event has already ended or was cancelled.",
        ),
        ("O evento **", "Event **"),
        (
            "** está cheio; ficaste na lista de espera.",
            "** is full; you were added to the waitlist.",
        ),
        (
            "Inscrição confirmada para **",
            "Registration confirmed for **",
        ),
        (
            "Já estás inscrito neste evento.",
            "You are already registered for this event.",
        ),
        ("Inscrição removida;", "Registration removed;"),
        (
            "foi promovido da lista de espera.",
            "was promoted from the waitlist.",
        ),
        (
            "Inscrição removida do evento.",
            "Registration removed from the event.",
        ),
        (
            "** ainda não tem inscrições.",
            "** has no registrations yet.",
        ),
        ("inscrição(ões)", "registration(s)"),
        (
            "Não foi possível registar o check-in.",
            "Unable to record the check-in.",
        ),
        (
            "A quota de workflows deste plano foi atingida",
            "This plan's workflow quota has been reached",
        ),
        (
            "Consulta `/plan` para saber como aumentar a capacidade da guild.",
            "Check `/plan` to learn how to increase guild capacity.",
        ),
        ("Workflow #", "Workflow #"),
        (
            "É executado quando uma mensagem corresponde à condição.",
            "Runs when a message matches the condition.",
        ),
        (
            "Não existem workflows configurados.",
            "There are no workflows configured.",
        ),
        ("Dry-run: workflow", "Dry run: workflow"),
        ("não seria executado.", "would not run."),
        ("correspondeu, mas a action", "matched, but the action"),
        ("não é suportada.", "is not supported."),
        (
            "Este canal não é um ticket do Helper.",
            "This channel is not a Helper ticket.",
        ),
        ("A equipa", "The team"),
        ("aplicada", "applied"),
        ("removida", "removed"),
        ("criado", "created"),
        ("criada", "created"),
        ("publicada", "published"),
        ("encontrado", "found"),
        ("desconhecido", "unknown"),
        ("está", "is"),
        ("estão", "are"),
        ("pode", "can"),
        ("podem", "can"),
        ("deve", "must"),
        ("devem", "must"),
        ("tem de", "must"),
        ("ter de", "must"),
        ("tens", "you have"),
        ("teu", "your"),
        ("tua", "your"),
        ("foi", "was"),
        ("foram", "were"),
        ("últimos", "last"),
        ("último", "last"),
        ("mínimo", "minimum"),
        ("atualizado", "updated"),
        ("atualizada", "updated"),
        ("guardado", "saved"),
        ("guardados", "saved"),
        ("restauro", "restoration"),
        ("restaurado", "restored"),
        ("obrigatório", "required"),
        ("opções", "options"),
        ("pergunta", "question"),
        ("prémio", "prize"),
        ("vencedores", "winners"),
        ("Termina", "Ends"),
        ("Carrega", "Click"),
        ("Vota", "Vote"),
        ("registado", "recorded"),
        ("registada", "recorded"),
        ("registro", "record"),
        ("configurado", "configured"),
        ("configurada", "configured"),
        ("configura", "configure"),
        ("desativado", "disabled"),
        ("desativada", "disabled"),
        ("ativado", "enabled"),
        ("ativada", "enabled"),
        ("segurança", "security"),
        ("seguranÃ§a", "security"),
        ("observação", "monitoring"),
        ("observaÃ§Ã£o", "monitoring"),
        ("contenção", "containment"),
        ("contenÃ§Ã£o", "containment"),
        ("Ativa-as no painel.", "Enable them in the dashboard."),
        ("Ativa-os no painel.", "Enable them in the dashboard."),
        ("Ativa-as", "Enable them"),
        ("Ativa-os", "Enable them"),
        ("ativado", "enabled"),
        ("desativado", "disabled"),
        ("ativada", "enabled"),
        ("desativada", "disabled"),
        ("servidor", "server"),
        ("membro", "member"),
        ("membros", "members"),
        ("cargo", "role"),
        ("cargos", "roles"),
        ("permissões", "permissions"),
        ("permissÃµes", "permissions"),
        ("hierarquia", "hierarchy"),
        ("canal", "channel"),
        ("canais", "channels"),
        ("mensagem", "message"),
        ("mensagens", "messages"),
        ("utilizador", "user"),
        ("utilizadores", "users"),
        ("razão", "reason"),
        ("razÃ£o", "reason"),
        ("equipa", "team"),
        ("histórico", "history"),
        ("histÃ³rico", "history"),
        ("resposta", "response"),
        ("respostas", "responses"),
        ("não", "not"),
        ("nÃ£o", "not"),
    ];
    REPLACEMENTS
        .iter()
        .fold(input.to_owned(), |text, (from, to)| text.replace(from, to))
}

fn option_string<'a>(command: &'a CommandInteraction, name: &str) -> Option<&'a str> {
    command.data.options.iter().find_map(|option| {
        (option.name == name).then_some(match &option.value {
            CommandDataOptionValue::String(value) => value.as_str(),
            _ => return None,
        })
    })
}

fn option_i64(command: &CommandInteraction, name: &str) -> Option<i64> {
    command.data.options.iter().find_map(|option| {
        (option.name == name).then_some(match option.value {
            CommandDataOptionValue::Integer(value) => value,
            _ => return None,
        })
    })
}

fn option_bool(command: &CommandInteraction, name: &str) -> Option<bool> {
    command.data.options.iter().find_map(|option| {
        (option.name == name).then_some(match option.value {
            CommandDataOptionValue::Boolean(value) => value,
            _ => return None,
        })
    })
}

fn option_role(command: &CommandInteraction, name: &str) -> Option<RoleId> {
    command.data.options.iter().find_map(|option| {
        (option.name == name).then_some(match option.value {
            CommandDataOptionValue::Role(role) => role,
            _ => return None,
        })
    })
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut output = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        output.push('…');
    }
    output
}

fn account_age_days(now_timestamp: i64, created_timestamp: i64) -> i64 {
    ((now_timestamp - created_timestamp) / 86_400).max(0)
}

fn is_destructive_audit_action(action: serenity::model::guild::audit_log::Action) -> bool {
    matches!(
        action,
        serenity::model::guild::audit_log::Action::Channel(
            serenity::model::guild::audit_log::ChannelAction::Delete,
        ) | serenity::model::guild::audit_log::Action::ChannelOverwrite(
            serenity::model::guild::audit_log::ChannelOverwriteAction::Delete,
        ) | serenity::model::guild::audit_log::Action::Role(
            serenity::model::guild::audit_log::RoleAction::Delete,
        ) | serenity::model::guild::audit_log::Action::Webhook(
            serenity::model::guild::audit_log::WebhookAction::Delete,
        ) | serenity::model::guild::audit_log::Action::Member(
            serenity::model::guild::audit_log::MemberAction::BanAdd,
        ) | serenity::model::guild::audit_log::Action::Member(
            serenity::model::guild::audit_log::MemberAction::Kick,
        )
    )
}

fn join_burst_armed(
    joins: &mut HashMap<String, VecDeque<Instant>>,
    guild_id: &str,
    now: Instant,
    window: Duration,
    threshold: usize,
) -> bool {
    let window = window.max(Duration::from_secs(1));
    let threshold = threshold.max(2);
    let entries = joins.entry(guild_id.to_owned()).or_default();
    while entries
        .front()
        .is_some_and(|joined_at| now.duration_since(*joined_at) > window)
    {
        entries.pop_front();
    }
    entries.push_back(now);
    if entries.len() < threshold {
        return false;
    }
    entries.clear();
    true
}

async fn finish_giveaway(http: &serenity::http::Http, store: &Store, id: i64) -> Result<bool> {
    let Some(giveaway) = store.giveaway(id)? else {
        return Ok(false);
    };
    if giveaway.ended || !store.end_giveaway(id)? {
        return Ok(false);
    }
    let winners = {
        let mut entries = store.giveaway_entries(id)?;
        let mut rng = rand::rng();
        entries.shuffle(&mut rng);
        entries
            .into_iter()
            .take(giveaway.winners as usize)
            .collect::<Vec<_>>()
    };
    let result = if winners.is_empty() {
        format!("🎁 Giveaway #{} ended without participants.", giveaway.id)
    } else {
        format!(
            "🎁 Giveaway #{} ended! Prize: **{}**\nWinners: {}",
            giveaway.id,
            giveaway.prize,
            winners
                .iter()
                .map(|id| format!("<@{id}>"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    let mut edited = false;
    if let Some(message_id) = giveaway
        .message_id
        .as_deref()
        .and_then(|raw| raw.parse::<u64>().ok())
        && let Ok(channel) = giveaway.channel_id.parse::<u64>()
    {
        let channel_id = ChannelId::new(channel);
        edited = channel_id
            .edit_message(
                http,
                serenity::all::MessageId::new(message_id),
                serenity::all::EditMessage::new()
                    .content(result.clone())
                    .components(Vec::new()),
            )
            .await
            .is_ok();
    }
    if !edited && let Ok(channel) = giveaway.channel_id.parse::<u64>() {
        let _ = ChannelId::new(channel).say(http, result).await;
    }
    Ok(true)
}

async fn reroll_giveaway(
    http: &serenity::http::Http,
    store: &Store,
    id: i64,
) -> Result<Option<String>> {
    let Some(giveaway) = store.giveaway(id)? else {
        return Ok(None);
    };
    if !giveaway.ended {
        return Ok(None);
    }
    let mut entries = store.giveaway_entries(id)?;
    if entries.is_empty() {
        return Ok(None);
    }
    entries.shuffle(&mut rand::rng());
    let winner = entries[0].clone();
    if let Ok(channel) = giveaway.channel_id.parse::<u64>() {
        ChannelId::new(channel)
            .say(
                http,
                format!(
                    "🎲 Giveaway #{} reroll: new winner <@{}> received **{}**.",
                    giveaway.id, winner, giveaway.prize
                ),
            )
            .await?;
    }
    Ok(Some(winner))
}

async fn finish_poll(http: &serenity::http::Http, store: &Store, id: i64) -> Result<bool> {
    let Some(poll) = store.poll(id)? else {
        return Ok(false);
    };
    if poll.closed || !store.close_poll(id)? {
        return Ok(false);
    }
    let counts = store.poll_counts(id, poll.options.len())?;
    let results = poll
        .options
        .iter()
        .enumerate()
        .map(|(index, option)| {
            format!(
                "{}️⃣ {} — {} voto(s)",
                index + 1,
                option,
                counts.get(index).copied().unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let content = format!(
        "🗳️ **Poll #{} encerrada: {}**\n{}",
        poll.id, poll.question, results
    );
    if let Some(message_id) = poll
        .message_id
        .as_deref()
        .and_then(|raw| raw.parse::<u64>().ok())
        && let Ok(channel) = poll.channel_id.parse::<u64>()
    {
        let _ = ChannelId::new(channel)
            .edit_message(
                http,
                serenity::all::MessageId::new(message_id),
                serenity::all::EditMessage::new()
                    .content(content)
                    .components(Vec::new()),
            )
            .await;
    }
    Ok(true)
}

async fn apply_lockdown(
    http: &serenity::http::Http,
    store: &Store,
    guild_id: serenity::all::GuildId,
    enabled: bool,
) -> Result<usize> {
    let channels = http.get_channels(guild_id).await?;
    let everyone = RoleId::new(guild_id.get());
    let mut changed = 0;
    for channel in channels
        .into_iter()
        .filter(|channel| matches!(channel.kind, ChannelType::Text | ChannelType::News))
    {
        let key = format!("security.lockdown.previous.{}", channel.id);
        if enabled {
            if store.get_setting(&guild_id.to_string(), &key)?.is_none() {
                if let Some(existing) = channel.permission_overwrites.iter().find(|overwrite| {
                    matches!(overwrite.kind, PermissionOverwriteType::Role(role) if role == everyone)
                }) {
                    store.set_setting(
                        &guild_id.to_string(),
                        &key,
                        &serde_json::to_string(existing)?,
                    )?;
                } else {
                    store.set_setting(&guild_id.to_string(), &key, "")?;
                }
            }
            channel
                .create_permission(
                    http,
                    PermissionOverwrite {
                        allow: Permissions::empty(),
                        deny: Permissions::SEND_MESSAGES,
                        kind: PermissionOverwriteType::Role(everyone),
                    },
                )
                .await?;
            changed += 1;
        } else if let Some(previous) = store.get_setting(&guild_id.to_string(), &key)? {
            if previous.is_empty() {
                channel
                    .delete_permission(http, PermissionOverwriteType::Role(everyone))
                    .await?;
            } else {
                let overwrite = serde_json::from_str::<PermissionOverwrite>(&previous)?;
                channel.create_permission(http, overwrite).await?;
            }
            store.delete_setting(&guild_id.to_string(), &key)?;
            changed += 1;
        }
    }
    store.set_setting(
        &guild_id.to_string(),
        "security.lockdown.active",
        if enabled { "true" } else { "false" },
    )?;
    Ok(changed)
}

fn required_permission(command: &str) -> Option<Permissions> {
    match command {
        "warn" | "violation" | "timeout" | "untimeout" | "note" | "reason" | "quarantine"
        | "unquarantine" | "modlogs" => Some(Permissions::MODERATE_MEMBERS),
        "kick" => Some(Permissions::KICK_MEMBERS),
        "ban" | "unban" | "tempban" | "softban" => Some(Permissions::BAN_MEMBERS),
        "purge" => Some(Permissions::MANAGE_MESSAGES),
        "ticket-panel" | "ticket-config" | "ticket-update" => Some(Permissions::MANAGE_CHANNELS),
        "slowmode" | "lockdown" | "unlock" => Some(Permissions::MANAGE_CHANNELS),
        "setup" | "verify-panel" => Some(Permissions::MANAGE_GUILD),
        "rolepanel" => Some(Permissions::MANAGE_ROLES),
        "join-gate" => Some(Permissions::MANAGE_ROLES),
        "anti-raid" => Some(Permissions::MANAGE_GUILD),
        "security-mode" => Some(Permissions::MANAGE_GUILD),
        "anti-nuke" => Some(Permissions::MANAGE_GUILD),
        "event-create" | "event-edit" | "event-cancel" | "event-attendees" => {
            Some(Permissions::CREATE_EVENTS)
        }
        "invites" => Some(Permissions::MANAGE_GUILD),
        "temp-channel" => Some(Permissions::MANAGE_CHANNELS),
        "embed" => Some(Permissions::MANAGE_MESSAGES),
        "tag-set" | "tag-delete" | "giveaway-start" | "giveaway-end" | "gstart" | "gend"
        | "greroll" | "starboard-set" | "workflow-create" | "workflow-dry-run"
        | "workflow-toggle" | "workflow-delete" => Some(Permissions::MANAGE_GUILD),
        "suggestion" => Some(Permissions::MANAGE_MESSAGES),
        _ => None,
    }
}

fn parse_duration(raw: &str) -> Option<i64> {
    let value = raw.trim();
    let (number, unit) = value.split_at(value.len().checked_sub(1)?);
    let amount = number.parse::<i64>().ok()?.checked_mul(match unit {
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        _ => return None,
    })?;
    (amount > 0 && amount <= 365 * 86_400_000).then_some(amount)
}

fn format_duration(milliseconds: i64) -> String {
    let units = [
        (86_400_000, "d"),
        (3_600_000, "h"),
        (60_000, "m"),
        (1_000, "s"),
    ];
    for (unit_ms, suffix) in units {
        if milliseconds % unit_ms == 0 {
            return format!("{}{}", milliseconds / unit_ms, suffix);
        }
    }
    format!("{}ms", milliseconds)
}

fn shadow_mode_enabled(raw: Option<&str>) -> bool {
    raw.is_some_and(|value| value.eq_ignore_ascii_case("true"))
}

fn feature_enabled(store: &Store, guild_id: &str, key: &str, legacy_key: Option<&str>) -> bool {
    store
        .get_setting(guild_id, &format!("feature.{key}"))
        .ok()
        .flatten()
        .and_then(|value| value.parse::<bool>().ok())
        .or_else(|| {
            legacy_key.and_then(|legacy| {
                store
                    .get_setting(guild_id, legacy)
                    .ok()
                    .flatten()
                    .and_then(|value| value.parse::<bool>().ok())
            })
        })
        .unwrap_or(false)
}

fn feature_explicitly_disabled(store: &Store, guild_id: &str, key: &str) -> bool {
    store
        .get_feature_setting(guild_id, key)
        .ok()
        .flatten()
        .map(|value| !value.enabled)
        .or_else(|| {
            store
                .get_setting(guild_id, &format!("feature.{key}"))
                .ok()
                .flatten()
                .and_then(|value| value.parse::<bool>().ok().map(|enabled| !enabled))
        })
        .unwrap_or(false)
}

fn is_moderation_command(name: &str) -> bool {
    matches!(
        name,
        "warn"
            | "violation"
            | "note"
            | "reason"
            | "kick"
            | "ban"
            | "timeout"
            | "tempban"
            | "softban"
            | "untimeout"
            | "unban"
            | "purge"
            | "quarantine"
            | "unquarantine"
            | "slowmode"
    )
}

fn setting_string(store: &Store, guild_id: &str, key: &str) -> Option<String> {
    store.get_setting(guild_id, key).ok().flatten()
}

fn setting_u64(store: &Store, guild_id: &str, key: &str, default: u64) -> u64 {
    setting_string(store, guild_id, key)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
}

fn setting_u64_optional(store: &Store, guild_id: &str, key: &str) -> Option<u64> {
    setting_string(store, guild_id, key).and_then(|value| value.parse::<u64>().ok())
}

fn setting_i64(store: &Store, guild_id: &str, key: &str, default: i64) -> i64 {
    setting_string(store, guild_id, key)
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(default)
}

fn setting_bool(store: &Store, guild_id: &str, key: &str, default: bool) -> bool {
    setting_string(store, guild_id, key)
        .and_then(|value| value.parse::<bool>().ok())
        .unwrap_or(default)
}

fn anti_spam_policy_for_store(store: &Store, guild_id: &str) -> AntiSpamPolicy {
    let csv = |key: &str| {
        setting_string(store, guild_id, key)
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    anti_spam_policy_from_json(&serde_json::json!({
        "floodCount": setting_u64(store, guild_id, "security.antispam.flood_count", 6),
        "windowSeconds": setting_u64(store, guild_id, "security.antispam.window_seconds", 10),
        "duplicateLimit": setting_u64(store, guild_id, "security.antispam.duplicate_limit", 3),
        "mentionLimit": setting_u64(store, guild_id, "security.antispam.mention_limit", 5),
        "timeoutSeconds": setting_u64(store, guild_id, "security.antispam.timeout_seconds", 60),
        "ignoredChannels": csv("security.antispam.ignored_channels"),
        "ignoredRoles": csv("security.antispam.ignored_roles"),
        "alertOnly": setting_bool(store, guild_id, "security.antispam.alert_only", false),
    }))
}

fn scam_policy_for_store(store: &Store, guild_id: &str) -> helper_core::ScamPolicy {
    let list = |key: &str| {
        setting_string(store, guild_id, key)
            .unwrap_or_default()
            .lines()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    scam_policy_from_json(&serde_json::json!({
        "blockInvites": setting_bool(store, guild_id, "security.antiscam.block_invites", true),
        "blockedDomains": list("security.antiscam.blocked_domains"),
        "blockedKeywords": list("security.antiscam.blocked_keywords"),
        "ignoredChannels": list("security.antiscam.ignored_channels"),
        "logChannel": setting_string(store, guild_id, "security.antiscam.log_channel").unwrap_or_default(),
        "timeoutSeconds": setting_u64(store, guild_id, "security.antiscam.timeout_seconds", 300),
        "alertOnly": setting_bool(store, guild_id, "security.antiscam.alert_only", false),
    }))
}

fn permission_passport_message() -> String {
    "**Permission Passport**\nBase: `View Channels`, `Send Messages`, `Embed Links`, `Read Message History`, `Use Application Commands`.\nSecurity opcional: `Manage Messages`, `Moderate Members`, `Kick Members`, `Ban Members`, `Manage Roles`.\nSupport/Events opcionais: `Manage Channels`, `Manage Threads`, `Create Private Threads`.\nGateway: `MESSAGE_CONTENT` e `GUILD_MEMBERS` só suportam módulos que precisam deles.\nCada permissão extra tem um módulo e uma consequência explícita; usa o painel para comparar o concedido com o necessário.".to_string()
}

fn parse_scheduled_event_window(
    start_raw: &str,
    end_raw: &str,
    now_timestamp: i64,
) -> Result<(serenity::all::Timestamp, serenity::all::Timestamp), &'static str> {
    let start = serenity::all::Timestamp::parse(start_raw).map_err(|_| "invalid_start")?;
    let end = serenity::all::Timestamp::parse(end_raw).map_err(|_| "invalid_end")?;
    if start.unix_timestamp() <= now_timestamp {
        return Err("start_must_be_in_future");
    }
    if end.unix_timestamp() <= start.unix_timestamp() {
        return Err("end_must_follow_start");
    }
    if end.unix_timestamp() - start.unix_timestamp() > 365 * 86_400 {
        return Err("event_too_long");
    }
    Ok((start, end))
}

async fn deliver_birthday_announcements(
    http: &serenity::http::Http,
    store: &Store,
    date: (i32, u32, u32),
) -> Result<()> {
    let (year, month, day) = date;
    let birthdays = store.due_birthdays(month, day, year, 500)?;
    for birthday in birthdays {
        if !feature_enabled(store, &birthday.guild_id, "community.birthdays", None) {
            continue;
        }
        let Some(channel_id) =
            setting_string(store, &birthday.guild_id, "community.birthdays.channel_id")
                .and_then(|value| value.parse::<u64>().ok())
        else {
            continue;
        };
        let template = setting_string(store, &birthday.guild_id, "community.birthdays.message")
            .unwrap_or_else(|| "Happy birthday, {user}! 🎉".into());
        let content = template.replace("{user}", &format!("<@{}>", birthday.user_id));
        if ChannelId::new(channel_id)
            .send_message(http, CreateMessage::new().content(content))
            .await
            .is_ok()
        {
            let _ = store.mark_birthday_announced(&birthday.guild_id, &birthday.user_id, year);
        }
    }
    Ok(())
}

async fn deliver_scheduled_action(
    http: &serenity::http::Http,
    store: &Store,
    id: i64,
    guild_id: &str,
    action_type: &str,
    target_id: &str,
    payload: &str,
) -> Result<()> {
    let value: serde_json::Value = serde_json::from_str(payload).unwrap_or_default();
    if action_type == "reminder" && !feature_enabled(store, guild_id, "utility.reminders", None) {
        // A scheduled reminder may outlive a feature disable. Consume it without
        // delivering anything so a later re-enable cannot replay stale notices.
        store.delete_scheduled_action(id)?;
        return Ok(());
    }
    if action_type == "unban" {
        let guild = guild_id
            .parse::<u64>()
            .map(serenity::all::GuildId::new)
            .map_err(|_| anyhow::anyhow!("invalid scheduled unban guild"))?;
        let user = target_id
            .parse::<u64>()
            .map(serenity::all::UserId::new)
            .map_err(|_| anyhow::anyhow!("invalid scheduled unban user"))?;
        let _ = guild.unban(http, user).await;
        store.delete_scheduled_action(id)?;
        return Ok(());
    }
    if action_type == "giveaway_end" {
        if let Some(giveaway_id) = value.get("giveaway_id").and_then(serde_json::Value::as_i64) {
            let _ = finish_giveaway(http, store, giveaway_id).await?;
        }
        store.delete_scheduled_action(id)?;
        return Ok(());
    }
    if action_type == "poll_end" {
        if let Some(poll_id) = value.get("poll_id").and_then(serde_json::Value::as_i64) {
            let _ = finish_poll(http, store, poll_id).await?;
        }
        store.delete_scheduled_action(id)?;
        return Ok(());
    }
    if action_type == "ticket_sla" {
        if let Some(raw_channel) = value.get("channel_id").and_then(serde_json::Value::as_str)
            && let Ok(Some(ticket)) = store.ticket_by_channel(raw_channel)
            && ticket.status == "open"
            && let Ok(channel) = raw_channel.parse::<u64>()
        {
            let _ = ChannelId::new(channel)
                .say(
                    http,
                    "⏱️ This ticket is waiting for a reply from the support team.",
                )
                .await;
        }
        store.delete_scheduled_action(id)?;
        return Ok(());
    }
    let channel_id = value
        .get("channel_id")
        .and_then(serde_json::Value::as_str)
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(ChannelId::new);
    if let Some(channel_id) = channel_id {
        let text = value
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("You have a pending reminder.");
        let notify_user = setting_bool(store, guild_id, "utility.reminders.notify_user", true);
        let content = if action_type == "reminder" && notify_user {
            format!("<@{target_id}> ⏰ {text}")
        } else {
            text.to_string()
        };
        channel_id
            .send_message(http, serenity::all::CreateMessage::new().content(content))
            .await?;
    }
    store.delete_scheduled_action(id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        account_age_days,
        adapter::{DiscordAdapter, Effect, FakeDiscordAdapter},
        english_bot_text, is_destructive_audit_action, join_burst_armed, parse_duration,
        parse_scheduled_event_window, shadow_mode_enabled,
    };
    use std::{
        collections::{HashMap, VecDeque},
        time::{Duration, Instant},
    };

    #[test]
    fn translates_legacy_user_facing_messages_to_english() {
        assert_eq!(
            english_bot_text("Olá <@1>. Explica aqui o que precisas."),
            "Hello <@1>. Tell us what you need."
        );
        assert_eq!(
            english_bot_text("Ticket fechado e arquivado. O canal não foi apagado."),
            "Ticket closed and archived. The channel was not deleted."
        );
        assert_eq!(
            english_bot_text("Este comando só pode ser usado num servidor."),
            "This command can only be used in a server."
        );
    }

    #[test]
    fn duration_parser_is_bounded_and_explicit() {
        assert_eq!(parse_duration("10m"), Some(600_000));
        assert_eq!(parse_duration("2h"), Some(7_200_000));
        assert_eq!(parse_duration("0m"), None);
        assert_eq!(parse_duration("10weeks"), None);
        assert_eq!(account_age_days(172800, 86400), 1);
        assert_eq!(account_age_days(86400, 172800), 0);
    }

    #[test]
    fn join_burst_arms_once_and_resets_after_threshold() {
        let mut joins: HashMap<String, VecDeque<Instant>> = HashMap::new();
        let start = Instant::now();
        let window = Duration::from_secs(10);
        assert!(!join_burst_armed(&mut joins, "guild", start, window, 3));
        assert!(!join_burst_armed(
            &mut joins,
            "guild",
            start + Duration::from_secs(1),
            window,
            3
        ));
        assert!(join_burst_armed(
            &mut joins,
            "guild",
            start + Duration::from_secs(2),
            window,
            3
        ));
        assert!(!join_burst_armed(
            &mut joins,
            "guild",
            start + Duration::from_secs(3),
            window,
            3
        ));
    }

    #[test]
    fn join_burst_ignores_expired_entries() {
        let mut joins: HashMap<String, VecDeque<Instant>> = HashMap::new();
        let start = Instant::now();
        let window = Duration::from_secs(3);
        assert!(!join_burst_armed(&mut joins, "guild", start, window, 2));
        assert!(!join_burst_armed(
            &mut joins,
            "guild",
            start + Duration::from_secs(4),
            window,
            2
        ));
    }

    #[test]
    fn anti_nuke_action_filter_is_destructive_only() {
        assert!(is_destructive_audit_action(
            serenity::model::guild::audit_log::Action::Role(
                serenity::model::guild::audit_log::RoleAction::Delete,
            )
        ));
        assert!(is_destructive_audit_action(
            serenity::model::guild::audit_log::Action::Member(
                serenity::model::guild::audit_log::MemberAction::BanAdd,
            )
        ));
        assert!(!is_destructive_audit_action(
            serenity::model::guild::audit_log::Action::Role(
                serenity::model::guild::audit_log::RoleAction::Create,
            )
        ));
        assert!(!is_destructive_audit_action(
            serenity::model::guild::audit_log::Action::GuildUpdate
        ));
    }

    #[test]
    fn scheduled_event_window_requires_rfc3339_future_and_bounded_end() {
        let now = serenity::all::Timestamp::parse("2026-07-24T00:00:00Z").unwrap();
        assert!(
            parse_scheduled_event_window(
                "2026-07-24T01:00:00Z",
                "2026-07-24T02:00:00Z",
                now.unix_timestamp()
            )
            .is_ok()
        );
        assert_eq!(
            parse_scheduled_event_window(
                "2026-07-23T23:00:00Z",
                "2026-07-24T02:00:00Z",
                now.unix_timestamp()
            )
            .unwrap_err(),
            "start_must_be_in_future"
        );
        assert_eq!(
            parse_scheduled_event_window(
                "2026-07-24T01:00:00Z",
                "2026-07-24T01:00:00Z",
                now.unix_timestamp()
            )
            .unwrap_err(),
            "end_must_follow_start"
        );
        assert_eq!(
            parse_scheduled_event_window(
                "2026-07-24T01:00:00Z",
                "2027-07-25T01:00:00Z",
                now.unix_timestamp()
            )
            .unwrap_err(),
            "event_too_long"
        );
    }

    #[test]
    fn shadow_mode_is_explicit_and_case_insensitive() {
        assert!(shadow_mode_enabled(Some("true")));
        assert!(shadow_mode_enabled(Some("TRUE")));
        assert!(!shadow_mode_enabled(Some("false")));
        assert!(!shadow_mode_enabled(None));
    }

    #[test]
    fn fake_discord_records_antispam_effects_and_surfaces_failures() {
        let mut discord = FakeDiscordAdapter::new();
        discord
            .timeout_member("guild", "member", 60, "matched:flood")
            .unwrap();
        discord
            .log("log-channel", "Anti-spam: matched:flood (action)")
            .unwrap();
        discord.reply("general", "Please slow down.").unwrap();
        assert_eq!(discord.effects().len(), 3);
        assert!(matches!(
            discord.effects()[0],
            Effect::Timeout { seconds: 60, .. }
        ));

        discord.fail_next();
        let error = discord.log("log-channel", "permission check").unwrap_err();
        assert_eq!(error, "discord_permission_denied");
        assert_eq!(discord.effects().len(), 3);
    }
}
