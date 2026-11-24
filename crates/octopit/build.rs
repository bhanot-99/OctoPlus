//! Generates the OctoPit stylesheet from `ui/tokens/tokens.json` (design.md §11, docs/design/octopit-ux.md §5):
//! colours are defined once there and never hard-coded in the window source.

use std::fmt::Write as _;
use std::path::PathBuf;

/// Semantic class → token used by the stylesheet. Order is stable so the output is reproducible.
const COLOR_VARS: [&str; 18] = [
    "bg.canvas",
    "bg.surface",
    "bg.raised",
    "border.subtle",
    "border.strong",
    "text.primary",
    "text.secondary",
    "text.muted",
    "accent.primary",
    "accent.primary.fg",
    "tier.safe",
    "tier.review",
    "tier.dangerous",
    "tier.dangerous.solid",
    "tier.dangerous.solid.fg",
    "taint.external",
    "egress",
    "status.degraded",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let tokens_path = manifest.join("../../ui/tokens/tokens.json");
    println!("cargo:rerun-if-changed={}", tokens_path.display());
    println!("cargo:rerun-if-changed=src/style.css.in");
    let tokens: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&tokens_path)?)?;
    let template = std::fs::read_to_string(manifest.join("src/style.css.in"))?;

    let out = PathBuf::from(std::env::var("OUT_DIR")?);
    for theme in ["dark", "light"] {
        let mut defs = String::new();
        for name in COLOR_VARS {
            let value = tokens["color"][theme][name]
                .as_str()
                .ok_or_else(|| format!("token color.{theme}.{name} missing"))?;
            writeln!(defs, "@define-color {} {value};", name.replace('.', "_"))?;
        }
        // OctoPit's own tokens (v2 visual language): oct_<name>.
        let oct = tokens["octopit"][theme]
            .as_object()
            .ok_or_else(|| format!("tokens octopit.{theme} missing"))?;
        for (name, value) in oct {
            let v = value
                .as_str()
                .ok_or_else(|| format!("token octopit.{theme}.{name} is not a string"))?;
            writeln!(defs, "@define-color oct_{} {v};", name.replace('.', "_"))?;
        }
        let radius = &tokens["radius"];
        let css = template
            .replace("/*@COLORS@*/", &defs)
            .replace("@RADIUS_SM@", &radius["sm"].to_string())
            .replace("@RADIUS_MD@", &radius["md"].to_string())
            .replace("@RADIUS_LG@", &radius["lg"].to_string())
            .replace("@FOCUS_W@", &tokens["focus"]["width"].to_string())
            .replace("@FOCUS_OFF@", &tokens["focus"]["offset"].to_string());
        std::fs::write(out.join(format!("style-{theme}.css")), css)?;
    }

    // The chart palette as constants, so drawn charts and the stylesheet share one source.
    let series = tokens["octopit"]["series"]
        .as_array()
        .ok_or("tokens octopit.series missing")?;
    let mut rs = String::from(
        "/// Chart series colours from `ui/tokens/tokens.json`.\npub const SERIES: [&str; 8] = [\n",
    );
    for c in series.iter().take(8) {
        let c = c.as_str().ok_or("series colour is not a string")?;
        writeln!(rs, "    \"{c}\",")?;
    }
    rs.push_str("];\n");
    std::fs::write(out.join("tokens.rs"), rs)?;

    // The bundled icons (docs/design/octopit-ux.md §5): one resource file, loaded at start.
    let assets = manifest.join("assets");
    println!("cargo:rerun-if-changed={}", assets.display());
    let target = out.join("octopit.gresource");
    let status = std::process::Command::new("glib-compile-resources")
        .arg(format!("--sourcedir={}", assets.display()))
        .arg(format!("--target={}", target.display()))
        .arg(assets.join("octopit.gresource.xml"))
        .status()
        .map_err(|e| format!("glib-compile-resources not found (libglib2.0-dev-bin): {e}"))?;
    if !status.success() {
        return Err("glib-compile-resources failed".into());
    }
    Ok(())
}
