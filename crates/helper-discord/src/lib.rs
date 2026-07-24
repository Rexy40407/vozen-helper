//! Discord gateway boundary. Handlers stay thin and delegate to core/modules.

use anyhow::Result;
use helper_core::Config;
use helper_modules::EntitlementClient;
use helper_store::Store;
use rand::seq::SliceRandom;
use serenity::{
    all::{
        ButtonStyle, ChannelId, Client, Command, CommandDataOptionValue, CommandInteraction,
        Context, CreateActionRow, CreateButton, CreateChannel, CreateCommand, CreateCommandOption,
        CreateInteractionResponse, CreateInteractionResponseMessage, EditChannel, EventHandler,
        GatewayIntents, Interaction, PermissionOverwrite, PermissionOverwriteType, Permissions,
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
use tracing::info;

#[derive(Clone)]
struct Handler {
    store: Store,
    spam: Arc<Mutex<HashMap<String, VecDeque<Instant>>>>,
    xp_ticks: Arc<Mutex<HashMap<String, u32>>>,
    scheduler_started: Arc<AtomicBool>,
    entitlements: Option<EntitlementClient>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!(user = %ready.user.name, "helper gateway ready");
        if !self.scheduler_started.swap(true, Ordering::AcqRel) {
            let store = self.store.clone();
            let http = ctx.http.clone();
            tokio::spawn(async move {
                loop {
                    if let Ok(actions) =
                        store.due_scheduled_actions(chrono::Utc::now().timestamp_millis(), 100)
                    {
                        for (id, _guild_id, action_type, target_id, payload) in actions {
                            let _ = deliver_scheduled_action(
                                &http,
                                &store,
                                id,
                                &action_type,
                                &target_id,
                                &payload,
                            )
                            .await;
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            });
        }
        let commands = vec![
            CreateCommand::new("ping").description("Check Helper latency"),
            CreateCommand::new("help").description("Show Helper modules"),
            CreateCommand::new("dashboard").description("Open the Helper dashboard"),
            CreateCommand::new("plan").description("Show the active Vozen plan"),
            CreateCommand::new("cases").description("List recent moderation cases"),
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
            CreateCommand::new("serverstats").description("Show basic server statistics"),
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
            CreateCommand::new("giveaway-list").description("List active giveaways"),
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
                        "contains",
                        "Only run when message contains this text",
                    )
                    .required(false),
                )
                .add_option(
                    CreateCommandOption::new(
                        serenity::all::CommandOptionType::String,
                        "reply",
                        "Reply text; use {user} and {message}",
                    )
                    .required(true),
                ),
            CreateCommand::new("workflow-list").description("List message automations"),
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
        ];
        if let Err(error) = Command::set_global_commands(&ctx.http, commands).await {
            tracing::error!(%error, "global command registration failed");
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
                                    .content("Não foi possível concluir esta ação.")
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
                    "Join gate: conta com {account_age_days} dia(s); mínimo configurado {minimum_age}"
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
                                "⚠️ <@{}> precisa de verificação: conta demasiado recente.",
                                new_member.user.id
                            ),
                        )
                        .await;
                }
            }
        }
        if let Ok(guild) = guild_id.to_partial_guild(&ctx.http).await
            && let Some(channel_id) = guild.system_channel_id
        {
            let _ = channel_id
                .say(
                    &ctx.http,
                    format!("👋 Bem-vindo ao servidor, <@{}>!", new_member.user.id),
                )
                .await;
        }
    }

    async fn guild_member_removal(
        &self,
        _ctx: Context,
        guild_id: serenity::all::GuildId,
        _user: serenity::all::User,
        _member_data_if_available: Option<serenity::all::Member>,
    ) {
        let day = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let _ = self.store.record_leave(&guild_id.to_string(), &day);
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

    async fn reaction_add(&self, ctx: Context, reaction: serenity::all::Reaction) {
        let Some(guild_id) = reaction.guild_id else {
            return;
        };
        if reaction.user_id == ctx.http.get_current_user().await.ok().map(|user| user.id) {
            return;
        }
        let serenity::all::ReactionType::Unicode(emoji) = &reaction.emoji else {
            return;
        };
        if emoji != "⭐" && emoji != "🌟" {
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
        let link = format!(
            "https://discord.com/channels/{}/{}/{}",
            guild_id, reaction.channel_id, reaction.message_id
        );
        if count < 3 {
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
        let xp_key = format!("{guild_text}:{user_text}");
        let should_award = {
            let mut ticks = self.xp_ticks.lock().expect("xp mutex poisoned");
            let value = ticks.entry(xp_key).or_default();
            *value = value.saturating_add(1);
            (*value).is_multiple_of(5)
        };
        if should_award {
            let _ = self.store.add_xp(&guild_text, &user_text, 5);
        }
        if let Ok(Some(afk)) = self.store.get_afk(&guild_text, &user_text) {
            let _ = self.store.clear_afk(&guild_text, &user_text);
            let _ = message
                .channel_id
                .say(
                    &ctx.http,
                    format!(
                        "Bem-vindo de volta, <@{}>. O teu AFK foi removido.",
                        afk.user_id
                    ),
                )
                .await;
        }
        let key = format!("{}:{}", guild_id, message.author.id);
        let now = Instant::now();
        let count = {
            let mut states = self.spam.lock().expect("spam mutex poisoned");
            let window = states.entry(key).or_default();
            while window
                .front()
                .is_some_and(|at| now.duration_since(*at) > Duration::from_secs(10))
            {
                window.pop_front();
            }
            window.push_back(now);
            window.len()
        };
        if count == 7 {
            let _ = self.store.record_case(
                &guild_id.to_string(),
                "anti-spam",
                &message.author.id.to_string(),
                &message.author.id.to_string(),
                "Too many messages in a short window",
                None,
            );
            let _ = message
                .channel_id
                .say(
                    &ctx.http,
                    format!(
                        "<@{}>, abranda o ritmo — o anti-spam registou este incidente.",
                        message.author.id
                    ),
                )
                .await;
        }
        if let Ok(workflows) = self.store.active_workflows(&guild_text, "message") {
            let lower = message.content.to_lowercase();
            for workflow in workflows {
                if !workflow.condition.is_empty()
                    && !lower.contains(&workflow.condition.to_lowercase())
                {
                    continue;
                }
                if workflow.action != "reply" {
                    continue;
                }
                let reply = workflow
                    .payload
                    .replace("{user}", &format!("<@{}>", message.author.id))
                    .replace("{message}", &truncate(&message.content, 500));
                let _ = message
                    .channel_id
                    .say(&ctx.http, truncate(&reply, 1_500))
                    .await;
                let _ = self.store.record_workflow_run(
                    workflow.id,
                    &guild_text,
                    &message.id.to_string(),
                );
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
        | GatewayIntents::GUILD_WEBHOOKS
        | GatewayIntents::AUTO_MODERATION_CONFIGURATION
        | GatewayIntents::AUTO_MODERATION_EXECUTION
        | GatewayIntents::MESSAGE_CONTENT;
    let store = Store::open(&config.database_url)?;
    let mut client = Client::builder(&config.discord_token, intents)
        .event_handler(Handler {
            store,
            spam: Arc::new(Mutex::new(HashMap::new())),
            xp_ticks: Arc::new(Mutex::new(HashMap::new())),
            scheduler_started: Arc::new(AtomicBool::new(false)),
            entitlements: EntitlementClient::new(
                config.entitlement_url.clone(),
                config.entitlement_secret.clone(),
            ),
        })
        .application_id(config.discord_application_id.into())
        .await?;
    client.start().await?;
    Ok(())
}

impl Handler {
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
        let content = match command.data.name.as_str() {
            "ping" => "Pong — Vozen Helper está online.".to_string(),
            "help" => "Vozen Helper público: Core, Studio, Security, Support, Events, Community, Automate e Insights. Usa /dashboard para configurar o servidor.".to_string(),
            "dashboard" => "Painel: https://helper.vozen.org (o endpoint permanece desligado até o rollout aprovado).".to_string(),
            "plan" => {
                if let Some(client) = &self.entitlements {
                    match client.resolve(&command.user.id.to_string(), command.guild_id.map(|id| id.to_string()).as_deref()).await {
                        Ok(snapshot) => { let label = match &snapshot.plan { helper_contracts::Plan::Free => "Free", helper_contracts::Plan::Plus => "Plus", helper_contracts::Plan::Premium { .. } => "Premium" }; format!("Plano {label} · {} guild(s) · entitlements v{}.", snapshot.plan.guild_limit(), snapshot.version) },
                        Err(error) => { tracing::warn!(%error, "central entitlement lookup failed"); "Não foi possível consultar o plano agora; o Helper mantém o último snapshot seguro.".to_string() }
                    }
                } else { "Entitlements central ainda não estão configurados nesta instalação.".to_string() }
            }
            "cases" => {
                if let Some(guild_id) = command.guild_id {
                    let cases = self.store.recent_cases(&guild_id.to_string(), 10)?;
                    if cases.is_empty() { "Ainda não existem casos neste servidor.".to_string() } else { cases.into_iter().map(|case_record| format!("#{} {} <@{}>: {}", case_record.id, case_record.kind, case_record.target_id, case_record.reason)).collect::<Vec<_>>().join("\n") }
                } else { "Este comando só pode ser usado num servidor.".to_string() }
            }
            "warn" => {
                if let Some(guild_id) = command.guild_id {
                    let target = command.data.options.iter().find_map(|option| match &option.value { CommandDataOptionValue::User(user) => Some(*user), _ => None });
                    if let Some(target) = target {
                        let reason = command.data.options.iter().find_map(|option| match &option.value { CommandDataOptionValue::String(value) => Some(value.as_str()), _ => None }).unwrap_or("Sem motivo");
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
                let count = raw_count.clamp(1, 100) as u8;
                let messages = command.channel_id.messages(&ctx.http, serenity::all::GetMessages::new().limit(count)).await?;
                if messages.is_empty() {
                    "Não encontrei mensagens para apagar.".to_string()
                } else {
                    let ids: Vec<_> = messages.iter().map(|message| message.id).collect();
                    command.channel_id.delete_messages(&ctx.http, ids).await?;
                    format!("{} mensagens apagadas.", messages.len())
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
                }).unwrap_or("Sem motivo");
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
                let time = option_string(command, "time").unwrap_or_default();
                let text = option_string(command, "text").unwrap_or_default();
                let Some(delay) = parse_duration(time) else {
                    return respond(ctx, command, "Duração inválida. Usa formatos como 10m, 2h ou 1d.").await;
                };
                if text.len() > 500 {
                    return respond(ctx, command, "O lembrete não pode exceder 500 caracteres.").await;
                }
                let id = self.store.schedule(
                    &guild_id.to_string(),
                    &command.user.id.to_string(),
                    chrono::Utc::now().timestamp_millis() + delay,
                    &serde_json::json!({"channel_id": command.channel_id.to_string(), "text": text}).to_string(),
                )?;
                format!("Lembrete #{id} agendado.")
            }
            "tag" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
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
                let names = self.store.list_tags(&guild_id.to_string(), 100)?;
                if names.is_empty() { "Ainda não existem tags.".to_string() } else { names.join(", ") }
            }
            "tag-set" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let name = option_string(command, "name").unwrap_or_default().to_lowercase();
                let content = option_string(command, "content").unwrap_or_default();
                if !(1..=32).contains(&name.len()) || content.len() > 1_000 {
                    return respond(ctx, command, "Nome ou conteúdo inválido.").await;
                }
                self.store.upsert_tag(&guild_id.to_string(), &name, content, &command.user.id.to_string())?;
                format!("Tag `{name}` guardada.")
            }
            "tag-delete" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let name = option_string(command, "name").unwrap_or_default().to_lowercase();
                if self.store.delete_tag(&guild_id.to_string(), &name)? { format!("Tag `{name}` eliminada.") } else { "Tag não encontrada.".to_string() }
            }
            "rank" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let user = command.data.options.iter().find_map(|option| match option.value { CommandDataOptionValue::User(user) => Some(user), _ => None }).unwrap_or(command.user.id);
                let xp = self.store.level_for(&guild_id.to_string(), &user.to_string())?;
                let level = (xp / 100) + 1;
                format!("<@{}> está no nível {} com {} XP.", user, level, xp)
            }
            "leaderboard" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let rows = self.store.top_levels(&guild_id.to_string(), 10)?;
                if rows.is_empty() { "Ainda não existem dados de XP.".to_string() } else { rows.into_iter().enumerate().map(|(index, row)| format!("{}. <@{}> — {} XP", index + 1, row.user_id, row.xp)).collect::<Vec<_>>().join("\n") }
            }
            "serverstats" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let rows = self.store.stats_for(&guild_id.to_string(), 7)?;
                let messages: i64 = rows.iter().map(|(_, messages, _, _)| messages).sum();
                format!("Mensagens registadas nos últimos {} dias: {}.", rows.len(), messages)
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
                let text = option_string(command, "text").unwrap_or_default().trim();
                if !(3..=1_000).contains(&text.len()) {
                    return respond(ctx, command, "A sugestão deve ter entre 3 e 1000 caracteres.").await;
                }
                let id = self.store.create_suggestion(&guild_id.to_string(), &command.user.id.to_string(), text)?;
                let message = command.channel_id.send_message(&ctx.http, serenity::all::CreateMessage::new()
                    .content(format!("**Sugestão #{id}** por <@{}>\n{text}\n\nVota nesta sugestão:", command.user.id))
                    .components(vec![CreateActionRow::Buttons(vec![
                        CreateButton::new(format!("suggest:up:{id}")).label("Apoio").style(ButtonStyle::Success),
                        CreateButton::new(format!("suggest:down:{id}")).label("Contra").style(ButtonStyle::Danger),
                    ])])).await?;
                self.store.set_suggestion_message(id, &message.id.to_string())?;
                format!("Sugestão #{id} publicada.")
            }
            "suggestion" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
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
            "giveaway-start" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
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
                    .content(format!("🎉 **Giveaway #{id}**\nPrémio: **{prize}**\nVencedores: **{winners}**\nTermina <t:{}:R>\nCarrega no botão para participar.", end_at / 1_000))
                    .components(vec![CreateActionRow::Buttons(vec![CreateButton::new(format!("giveaway:join:{id}")).label("Participar").style(ButtonStyle::Primary)])])).await?;
                self.store.set_giveaway_message(id, &message.id.to_string())?;
                self.store.schedule_typed(&guild_id.to_string(), "giveaway_end", &command.user.id.to_string(), end_at, &serde_json::json!({"channel_id": command.channel_id.to_string(), "giveaway_id": id}).to_string())?;
                format!("Giveaway #{id} criado.")
            }
            "giveaway-end" => {
                let id = option_i64(command, "id").unwrap_or(0);
                if finish_giveaway(&ctx.http, &self.store, id).await? {
                    format!("Giveaway #{id} terminado.")
                } else {
                    "Giveaway não encontrado ou já terminado.".to_string()
                }
            }
            "giveaway-list" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let rows = self.store.active_giveaways(&guild_id.to_string(), 20)?;
                if rows.is_empty() { "Não existem giveaways ativos.".to_string() } else {
                    rows.into_iter().map(|row| format!("#{} — {} — termina <t:{}:R>", row.id, row.prize, row.end_at / 1_000)).collect::<Vec<_>>().join("\n")
                }
            }
            "poll" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
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
                    .content(format!("🗳️ **Poll #{id}: {question}**\n{}\nTermina <t:{}:R>", options.iter().enumerate().map(|(i, v)| format!("{}️⃣ {}", i + 1, v)).collect::<Vec<_>>().join("\n"), end_at / 1_000))
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
            "workflow-create" => {
                let Some(guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let name = option_string(command, "name").unwrap_or_default().trim();
                let condition = option_string(command, "contains").unwrap_or_default().trim();
                let reply = option_string(command, "reply").unwrap_or_default().trim();
                if !(1..=50).contains(&name.len()) || !(1..=1_000).contains(&reply.len()) || condition.len() > 200 {
                    return respond(ctx, command, "Nome, condição ou resposta inválidos.").await;
                }
                let id = self.store.create_workflow(&guild_id.to_string(), name, "message", condition, "reply", reply)?;
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
                let Some(_guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                command
                    .channel_id
                    .send_message(
                        &ctx.http,
                        serenity::all::CreateMessage::new()
                            .content("Precisas de ajuda? Abre um ticket privado.")
                            .components(vec![CreateActionRow::Buttons(vec![
                                CreateButton::new("ticket:open")
                                    .label("Abrir ticket")
                                    .style(ButtonStyle::Primary),
                            ])]),
                    )
                    .await?;
                format!("Painel de tickets criado em <#{}>.", command.channel_id)
            }
            "rolepanel" => {
                let Some(_guild_id) = command.guild_id else {
                    return respond(ctx, command, "Este comando só pode ser usado num servidor.").await;
                };
                let title = option_string(command, "title").unwrap_or("Escolhe os teus cargos");
                let mut buttons = Vec::new();
                for name in ["role1", "role2", "role3", "role4", "role5"] {
                    if let Some(role_id) = command.data.options.iter().find_map(|option| {
                        (option.name == name).then_some(match option.value {
                            CommandDataOptionValue::Role(role) => role,
                            _ => return None,
                        })
                    }) {
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
                command
                    .channel_id
                    .send_message(
                        &ctx.http,
                        serenity::all::CreateMessage::new()
                            .content(title)
                            .components(vec![CreateActionRow::Buttons(buttons)]),
                    )
                    .await?;
                "Painel de cargos criado.".to_string()
            }
            _ => "Comando desconhecido.".to_string(),
        };
        command
            .create_response(
                ctx,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content(content)
                        .ephemeral(command.data.name != "ping"),
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
            let member = guild_id.member(&ctx.http, component.user.id).await?;
            if member.roles.contains(&role_id) {
                member.remove_role(&ctx.http, role_id).await?;
                return respond_component(ctx, component, "Cargo removido.").await;
            }
            member.add_role(&ctx.http, role_id).await?;
            return respond_component(ctx, component, "Cargo atribuído.").await;
        }
        match component.data.custom_id.as_str() {
            "ticket:open" => {
                if let Some(ticket) = self
                    .store
                    .active_ticket_for_user(&guild_id.to_string(), &component.user.id.to_string())?
                {
                    return respond_component(
                        ctx,
                        component,
                        &format!("Já tens um ticket aberto: <#{}>.", ticket.channel_id),
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
                                "Olá <@{}>. Explica aqui o que precisas.",
                                component.user.id
                            ))
                            .components(vec![CreateActionRow::Buttons(vec![
                                CreateButton::new("ticket:claim")
                                    .label("Assumir")
                                    .style(ButtonStyle::Secondary),
                                CreateButton::new("ticket:close")
                                    .label("Fechar")
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
                            format!("Ticket assumido por <@{}>.", component.user.id),
                        )
                        .await?;
                    respond_component(ctx, component, "Ticket assumido.").await
                } else {
                    respond_component(ctx, component, "Este ticket já foi fechado.").await
                }
            }
            "ticket:close" => {
                let ticket = self
                    .store
                    .ticket_by_channel(&component.channel_id.to_string())?;
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
                        let mut transcript = format!("Transcript do ticket de {opener}\n");
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
                    respond_component(ctx, component, "Ticket fechado. O canal será removido.")
                        .await?;
                    component.channel_id.delete(&ctx.http).await?;
                } else {
                    respond_component(ctx, component, "Este ticket já está fechado.").await?;
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
                    .content(content)
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
                    .content(content)
                    .ephemeral(true),
            ),
        )
        .await?;
    Ok(())
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
        format!("🎁 Giveaway #{} terminou sem participantes.", giveaway.id)
    } else {
        format!(
            "🎁 Giveaway #{} terminou! Prémio: **{}**\nVencedores: {}",
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

fn required_permission(command: &str) -> Option<Permissions> {
    match command {
        "warn" | "violation" | "timeout" | "untimeout" | "note" | "reason" | "quarantine"
        | "unquarantine" => Some(Permissions::MODERATE_MEMBERS),
        "kick" => Some(Permissions::KICK_MEMBERS),
        "ban" | "unban" => Some(Permissions::BAN_MEMBERS),
        "purge" => Some(Permissions::MANAGE_MESSAGES),
        "ticket-panel" | "ticket-config" => Some(Permissions::MANAGE_CHANNELS),
        "slowmode" => Some(Permissions::MANAGE_CHANNELS),
        "rolepanel" => Some(Permissions::MANAGE_ROLES),
        "join-gate" => Some(Permissions::MANAGE_ROLES),
        "tag-set" | "tag-delete" | "giveaway-start" | "giveaway-end" | "starboard-set"
        | "workflow-create" | "workflow-delete" => Some(Permissions::MANAGE_GUILD),
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

async fn deliver_scheduled_action(
    http: &serenity::http::Http,
    store: &Store,
    id: i64,
    action_type: &str,
    target_id: &str,
    payload: &str,
) -> Result<()> {
    let value: serde_json::Value = serde_json::from_str(payload).unwrap_or_default();
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
                    "⏱️ Este ticket aguarda resposta da equipa de suporte.",
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
            .unwrap_or("Tens um lembrete pendente.");
        let content = if action_type == "reminder" {
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
    use super::{account_age_days, parse_duration};

    #[test]
    fn duration_parser_is_bounded_and_explicit() {
        assert_eq!(parse_duration("10m"), Some(600_000));
        assert_eq!(parse_duration("2h"), Some(7_200_000));
        assert_eq!(parse_duration("0m"), None);
        assert_eq!(parse_duration("10weeks"), None);
        assert_eq!(account_age_days(172800, 86400), 1);
        assert_eq!(account_age_days(86400, 172800), 0);
    }
}
