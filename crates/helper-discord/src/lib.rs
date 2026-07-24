//! Discord gateway boundary. Handlers stay thin and delegate to core/modules.

use anyhow::Result;
use helper_core::Config;
use helper_modules::EntitlementClient;
use helper_store::Store;
use serenity::{
    all::{
        Client, Command, CommandDataOptionValue, CommandInteraction, Context, CreateCommand,
        CreateCommandOption, CreateInteractionResponse, CreateInteractionResponseMessage,
        EventHandler, GatewayIntents, Interaction, Ready,
    },
    async_trait,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tracing::info;

#[derive(Clone)]
struct Handler {
    store: Store,
    spam: Arc<Mutex<HashMap<String, VecDeque<Instant>>>>,
    entitlements: Option<EntitlementClient>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!(user = %ready.user.name, "helper gateway ready");
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
        ];
        if let Err(error) = Command::set_global_commands(&ctx.http, commands).await {
            tracing::error!(%error, "global command registration failed");
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let Interaction::Command(command) = interaction else {
            return;
        };
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

    async fn message(&self, ctx: Context, message: serenity::all::Message) {
        if message.author.bot {
            return;
        }
        let Some(guild_id) = message.guild_id else {
            return;
        };
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
