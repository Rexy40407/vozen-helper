/// Identifica os templates que precisam da contagem atual do Discord.
pub fn uses_member_count(template: &str) -> bool {
    template.contains("{members}") || template.contains("{count}")
}

/// Não inventa uma contagem quando o Discord não devolve esse campo.
pub fn render_member_count(template: &str, members: Option<u64>) -> Option<String> {
    if !uses_member_count(template) {
        return Some(template.to_owned());
    }
    let count = members?.to_string();
    Some(
        template
            .replace("{members}", &count)
            .replace("{count}", &count),
    )
}

/// Limita as alterações de nome para evitar pedidos repetidos ao Discord.
pub fn refresh_interval_minutes(configured: i64, members: bool) -> i64 {
    configured.clamp(if members { 10 } else { 5 }, 1_440)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_current_total_instead_of_a_frozen_count() {
        assert_eq!(
            render_member_count("📊 Members: {members}", Some(26)).as_deref(),
            Some("📊 Members: 26")
        );
    }

    #[test]
    fn supports_legacy_count_placeholder_and_zero() {
        assert_eq!(
            render_member_count("{count}/{members}", Some(0)).as_deref(),
            Some("0/0")
        );
    }

    #[test]
    fn unavailable_count_never_becomes_zero_or_a_literal_placeholder() {
        assert_eq!(render_member_count("Members: {count}", None), None);
    }

    #[test]
    fn message_only_templates_do_not_require_member_data() {
        assert_eq!(
            render_member_count("messages-{messages}", None).as_deref(),
            Some("messages-{messages}")
        );
        assert!(!uses_member_count("messages-{messages}"));
    }

    #[test]
    fn refresh_is_bounded_and_member_names_wait_at_least_ten_minutes() {
        assert_eq!(refresh_interval_minutes(5, true), 10);
        assert_eq!(refresh_interval_minutes(15, true), 15);
        assert_eq!(refresh_interval_minutes(-1, false), 5);
        assert_eq!(refresh_interval_minutes(9_999, true), 1_440);
    }
}
