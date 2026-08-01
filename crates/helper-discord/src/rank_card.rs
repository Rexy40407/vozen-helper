use base64::{Engine, engine::general_purpose::STANDARD};
use helper_contracts::RankCardConfig;

const WIDTH: u32 = 900;
const HEIGHT: u32 = 300;

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn safe_hex<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.len() == 7
        && value.starts_with('#')
        && value.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
    {
        value
    } else {
        fallback
    }
}

fn font_family(font: &str) -> &'static str {
    match font {
        "inter" => "Inter, Arial, sans-serif",
        "roboto" => "Roboto, Arial, sans-serif",
        "poppins" => "Poppins, Arial, sans-serif",
        "space_grotesk" => "'Space Grotesk', Arial, sans-serif",
        "lexend" => "Lexend, Arial, sans-serif",
        _ => "system-ui, -apple-system, Segoe UI, sans-serif",
    }
}

fn progress_width(xp: i64) -> f32 {
    (xp.rem_euclid(100) as f32 / 100.0) * 560.0
}

fn preset_data_uri(preset: &str) -> Option<String> {
    let bytes: &[u8] = match preset {
        "aurora-lake" => {
            include_bytes!("../../../assets/rank-card-banners/banner-01-aurora-lake.png")
        }
        "neon-rain" => include_bytes!("../../../assets/rank-card-banners/banner-02-neon-rain.png"),
        "enchanted-forest" => {
            include_bytes!("../../../assets/rank-card-banners/banner-03-enchanted-forest.png")
        }
        "desert-ruins" => {
            include_bytes!("../../../assets/rank-card-banners/banner-04-desert-ruins.png")
        }
        "coral-cavern" => {
            include_bytes!("../../../assets/rank-card-banners/banner-05-coral-cavern.png")
        }
        "sky-islands" => {
            include_bytes!("../../../assets/rank-card-banners/banner-06-sky-islands.png")
        }
        "volcanic-forge" => {
            include_bytes!("../../../assets/rank-card-banners/banner-07-volcanic-forge.png")
        }
        "moonlit-village" => {
            include_bytes!("../../../assets/rank-card-banners/banner-08-moonlit-village.png")
        }
        "starship-hangar" => {
            include_bytes!("../../../assets/rank-card-banners/banner-09-starship-hangar.png")
        }
        "lavender-storm" => {
            include_bytes!("../../../assets/rank-card-banners/banner-10-lavender-storm.png")
        }
        _ => return None,
    };
    Some(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}

/// Produces a self-contained SVG so Discord can display the rank card without
/// a native canvas dependency. Curated banner presets are embedded directly;
/// profile URLs are XML-escaped here.
pub fn render_rank_card(
    config: &RankCardConfig,
    username: &str,
    avatar_url: Option<&str>,
    rank: Option<u64>,
    level: i64,
    xp: i64,
) -> String {
    let background = safe_hex(&config.background_color, "#101725");
    let primary = safe_hex(&config.primary_color, "#8EE5D2");
    let text = safe_hex(&config.text_color, "#F4F7FB");
    let ring = safe_hex(&config.avatar_ring_color, primary);
    let opacity = config.overlay_opacity.clamp(0.0, 0.85);
    let ring_width = config.avatar_ring_width.min(8);
    let username = xml_escape(username);
    let rank_text = rank.map_or_else(|| "—".to_string(), |value| format!("#{value}"));
    let bg_source = config
        .background_preset
        .as_deref()
        .and_then(preset_data_uri);
    let bg_image = bg_source
        .as_deref()
        .map(xml_escape)
        .map(|url| {
            format!(
                r#"<image href="{url}" x="0" y="0" width="{WIDTH}" height="{HEIGHT}" preserveAspectRatio="xMidYMid slice"/>"#
            )
        })
        .unwrap_or_default();
    let avatar_image = avatar_url
        .map(xml_escape)
        .map(|url| {
            format!(
                r#"<image href="{url}" x="58" y="58" width="184" height="184" preserveAspectRatio="xMidYMid slice" clip-path="url(#avatar-clip)"/>"#
            )
        })
        .unwrap_or_default();
    let width = progress_width(xp).max(8.0);
    let xp_current = xp.rem_euclid(100);
    let font = font_family(&config.font);

    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" viewBox="0 0 {WIDTH} {HEIGHT}">
  <defs>
    <linearGradient id="panel" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="{primary}" stop-opacity=".18"/><stop offset="1" stop-color="{background}" stop-opacity=".96"/></linearGradient>
    <clipPath id="avatar-clip"><circle cx="150" cy="150" r="92"/></clipPath>
  </defs>
  <rect width="{WIDTH}" height="{HEIGHT}" rx="22" fill="{background}"/>
  {bg_image}
  <rect width="{WIDTH}" height="{HEIGHT}" rx="22" fill="#000000" opacity="{opacity:.3}"/>
  <rect width="{WIDTH}" height="{HEIGHT}" rx="22" fill="url(#panel)"/>
  <circle cx="150" cy="150" r="96" fill="none" stroke="{ring}" stroke-width="{ring_width}"/>
  <circle cx="150" cy="150" r="92" fill="#202A3A"/>
  {avatar_image}
  <text x="285" y="72" fill="{text}" font-family="{font}" font-size="30" font-weight="800">{username}</text>
  <text x="285" y="108" fill="{primary}" font-family="{font}" font-size="16" font-weight="700">Rank {rank_text}</text>
  <text x="746" y="108" fill="{text}" font-family="{font}" font-size="16" font-weight="700" text-anchor="end">Level {level}</text>
  <text x="285" y="163" fill="#AAB8CB" font-family="{font}" font-size="15">{xp_current} / 100 XP</text>
  <rect x="285" y="181" width="560" height="18" rx="9" fill="#263346"/>
  <rect x="285" y="181" width="{width:.1}" height="18" rx="9" fill="{primary}"/>
  <text x="285" y="240" fill="#8D9DB4" font-family="{font}" font-size="13">Keep participating to reach the next level.</text>
  <text x="845" y="240" fill="{text}" font-family="{font}" font-size="13" text-anchor="end">{xp} total XP</text>
</svg>"##,
    )
}

pub fn parse_config(raw: Option<String>) -> RankCardConfig {
    raw.and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_escaped_user_data_and_configured_colours() {
        let config = RankCardConfig {
            primary_color: "#123ABC".into(),
            ..RankCardConfig::default()
        };
        let svg = render_rank_card(
            &config,
            "A & <user>",
            Some("https://cdn.test/avatar.png?a=1&b=2"),
            Some(4),
            12,
            429,
        );
        assert!(svg.contains("A &amp; &lt;user&gt;"));
        assert!(svg.contains("#123ABC"));
        assert!(svg.contains("a=1&amp;b=2"));
        assert!(svg.contains("width=\"162"));
    }

    #[test]
    fn renders_only_curated_background_presets() {
        let config = RankCardConfig {
            background_preset: Some("aurora-lake".into()),
            background_url: Some("https://untrusted.example/image.png".into()),
            ..RankCardConfig::default()
        };
        let svg = render_rank_card(&config, "member", None, None, 1, 25);
        assert!(svg.contains("data:image/png;base64,"));
        assert!(!svg.contains("untrusted.example"));
    }
}
