use base64::{Engine, engine::general_purpose::STANDARD};
use std::sync::{Arc, OnceLock};

pub const WIDTH: u32 = 800;
pub const HEIGHT: u32 = 320;

pub struct Card<'a> {
    pub author: &'a str,
    pub message: &'a str,
    pub channel: &'a str,
    pub stars: i64,
    pub avatar: Option<(&'a str, &'a [u8])>,
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn short(value: &str, limit: usize) -> String {
    let clean: String = value.chars().filter(|c| !c.is_control()).collect();
    if clean.chars().count() <= limit {
        clean
    } else {
        clean
            .chars()
            .take(limit.saturating_sub(1))
            .chain(['…'])
            .collect()
    }
}

fn glyph_weight(character: char) -> usize {
    match character {
        'i' | 'l' | 'I' | 'j' | '.' | ',' | ':' | ';' | '!' | '\'' | '|' => 35,
        'm' | 'w' | 'M' | 'W' | '@' => 100,
        ' ' => 40,
        c if c.is_ascii_uppercase() => 80,
        c if c.is_ascii() => 65,
        _ => 120,
    }
}

fn lines(value: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut length = 0;
    let mut width = 0;
    let mut truncated = false;
    for character in value.chars().filter(|c| !c.is_control() || *c == '\n') {
        if character == '\n' || length >= 30 || width + glyph_weight(character) > 1600 {
            lines.push(line);
            line = String::new();
            length = 0;
            width = 0;
            if lines.len() == 3 {
                truncated = true;
                break;
            }
        }
        if !character.is_control() {
            line.push(character);
            length += 1;
            width += glyph_weight(character);
        }
    }
    if lines.len() < 3 && !line.is_empty() {
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push("Message with an attachment".into());
    }
    if truncated && let Some(last) = lines.last_mut() {
        *last = short(&format!("{last}…"), 30);
    }
    lines
}

