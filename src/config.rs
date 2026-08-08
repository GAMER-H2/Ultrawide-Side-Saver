//! User configuration, loaded from `$XDG_CONFIG_HOME/ultrawide-side-saver/config.toml`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Connector name of the ultrawide display, e.g. "DP-1". Empty = auto-pick the
    /// widest connected output.
    pub output: String,

    /// Aspect ratio of the content that sits in the middle, as "W:H".
    pub content_aspect: String,

    /// Frames per second for the animation. The whole point is slow movement, so
    /// low values are both prettier and cheaper.
    pub fps: u32,

    /// Overall output level, 0.0..=1.0. Keep this low: the bars exist to give the
    /// edge pixels *some* work, not to light them up as hard as the centre.
    pub brightness: f32,

    /// Animation speed multiplier. 1.0 = one full cycle every 10 minutes.
    pub speed: f32,

    /// Named colour palette. See `palette::PALETTES`.
    pub palette: String,

    /// Width of the soft fade on the edge facing the content, as a fraction of the
    /// bar width. Softens the transition so the bars don't frame the picture.
    pub inner_fade: f32,

    /// Show the bars immediately on startup instead of waiting for a toggle.
    pub start_visible: bool,

    /// Draw over Plasma panels that reach into the bar area, rather than being
    /// pushed out of their way. True suits the main use case (fullscreen 16:9
    /// content, where the panel is hidden anyway); false keeps a panel's outer
    /// ends visible at the cost of the bars stopping short of the screen edge.
    pub cover_panels: bool,

    /// Override the bar width in pixels. 0 = derive it from `content_aspect`.
    pub bar_width: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            output: String::new(),
            content_aspect: "16:9".into(),
            fps: 30,
            brightness: 0.35,
            speed: 1.0,
            palette: "aurora".into(),
            inner_fade: 0.18,
            start_visible: false,
            cover_panels: true,
            bar_width: 0,
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
            });
        base.join("ultrawide-side-saver").join("config.toml")
    }

    /// Load the config, falling back to defaults if the file does not exist.
    /// A malformed file is a hard error: silently ignoring it would be baffling.
    pub fn load() -> Result<Self> {
        let path = Self::path();
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e).context(format!("reading {}", path.display())),
        };
        let cfg: Self =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.fps >= 1 && self.fps <= 240, "fps must be 1..=240");
        anyhow::ensure!(
            (0.0..=1.0).contains(&self.brightness),
            "brightness must be 0.0..=1.0"
        );
        anyhow::ensure!(
            (0.0..=1.0).contains(&self.inner_fade),
            "inner_fade must be 0.0..=1.0"
        );
        anyhow::ensure!(self.speed > 0.0, "speed must be > 0");
        self.aspect()?;
        crate::palette::lookup(&self.palette)
            .with_context(|| format!("unknown palette {:?}", self.palette))?;
        Ok(())
    }

    /// Content aspect as a bare ratio (width / height).
    pub fn aspect(&self) -> Result<f32> {
        let (w, h) = self
            .content_aspect
            .split_once([':', '/'])
            .context("content_aspect must look like \"16:9\"")?;
        let w: f32 = w.trim().parse().context("bad content_aspect width")?;
        let h: f32 = h.trim().parse().context("bad content_aspect height")?;
        anyhow::ensure!(w > 0.0 && h > 0.0, "content_aspect components must be > 0");
        Ok(w / h)
    }

    /// Width of one side bar for an output of the given logical size.
    /// Returns 0 when the content already fills the output.
    pub fn bar_width_for(&self, output_w: u32, output_h: u32) -> Result<u32> {
        if self.bar_width > 0 {
            return Ok(self.bar_width.min(output_w / 2));
        }
        let content_w = (output_h as f32 * self.aspect()?).round() as i64;
        let slack = output_w as i64 - content_w;
        Ok(if slack <= 0 { 0 } else { (slack / 2) as u32 })
    }

    /// Write a fully-commented default config, without clobbering an existing one.
    pub fn write_default_if_missing() -> Result<PathBuf> {
        let path = Self::path();
        if path.exists() {
            return Ok(path);
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, DEFAULT_CONFIG_TOML)?;
        Ok(path)
    }
}

pub const DEFAULT_CONFIG_TOML: &str = r#"# ultrawide-side-saver configuration

# Connector name of the ultrawide display (see `kscreen-doctor -o`).
# Leave empty to auto-pick the widest connected output.
output = ""

# Aspect ratio of the content in the middle. The bars fill whatever is left over.
content_aspect = "16:9"

# Animation frame rate. Low is the point: the movement should be barely perceptible.
fps = 30

# Overall brightness, 0.0..=1.0. Deliberately dim - these pixels are being exercised,
# not shown off. Raising this defeats the purpose of the whole exercise.
brightness = 0.35

# Animation speed multiplier. 1.0 = one full loop every 10 minutes.
speed = 1.0

# One of: aurora, ember, ocean, mono, forest
palette = "aurora"

# Soft fade on the edge facing the content, as a fraction of bar width.
inner_fade = 0.18

# Show the bars as soon as the daemon starts.
start_visible = false

# Draw over Plasma panels whose ends reach into the bar area. True suits the main
# use case (fullscreen 16:9 content, where the panel is hidden anyway). Set false
# if you toggle the bars on the desktop and want to keep your tray and clock
# visible - the bars then stop short of the panel instead of covering it.
cover_panels = true

# Force a bar width in pixels. 0 = derive from content_aspect.
bar_width = 0
"#;
