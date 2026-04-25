//! Theme overrides loaded from `~/.config/zos/theme.toml`.
//!
//! TOML shape:
//!
//! ```toml
//! [palette]
//! base = "#1e1e2e"
//! blue = "#89b4fa"
//!
//! [font_size]
//! base = 14
//! lg = 16
//!
//! [space]
//! x4 = 16
//! ```
//!
//! Every section is optional; missing keys fall through to the baked-in
//! Catppuccin Mocha palette + token tables in [`crate::theme`].

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Top-level shape of `theme.toml`.
///
/// Each section is a free-form `HashMap` so users can override only the
/// palette names / token keys they care about. Unknown keys are tolerated
/// (consumers will simply not look them up).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ThemeOverrides {
    /// Palette overrides keyed by name (e.g. `"base"`, `"blue"`). Values
    /// are hex strings — `"#RRGGBB"` or `"#RRGGBBAA"`.
    #[serde(default)]
    pub palette: HashMap<String, String>,
    /// Font-size overrides keyed by token (e.g. `"base"`, `"lg"`).
    #[serde(default)]
    pub font_size: HashMap<String, f32>,
    /// Spacing overrides keyed by token (e.g. `"x4"`).
    #[serde(default)]
    pub space: HashMap<String, f32>,
    /// Border-radius overrides keyed by token.
    #[serde(default)]
    pub radius: HashMap<String, f32>,
}

impl ThemeOverrides {
    /// Look up a palette override by name (e.g. `"base"`, `"blue"`).
    /// Returns the parsed RGBA color in the `[0.0, 1.0]` range, or `None`
    /// if the key isn't overridden / the hex string failed to parse.
    pub fn palette_color(&self, name: &str) -> Option<[f32; 4]> {
        let hex = self.palette.get(name)?;
        parse_hex_color(hex)
    }
}

fn parse_hex_color(hex: &str) -> Option<[f32; 4]> {
    let s = hex.trim_start_matches('#');
    let (r, g, b, a) = match s.len() {
        6 => (
            u8::from_str_radix(&s[0..2], 16).ok()?,
            u8::from_str_radix(&s[2..4], 16).ok()?,
            u8::from_str_radix(&s[4..6], 16).ok()?,
            255u8,
        ),
        8 => (
            u8::from_str_radix(&s[0..2], 16).ok()?,
            u8::from_str_radix(&s[2..4], 16).ok()?,
            u8::from_str_radix(&s[4..6], 16).ok()?,
            u8::from_str_radix(&s[6..8], 16).ok()?,
        ),
        _ => return None,
    };
    Some([
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    ])
}

/// Load `<config_dir>/theme.toml`. On missing or malformed file, returns
/// empty overrides + logs a warning via `tracing`.
pub fn load_theme_overrides() -> ThemeOverrides {
    let path = super::config_dir().join("theme.toml");
    load_theme_overrides_from(&path)
}

/// Load theme overrides from a specific path. Useful for tests and for
/// callers that want to honour a `--config` flag.
pub fn load_theme_overrides_from(path: &Path) -> ThemeOverrides {
    match std::fs::read_to_string(path) {
        Ok(content) => match toml::from_str::<ThemeOverrides>(&content) {
            Ok(t) => {
                tracing::debug!(path = %path.display(), "loaded theme overrides");
                t
            }
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = ?e,
                    "failed to parse theme.toml; using defaults"
                );
                ThemeOverrides::default()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => ThemeOverrides::default(),
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = ?e,
                "failed to read theme.toml; using defaults"
            );
            ThemeOverrides::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn parse_hex_color_rrggbb() {
        let c = parse_hex_color("#1e1e2e").expect("#1e1e2e must parse");
        assert!(approx(c[0], 0x1e as f32 / 255.0));
        assert!(approx(c[1], 0x1e as f32 / 255.0));
        assert!(approx(c[2], 0x2e as f32 / 255.0));
        assert!(approx(c[3], 1.0));

        // Without leading `#` should also work.
        let c2 = parse_hex_color("89b4fa").expect("89b4fa must parse");
        assert!(approx(c2[0], 0x89 as f32 / 255.0));
        assert!(approx(c2[1], 0xb4 as f32 / 255.0));
        assert!(approx(c2[2], 0xfa as f32 / 255.0));
        assert!(approx(c2[3], 1.0));
    }

    #[test]
    fn parse_hex_color_rrggbbaa() {
        let c = parse_hex_color("#11223380").expect("#11223380 must parse");
        assert!(approx(c[0], 0x11 as f32 / 255.0));
        assert!(approx(c[1], 0x22 as f32 / 255.0));
        assert!(approx(c[2], 0x33 as f32 / 255.0));
        assert!(approx(c[3], 0x80 as f32 / 255.0));
    }

    #[test]
    fn parse_hex_color_rejects_garbage() {
        assert!(parse_hex_color("#zzz").is_none());
        assert!(parse_hex_color("").is_none());
        assert!(parse_hex_color("#12345").is_none());
        assert!(parse_hex_color("#GGGGGG").is_none());
    }

    #[test]
    fn empty_toml_deserializes_to_default() {
        let parsed: ThemeOverrides = toml::from_str("").expect("empty TOML must parse");
        assert!(parsed.palette.is_empty());
        assert!(parsed.font_size.is_empty());
        assert!(parsed.space.is_empty());
        assert!(parsed.radius.is_empty());
    }

    #[test]
    fn doc_example_parses_and_palette_lookup_works() {
        let toml_src = r##"
[palette]
base = "#1e1e2e"
blue = "#89b4fa"

[font_size]
base = 14
lg = 16

[space]
x4 = 16
"##;
        let parsed: ThemeOverrides =
            toml::from_str(toml_src).expect("doc-comment example must parse");

        assert_eq!(parsed.palette.get("base").map(String::as_str), Some("#1e1e2e"));
        assert_eq!(parsed.palette.get("blue").map(String::as_str), Some("#89b4fa"));
        assert_eq!(parsed.font_size.get("base").copied(), Some(14.0));
        assert_eq!(parsed.font_size.get("lg").copied(), Some(16.0));
        assert_eq!(parsed.space.get("x4").copied(), Some(16.0));

        let base = parsed.palette_color("base").expect("base must resolve");
        assert!(approx(base[0], 0x1e as f32 / 255.0));
        assert!(approx(base[3], 1.0));

        // Missing palette key should return None, not panic.
        assert!(parsed.palette_color("does-not-exist").is_none());
    }

    #[test]
    fn missing_file_returns_default() {
        let overrides =
            load_theme_overrides_from(Path::new("/this/path/should/not/exist/theme.toml"));
        assert!(overrides.palette.is_empty());
        assert!(overrides.font_size.is_empty());
    }
}
