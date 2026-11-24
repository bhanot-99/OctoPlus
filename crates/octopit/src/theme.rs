//! Light and dark themes from the design tokens, and the motion setting (docs/design/octopit-ux.md §5).

use gtk4 as gtk;

const CSS_DARK: &str = include_str!(concat!(env!("OUT_DIR"), "/style-dark.css"));
const CSS_LIGHT: &str = include_str!(concat!(env!("OUT_DIR"), "/style-light.css"));

/// The user's choice of theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    /// Follow the desktop; dark when the desktop does not say.
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

/// Whether the desktop prefers dark (design.md §3.3: dark first when unknown).
fn system_prefers_dark() -> bool {
    let Some(settings) = gtk::Settings::default() else {
        return true;
    };
    if settings.is_gtk_application_prefer_dark_theme() {
        return true;
    }
    let name = settings.gtk_theme_name().map(|s| s.to_lowercase());
    !matches!(name, Some(t) if !t.contains("dark") && t != "adwaita")
}

/// Installs the stylesheet for `theme` on the default display. Safe to call again to switch.
/// Returns whether the dark theme is now in use.
pub fn apply(provider: &gtk::CssProvider, theme: Theme) -> bool {
    let dark = match theme {
        Theme::Dark => true,
        Theme::Light => false,
        Theme::System => system_prefers_dark(),
    };
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(dark);
    }
    provider.load_from_string(if dark { CSS_DARK } else { CSS_LIGHT });
    dark
}

/// Registers the provider once.
pub fn install(provider: &gtk::CssProvider) {
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

/// Whether the desktop asks for no animation.
pub fn animations_enabled() -> bool {
    gtk::Settings::default().is_none_or(|s| s.is_gtk_enable_animations())
}

/// Registers the bundled icons so `octopit-*-symbolic` and the app icon resolve by name.
pub fn install_icons() {
    let bytes = gtk::glib::Bytes::from_static(include_bytes!(concat!(
        env!("OUT_DIR"),
        "/octopit.gresource"
    )));
    match gtk::gio::Resource::from_data(&bytes) {
        Ok(res) => gtk::gio::resources_register(&res),
        Err(e) => tracing::warn!("icon resource did not load: {e}"),
    }
    if let Some(display) = gtk::gdk::Display::default() {
        gtk::IconTheme::for_display(&display)
            .add_resource_path("/org/octoplus/octopit/icons/icons/hicolor");
    }
    gtk::Window::set_default_icon_name("octopit");
}