fn svg(card: &Card<'_>) -> String {
    let author = escape(&short(card.author, 32));
    let author_weight = short(card.author, 32)
        .chars()
        .map(glyph_weight)
        .sum::<usize>();
    let author_size = (58000 / author_weight.max(1)).min(36);
    let channel = escape(&short(card.channel, 18));
    let stars = card.stars.max(0);
    let count_label = if stars == 1 {
        "1 star".into()
    } else if stars > 9999 {
        "9999+ stars".into()
    } else {
        format!("{stars} stars")
    };
    let count_size = (13200 / count_label.chars().map(glyph_weight).sum::<usize>().max(1)).min(24);
    let message = lines(card.message)
        .iter()
        .enumerate()
        .map(|(index, line)| {
            format!(
                r##"<text x="176" y="{}" fill="#F4F7FB" font-size="34">{}</text>"##,
                116 + index * 44,
                escape(line)
            )
        })
        .collect::<String>();
    let avatar = card.avatar.filter(|(mime, bytes)| matches!(*mime, "image/png" | "image/jpeg" | "image/webp") && bytes.len() <= 256 * 1024)
        .map(|(mime, bytes)| format!(r#"<image href="data:{mime};base64,{}" x="32" y="64" width="104" height="104" clip-path="url(#avatar)" preserveAspectRatio="xMidYMid slice"/>"#, STANDARD.encode(bytes)))
        .unwrap_or_else(|| r##"<circle cx="84" cy="101" r="16" fill="#8EE5D2"/><path d="M52 154 C52 114 116 114 116 154" fill="#8EE5D2"/>"##.into());
    format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="800" height="320" viewBox="0 0 800 320">
      <defs><clipPath id="avatar"><circle cx="84" cy="116" r="52"/></clipPath><clipPath id="message"><rect x="176" y="80" width="594" height="140"/></clipPath><clipPath id="author"><rect x="176" y="24" width="594" height="48"/></clipPath></defs>
      <rect width="800" height="320" rx="20" fill="#101A29"/>
      <path d="M152 32 V288" stroke="#365269" stroke-width="2"/>
      <path d="M84 197 L92 213 L109 215 L97 228 L100 245 L84 236 L68 245 L71 228 L59 215 L76 213 Z" fill="#FFC56B"/>
      <circle cx="84" cy="116" r="55" fill="#223548" stroke="#8EE5D2" stroke-width="3"/>
      {avatar}
      <g font-family="sans-serif">
        <text x="84" y="284" text-anchor="middle" fill="#FFC56B" font-size="{count_size}" font-weight="700">{count_label}</text>
        <text x="176" y="58" fill="#8EE5D2" font-size="{author_size}" font-weight="700" clip-path="url(#author)">{author}</text>
        <g clip-path="url(#message)">{message}</g>
        <path d="M176 240 H770" stroke="#365269"/>
        <text x="176" y="284" fill="#BACBDD" font-size="20">#{channel}</text>
        <text x="770" y="284" text-anchor="end" fill="#8EE5D2" font-size="18" font-weight="700">Vozen Starboard</text>
      </g>
    </svg>"##
    )
}

pub fn render(card: &Card<'_>) -> Option<Vec<u8>> {
    static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    let fonts = FONTS.get_or_init(|| {
        let mut fonts = resvg::usvg::fontdb::Database::new();
        fonts.load_system_fonts();
        let family = if fonts
            .faces()
            .any(|face| face.families.iter().any(|(name, _)| name == "DejaVu Sans"))
        {
            "DejaVu Sans"
        } else {
            "Arial"
        };
        fonts.set_sans_serif_family(family);
        Arc::new(fonts)
    });
    fonts.faces().next()?;
    let options = resvg::usvg::Options {
        fontdb: fonts.clone(),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_str(&svg(card), &options).ok()?;
    let mut pixels = resvg::tiny_skia::Pixmap::new(WIDTH, HEIGHT)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixels.as_mut(),
    );
    pixels.encode_png().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample<'a>() -> Card<'a> {
        Card {
            author: "Rexy",
            message: "batata",
            channel: "aaaaa",
            stars: 2,
            avatar: None,
        }
    }

    #[test]
    fn larger_card_uses_readable_type_and_avatar_without_an_outer_outline() {
        let xml = svg(&sample());
        assert_eq!((WIDTH, HEIGHT), (800, 320));
        assert!(xml.contains("font-size=\"34\""));
        assert!(xml.contains("font-size=\"36\""));
        assert!(xml.contains("cx=\"84\" cy=\"116\" r=\"55\""));
        assert!(!xml.contains("fill=\"none\" stroke=\"#365269\""));
    }

    #[test]
    fn raster_contains_visible_text_and_badge() {
        let png = render(&sample()).expect("render PNG");
        assert!(png.len() < 500_000);
        let pixels = resvg::tiny_skia::Pixmap::decode_png(&png).unwrap();
        assert_eq!((pixels.width(), pixels.height()), (WIDTH, HEIGHT));
        let white_text = (90..120)
            .flat_map(|y| (175..450).map(move |x| (x, y)))
            .filter(|(x, y)| {
                let p = pixels.pixel(*x, *y).unwrap();
                p.red() > 200 && p.green() > 200 && p.blue() > 200
            })
            .count();
        assert!(white_text > 60, "SVG text must rasterize, not just parse");
        let gold = pixels.pixel(84, 220).unwrap();
        assert!(gold.red() > 230 && gold.green() > 140 && gold.blue() < 150);
        let ring = pixels.pixel(84, 61).unwrap();
        assert!(ring.green() > 150);
    }

    #[test]
    fn inputs_are_escaped_and_bounded() {
        let mut card = sample();
        card.author = "<&\"'";
        card.message = "<script>alert('x')</script>\n&hello";
        let xml = svg(&card);
        assert!(!xml.contains("<script>"));
        assert!(xml.contains("&lt;script&gt;"));
        assert!(xml.contains("&amp;hello"));
        assert_eq!(lines(&"😀".repeat(500)).len(), 3);
        assert_eq!(lines("ola\nadeus"), vec!["ola", "adeus"]);
        assert!(
            lines(&"W".repeat(100))
                .iter()
                .all(|line| line.chars().count() <= 17)
        );
        assert!(
            lines(&"x".repeat(500))
                .iter()
                .all(|line| line.chars().count() <= 30)
        );
        card.stars = -3;
        assert!(svg(&card).contains("0 stars"));
        card.stars = 1;
        assert!(svg(&card).contains(">1 star</text>"));
    }

    #[test]
    fn avatar_is_embedded_and_untrusted_mime_is_rejected() {
        let mut avatar = resvg::tiny_skia::Pixmap::new(32, 32).unwrap();
        avatar.fill(resvg::tiny_skia::Color::from_rgba8(220, 40, 80, 255));
        let avatar = avatar.encode_png().unwrap();
        let mut card = sample();
        card.avatar = Some(("image/png", &avatar));
        let pixels = resvg::tiny_skia::Pixmap::decode_png(&render(&card).unwrap()).unwrap();
        let center = pixels.pixel(84, 116).unwrap();
        assert_eq!((center.red(), center.green(), center.blue()), (220, 40, 80));
        card.avatar = Some(("image/svg+xml", &avatar));
        assert!(!svg(&card).contains("data:image/svg+xml"));
    }
}
