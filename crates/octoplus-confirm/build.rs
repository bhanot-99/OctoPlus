//! Generates the dialog stylesheet from `ui/tokens/tokens.json` (design.md §11):
//! colours are defined once there and never hard-coded in the dialog source.

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
    let width = tokens["dialog"]["width"]
        .as_u64()
        .ok_or("token dialog.width missing")?;
    std::fs::write(
        out.join("tokens.rs"),
        format!(
            "/// Dialog width from the design tokens.\npub const DIALOG_WIDTH: i32 = {width};\n"
        ),
    )?;
    Ok(())
}
