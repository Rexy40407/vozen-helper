//! Discord gateway boundary. Handlers stay thin and delegate to core/modules.

use anyhow::Result;
use helper_core::Config;
use helper_modules::EntitlementClient;
use helper_store::Store;
use serenity::{
    all::{
        ButtonStyle, ChannelId, Client, Command, CommandDataOptionValue, CommandInteraction,
        Context, CreateActionRow, CreateButton, CreateChannel, CreateCommand, CreateCommandOption,
        CreateInteractionResponse, CreateInteractionResponseMessage, EventHandler, GatewayIntents,
        Interaction, PermissionOverwrite, PermissionOverwriteType, Permissions, Ready, RoleId,
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
            CreateCommand::new("ticket-panel").description("Create a support ticket panel"),
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
            *value % 5 == 0
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
                let Some(delay) = parse_duration(&time) else {
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
                self.store.upsert_tag(&guild_id.to_string(), &name, &content, &command.user.id.to_string())?;
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
                let channel = guild_id
                    .create_channel(
                        &ctx.http,
                        CreateChannel::new(format!("ticket-{}", component.user.name)).permissions(
                            [
                                PermissionOverwrite {
                                    allow: Permissions::empty(),
                                    deny: Permissions::VIEW_CHANNEL,
                                    kind: PermissionOverwriteType::Role(
                                        serenity::all::RoleId::new(guild_id.get()),
                                    ),
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
                            ],
                        ),
                    )
                    .await?;
                self.store.open_ticket(
                    &guild_id.to_string(),
                    &component.user.id.to_string(),
                    &channel.id.to_string(),
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
                if self.store.close_ticket(&component.channel_id.to_string())? {
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

fn required_permission(command: &str) -> Option<Permissions> {
    match command {
        "warn" | "violation" | "timeout" | "untimeout" | "note" | "reason" => {
            Some(Permissions::MODERATE_MEMBERS)
        }
        "kick" => Some(Permissions::KICK_MEMBERS),
        "ban" | "unban" => Some(Permissions::BAN_MEMBERS),
        "purge" => Some(Permissions::MANAGE_MESSAGES),
        "ticket-panel" => Some(Permissions::MANAGE_CHANNELS),
        "rolepanel" => Some(Permissions::MANAGE_ROLES),
        "tag-set" | "tag-delete" => Some(Permissions::MANAGE_GUILD),
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

#[cfg(test)]
mod tests {
    use super::parse_duration;

    #[test]
    fn duration_parser_is_bounded_and_explicit() {
        assert_eq!(parse_duration("10m"), Some(600_000));
        assert_eq!(parse_duration("2h"), Some(7_200_000));
        assert_eq!(parse_duration("0m"), None);
        assert_eq!(parse_duration("10weeks"), None);
    }
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
