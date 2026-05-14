use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;

use crate::render::{Color, Theme};

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    pub font: Option<String>,
    pub bg: Option<String>,
    pub fg: Option<String>,
    pub selbg: Option<String>,
    pub selfg: Option<String>,
    pub matchbg: Option<String>,
    pub matchfg: Option<String>,
    pub border: Option<String>,
    pub border_width: Option<i32>,
    pub width: Option<u16>,
    pub height: Option<u16>,
    pub padding_x: Option<i32>,
    pub padding_y: Option<i32>,
    pub row_padding_y: Option<i32>,
    pub opacity: Option<f64>,
}

const DEF_FONT: &str = "Sans 12";
const DEF_BG: &str = "#222222";
const DEF_FG: &str = "#dddddd";
const DEF_SELBG: &str = "#005577";
const DEF_SELFG: &str = "#ffffff";
const DEF_MATCHBG: &str = "#3b3b00";
const DEF_MATCHFG: &str = "#ffff66";
const DEF_BORDER: &str = "#888888";
const DEF_BORDER_WIDTH: i32 = 2;
const DEF_WIDTH: u16 = 700;
const DEF_HEIGHT: u16 = 420;
const DEF_PAD_X: i32 = 10;
const DEF_PAD_Y: i32 = 6;
const DEF_ROW_PAD_Y: i32 = 6;
const DEF_OPACITY: f64 = 1.0;

impl Config {
    /// Load config from `$XDG_CONFIG_HOME/gmenu/config.toml` (or `~/.config/...`).
    /// Returns defaults if the file does not exist. Errors only on parse failure.
    pub fn load() -> Result<Self> {
        let path = match config_path() {
            Some(p) => p,
            None => return Ok(Self::default()),
        };
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn into_theme(self, prompt: String) -> Result<Theme> {
        let parse = |s: &str, name: &str| -> Result<Color> {
            Color::from_hex(s)
                .with_context(|| format!("invalid {name} color: {s:?} (want #RRGGBB)"))
        };
        Ok(Theme {
            bg: parse(self.bg.as_deref().unwrap_or(DEF_BG), "bg")?,
            fg: parse(self.fg.as_deref().unwrap_or(DEF_FG), "fg")?,
            selbg: parse(self.selbg.as_deref().unwrap_or(DEF_SELBG), "selbg")?,
            selfg: parse(self.selfg.as_deref().unwrap_or(DEF_SELFG), "selfg")?,
            matchbg: parse(self.matchbg.as_deref().unwrap_or(DEF_MATCHBG), "matchbg")?,
            matchfg: parse(self.matchfg.as_deref().unwrap_or(DEF_MATCHFG), "matchfg")?,
            border: parse(self.border.as_deref().unwrap_or(DEF_BORDER), "border")?,
            border_width: self.border_width.unwrap_or(DEF_BORDER_WIDTH).max(0),
            font: self.font.unwrap_or_else(|| DEF_FONT.to_string()),
            prompt,
            padding_x: self.padding_x.unwrap_or(DEF_PAD_X),
            padding_y: self.padding_y.unwrap_or(DEF_PAD_Y),
            row_padding_y: self.row_padding_y.unwrap_or(DEF_ROW_PAD_Y).max(0),
        })
    }

    pub fn width(&self) -> u16 { self.width.unwrap_or(DEF_WIDTH) }
    pub fn height(&self) -> u16 { self.height.unwrap_or(DEF_HEIGHT) }
    pub fn opacity(&self) -> f64 { self.opacity.unwrap_or(DEF_OPACITY).clamp(0.0, 1.0) }
}

pub fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("gmenu").join("config.toml"))
}
