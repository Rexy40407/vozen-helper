use super::*;

impl Store {
    /// Read-through compatibility, never a destructive migration. Once the
    /// unified feature is saved, legacy state cannot reactivate it.
    pub fn welcome_configuration(&self, guild: &str) -> Result<(bool, serde_json::Value)> {
        let (primary_enabled, mut primary) = self.welcome_variant(guild, false)?;
        if self
            .get_setting(guild, "support.welcome.unified")?
            .as_deref()
            == Some("true")
            || primary.get("guideEnabled").is_some()
        {
            if primary.get("guideEnabled").is_none() {
                primary["guideEnabled"] = serde_json::json!(false);
            }
            return Ok((primary_enabled, primary));
        }
        let (legacy_enabled, legacy) = self.welcome_variant(guild, true)?;
        for field in [
            "steps",
            "rulesChannel",
            "introductionsChannel",
            "channelsChannel",
        ] {
            if let Some(value) = legacy.get(field) {
                primary[field] = value.clone();
            }
        }
        if !primary_enabled && legacy_enabled {
            // A previously disabled plain welcome must not gain new DM/role
            // side effects merely because its guide was active.
            primary["sendDm"] = serde_json::json!(false);
            primary["autoRole"] = serde_json::json!("");
            primary["farewellChannel"] = serde_json::json!("");
            primary["farewellMessage"] = serde_json::json!("");
            primary["templateId"] = serde_json::json!("");
            for (old, new) in [
                ("channelId", "channel"),
                ("message", "message"),
                ("templateId", "templateId"),
            ] {
                if let Some(value) = legacy.get(old) {
                    primary[new] = value.clone();
                }
            }
        }
        primary["guideEnabled"] = serde_json::json!(legacy_enabled);
        if primary.get("steps").is_none() {
            primary["steps"] = serde_json::json!(["rules", "introductions", "channels"]);
        }
        Ok((primary_enabled || legacy_enabled, primary))
    }

    fn welcome_variant(&self, guild: &str, guided: bool) -> Result<(bool, serde_json::Value)> {
        let key = if guided {
            "support.welcome_channel"
        } else {
            "support.welcome"
        };
        let stored = self.get_feature_setting(guild, key)?;
        let enabled = if let Some(record) = &stored {
            record.enabled
        } else {
            self.get_setting(guild, &format!("feature.{key}"))?
                .as_deref()
                == Some("true")
        };
        let raw = stored
            .map(|record| record.config_json)
            .or(self.get_setting(guild, &format!("feature.config.{key}"))?);
        let mut config = raw
            .map(|raw| serde_json::from_str::<serde_json::Value>(&raw))
            .transpose()?
            .unwrap_or(serde_json::json!({}));
        anyhow::ensure!(config.is_object(), "invalid welcome configuration");
        for (field, suffix) in [
            (if guided { "channelId" } else { "channel" }, "channel_id"),
            ("message", "message"),
            ("templateId", "template_id"),
            ("dmMessage", "dm_message"),
            ("autoRole", "auto_role"),
            ("farewellChannel", "farewell_channel_id"),
            ("farewellMessage", "farewell_message"),
            ("rulesChannel", "rules_channel"),
            ("introductionsChannel", "introductions_channel"),
            ("channelsChannel", "channels_channel"),
        ] {
            if config.get(field).is_none()
                && let Some(value) = self.get_setting(guild, &format!("{key}.{suffix}"))?
            {
                config[field] = serde_json::json!(value);
            }
        }
        for (field, suffix) in [
            ("sendDm", "send_dm"),
            ("delaySeconds", "delay_seconds"),
            ("steps", "steps"),
        ] {
            if config.get(field).is_none()
                && let Some(raw) = self.get_setting(guild, &format!("{key}.{suffix}"))?
            {
                config[field] = match field {
                    "steps" => serde_json::json!(
                        raw.split(',').filter(|s| !s.is_empty()).collect::<Vec<_>>()
                    ),
                    "sendDm" => serde_json::json!(raw == "true"),
                    _ => serde_json::json!(raw.parse::<u64>().unwrap_or(0)),
                };
            }
        }
        Ok((enabled, config))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unified_welcome_preserves_legacy_guide_without_two_deliveries() {
        let store = Store::open(":memory:").unwrap();
        store
            .set_setting("a", "feature.support.welcome_channel", "true")
            .unwrap();
        store
            .set_setting(
                "a",
                "feature.config.support.welcome_channel",
                r#"{"channelId":"123","message":"Guide","steps":["rules"],"rulesChannel":"456"}"#,
            )
            .unwrap();
        let (enabled, config) = store.welcome_configuration("a").unwrap();
        assert!(enabled);
        assert_eq!(config["channel"], "123");
        assert_eq!(config["message"], "Guide");
        assert_eq!(config["guideEnabled"], true);
        assert_eq!(config["rulesChannel"], "456");
        assert!(!store.welcome_configuration("b").unwrap().0);
        assert_eq!(store.welcome_configuration("a").unwrap().1, config);
    }
    #[test]
    fn unified_welcome_prefers_primary_message_and_respects_explicit_disable() {
        let store = Store::open(":memory:").unwrap();
        store
            .set_setting("a", "feature.support.welcome", "true")
            .unwrap();
        store
            .set_setting(
                "a",
                "feature.config.support.welcome",
                r#"{"channel":"123","message":"Hello","sendDm":true}"#,
            )
            .unwrap();
        store
            .set_setting("a", "feature.support.welcome_channel", "true")
            .unwrap();
        store
            .set_setting(
                "a",
                "feature.config.support.welcome_channel",
                r#"{"channelId":"456","message":"Guide"}"#,
            )
            .unwrap();
        let (_, config) = store.welcome_configuration("a").unwrap();
        assert_eq!(config["channel"], "123");
        assert_eq!(config["message"], "Hello");
        assert_eq!(config["sendDm"], true);
        store
            .set_setting("a", "support.welcome.unified", "true")
            .unwrap();
        store
            .set_setting("a", "feature.support.welcome", "false")
            .unwrap();
        assert!(!store.welcome_configuration("a").unwrap().0);
        assert_eq!(
            store.welcome_configuration("a").unwrap().1["guideEnabled"],
            false
        );
    }

    #[test]
    fn unified_welcome_save_is_authoritative_and_does_not_erase_legacy() {
        let store = Store::open(":memory:").unwrap();
        store
            .set_setting("a", "feature.support.welcome_channel", "true")
            .unwrap();
        store
            .set_setting(
                "a",
                "feature.config.support.welcome",
                r#"{"sendDm":true,"autoRole":"999"}"#,
            )
            .unwrap();
        let (_, config) = store.welcome_configuration("a").unwrap();
        assert_eq!(config["sendDm"], false);
        assert_eq!(config["autoRole"], "");
        store
            .publish_feature_setting("a", "support.welcome", false, "{}", Some(0), "test", &[])
            .unwrap();
        assert!(!store.welcome_configuration("a").unwrap().0);
        assert_eq!(
            store
                .get_setting("a", "feature.support.welcome_channel")
                .unwrap()
                .as_deref(),
            Some("true")
        );
        assert_eq!(
            store
                .get_feature_setting("a", "support.welcome")
                .unwrap()
                .unwrap()
                .revision,
            1
        );
    }
}
