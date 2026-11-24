//! The window: header, sidebar, pages, voice dock and status strip (docs/design/octopit-ux.md §2).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;
use tokio::sync::mpsc::UnboundedReceiver;

use octopit_core::controller::{Command, Controller, Update};
use octopit_core::format;
use octopit_core::gpu::GpuState;
use octopit_core::settings::{self, Settings, ThemeChoice};
use octopit_core::state::{AppState, Changed};

use crate::mic::MicFab;
use crate::pages::activity::ActivityPage;
use crate::pages::backends::BackendsPage;
use crate::pages::chat::ChatPage;
use crate::pages::overview::OverviewPage;
use crate::pages::privacy::PrivacyPage;
use crate::pages::settings::SettingsPage;
use crate::pages::{Ctx, Page, PageId};
use crate::theme::{self, Theme};
use crate::widgets::{dot, icon, label, set_dot};

/// Overview shows the big voice card, so the floating mic hides there.
const OVERVIEW_HAS_VOICE_CARD: bool = true;

/// Everything the window owns.
pub struct Window {
    /// The top-level window.
    pub window: gtk::ApplicationWindow,
    stack: gtk::Stack,
    nav: Vec<(PageId, gtk::Button)>,
    nav_labels: Vec<gtk::Label>,
    rail_btns: Vec<(PageId, gtk::Button)>,
    search_label: gtk::Label,
    brand_label: gtk::Label,
    narrow: Cell<bool>,
    compact: Cell<bool>,
    pages: Vec<(PageId, Box<dyn Page>)>,
    pill_dot: gtk::Box,
    pill_text: gtk::Label,
    pill: gtk::Box,
    mic: MicFab,
    fact_egress: gtk::Button,
    fact_memory: gtk::Button,
    fact_gpu: gtk::Button,
    fact_speed: gtk::Button,
    toast: gtk::Label,
    toast_box: gtk::Box,
    toast_gen: Cell<u64>,
    state: Rc<RefCell<AppState>>,
    current: Cell<PageId>,
    provider: gtk::CssProvider,
    controller: Controller,
    scale_provider: gtk::CssProvider,
    settings: RefCell<Settings>,
    settings_path: Option<std::path::PathBuf>,
    palette: RefCell<Option<crate::palette::Palette>>,
}

fn fact(text: &str, tip: &str) -> gtk::Button {
    let b = gtk::Button::with_label(text);
    b.add_css_class("status-fact");
    b.add_css_class("num");
    b.set_tooltip_text(Some(tip));
    b
}

fn set_fact(b: &gtk::Button, text: &str) {
    b.set_label(text);
}

impl Window {
    /// Builds the window and starts applying updates from `rx`.
    pub fn build(
        app: &gtk::Application,
        controller: &Controller,
        rx: UnboundedReceiver<Update>,
    ) -> Rc<Self> {
        theme::install_icons();
        let provider = gtk::CssProvider::new();
        theme::install(&provider);
        theme::apply(&provider, Theme::System);

        let window = gtk::ApplicationWindow::new(app);
        window.set_title(Some("OctoPit"));
        // Never bigger than the screen: 1100x720 on a roomy display, otherwise most of it.
        let (mut width, mut height) = (1100, 720);
        if let Some(display) = gtk::gdk::Display::default()
            && let Some(monitor) = display
                .monitors()
                .item(0)
                .and_then(|m| m.downcast::<gtk::gdk::Monitor>().ok())
        {
            let g = monitor.geometry();
            width = width.min(g.width() * 9 / 10);
            height = height.min(g.height() * 9 / 10);
        }
        // OCTOPIT_SIZE=WxH forces a size (the screenshot review of narrow windows).
        if let Some((w, h)) = std::env::var("OCTOPIT_SIZE")
            .ok()
            .and_then(|v| {
                v.split_once('x')
                    .map(|(a, b)| (a.parse().ok(), b.parse().ok()))
            })
            .and_then(|(a, b)| a.zip(b))
        {
            width = w;
            height = h;
        }
        window.set_default_size(width, height);
        window.set_size_request(480, 400);
        window.add_css_class("octopit");
        if !theme::animations_enabled() {
            window.add_css_class("reduce-motion");
        }

        let state = Rc::new(RefCell::new(AppState::new()));
        let stack = gtk::Stack::new();
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        stack.set_transition_duration(120);
        stack.set_hexpand(true);
        stack.set_vexpand(true);

        // The navigation closure needs the window; it is filled in after construction.
        let nav_cell: Rc<RefCell<Option<Rc<Window>>>> = Rc::new(RefCell::new(None));
        let navigate: Rc<dyn Fn(PageId)> = {
            let cell = Rc::clone(&nav_cell);
            Rc::new(move |p| {
                let win = cell.borrow().clone();
                if let Some(w) = win {
                    w.show_page(p);
                }
            })
        };
        let toast_cell: Rc<RefCell<Option<Rc<Window>>>> = Rc::clone(&nav_cell);
        let toast: Rc<dyn Fn(&str)> = Rc::new(move |t| {
            let win = toast_cell.borrow().clone();
            if let Some(w) = win {
                w.show_toast(t);
            }
        });
        let ask_cell: Rc<RefCell<Option<Rc<Window>>>> = Rc::clone(&nav_cell);
        let ask: Rc<dyn Fn(&str)> = Rc::new(move |q| {
            let win = ask_cell.borrow().clone();
            if let Some(w) = win {
                w.show_page(PageId::Chat);
                w.ask(q);
            }
        });
        let ctx = Ctx {
            controller: controller.clone(),
            navigate,
            toast,
            ask,
            state: Rc::clone(&state),
        };

        let settings_path = settings::default_path(
            std::env::var_os("XDG_CONFIG_HOME").as_deref(),
            std::env::var_os("HOME").as_deref(),
        );
        let loaded = settings_path
            .as_deref()
            .map(Settings::load)
            .unwrap_or_default();
        let settings_page = SettingsPage::new(&ctx);
        settings_page.set(&loaded);
        {
            let cell = Rc::clone(&nav_cell);
            settings_page.on_change(Box::new(move |s| {
                let win = cell.borrow().clone();
                if let Some(w) = win {
                    w.change_settings(s);
                }
            }));
        }

        let pages: Vec<(PageId, Box<dyn Page>)> = vec![
            (PageId::Overview, Box::new(OverviewPage::new(&ctx))),
            (PageId::Chat, Box::new(ChatPage::new(&ctx))),
            (PageId::Backends, Box::new(BackendsPage::new(&ctx))),
            (PageId::Activity, Box::new(ActivityPage::new(&ctx))),
            (PageId::Privacy, Box::new(PrivacyPage::new(&ctx))),
            (PageId::Settings, Box::new(settings_page)),
        ];
        for (id, p) in &pages {
            stack.add_named(&p.widget(), Some(id.key()));
        }

        // Header: brand, pill tabs, search, status, settings (docs/design/octopit-ux.md §10).
        let header = gtk::HeaderBar::new();
        header.add_css_class("octopit-header");
        header.set_show_title_buttons(true);
        let brand = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        brand.append(&icon("octopit", 28));
        let brand_label = label("OctoPit", &["brand"]);
        brand.append(&brand_label);
        brand.set_margin_end(12);

        let tabs = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let mut nav = Vec::new();
        let mut nav_labels = Vec::new();
        for id in PageId::SIDEBAR {
            let b = gtk::Button::new();
            b.add_css_class("tab");
            b.set_tooltip_text(Some(id.title()));
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            row.append(&icon(id.icon(), 16));
            let text = label(id.title(), &[]);
            nav_labels.push(text.clone());
            row.append(&text);
            b.set_child(Some(&row));
            b.update_property(&[gtk::accessible::Property::Label(id.title())]);
            tabs.append(&b);
            nav.push((id, b));
        }

        let avatar = gtk::Button::new();
        avatar.add_css_class("avatar");
        avatar.set_child(Some(&icon("octopit-symbolic", 18)));
        avatar.set_tooltip_text(Some("Settings"));
        avatar.update_property(&[gtk::accessible::Property::Label("Settings")]);
        let pill = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        pill.add_css_class("pill");
        pill.add_css_class("loading");
        let pill_dot = dot(octopit_core::health::PartState::Starting);
        let pill_text = label("Connecting\u{2026}", &[]);
        pill.append(&pill_dot);
        pill.append(&pill_text);
        let search = gtk::Button::new();
        search.add_css_class("search-pill");
        let search_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        search_row.append(&icon("octopit-search-symbolic", 14));
        let search_label = label("Search", &[]);
        search_row.append(&search_label);
        search.set_child(Some(&search_row));
        search.set_tooltip_text(Some("Search commands (Ctrl+K)"));
        search.update_property(&[gtk::accessible::Property::Label("Search commands")]);
        // One full-width row so nothing overlaps: brand | tabs | search, status, settings.
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        row.set_hexpand(true);
        row.append(&brand);
        let grow_l = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        grow_l.set_hexpand(true);
        row.append(&grow_l);
        row.append(&tabs);
        let grow_r = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        grow_r.set_hexpand(true);
        row.append(&grow_r);
        row.append(&search);
        row.append(&pill);
        row.append(&avatar);
        header.set_title_widget(Some(&row));
        window.set_titlebar(Some(&header));

        // Left rail: a vertical pill of round icon buttons.
        let rail = gtk::Box::new(gtk::Orientation::Vertical, 4);
        rail.add_css_class("rail");
        rail.set_valign(gtk::Align::Start);
        rail.set_margin_start(14);
        rail.set_margin_top(6);
        let mut rail_btns = Vec::new();
        let rail_button = |icon_name: &str, tip: &str| {
            let b = gtk::Button::new();
            b.add_css_class("rail-btn");
            b.set_child(Some(&icon(icon_name, 18)));
            b.set_tooltip_text(Some(tip));
            b.update_property(&[gtk::accessible::Property::Label(tip)]);
            b
        };
        let rail_overview = rail_button(PageId::Overview.icon(), "Overview");
        let rail_search = rail_button("octopit-search-symbolic", "Search commands");
        let rail_privacy = rail_button(PageId::Privacy.icon(), "Privacy");
        let rail_settings = rail_button(PageId::Settings.icon(), "Settings");
        let rail_theme = rail_button("octopit-theme-symbolic", "Switch light and dark");
        for b in [
            &rail_overview,
            &rail_search,
            &rail_privacy,
            &rail_settings,
            &rail_theme,
        ] {
            rail.append(b);
        }
        rail_btns.push((PageId::Overview, rail_overview.clone()));
        rail_btns.push((PageId::Privacy, rail_privacy.clone()));
        rail_btns.push((PageId::Settings, rail_settings.clone()));

        // Floating mic and status strip.
        let mic = MicFab::new(controller);
        let fact_egress = fact("Sent to network: \u{2014}", "Open the Privacy page");
        let fact_memory = fact("Memory: \u{2014}", "Open the Overview page");
        let fact_gpu = fact("GPU: \u{2014}", "Open the Backends page");
        let fact_speed = fact("Model speed: \u{2014}", "Open the Activity page");
        let strip = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        strip.add_css_class("status-strip");
        for f in [&fact_egress, &fact_memory, &fact_gpu, &fact_speed] {
            strip.append(f);
        }
        let grow = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        grow.set_hexpand(true);
        strip.append(&grow);
        strip.append(&label("OctoPit", &["caption", "muted"]));

        // Toast and floating mic overlay the pages.
        let toast_label = label("", &[]);
        toast_label.set_wrap(true);
        let toast_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        toast_box.add_css_class("toast");
        toast_box.set_halign(gtk::Align::Center);
        toast_box.set_valign(gtk::Align::End);
        toast_box.set_margin_bottom(24);
        toast_box.append(&toast_label);
        toast_box.set_visible(false);
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&stack));
        overlay.add_overlay(&toast_box);
        overlay.add_overlay(&mic.widget());

        let middle = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        middle.append(&rail);
        middle.append(&overlay);
        let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
        middle.set_vexpand(true);
        outer.append(&middle);
        outer.append(&strip);
        window.set_child(Some(&outer));

        let win = Rc::new(Self {
            window,
            stack,
            nav,
            nav_labels,
            rail_btns,
            search_label,
            brand_label,
            narrow: Cell::new(false),
            compact: Cell::new(false),
            pages,
            pill_dot,
            pill_text,
            pill,
            mic,
            fact_egress,
            fact_memory,
            fact_gpu,
            fact_speed,
            toast: toast_label,
            toast_box,
            toast_gen: Cell::new(0),
            state,
            current: Cell::new(PageId::Overview),
            provider,
            controller: controller.clone(),
            scale_provider: gtk::CssProvider::new(),
            settings: RefCell::new(loaded.clone()),
            settings_path,
            palette: RefCell::new(None),
        });
        *nav_cell.borrow_mut() = Some(Rc::clone(&win));

        for (id, b) in &win.nav {
            let w = Rc::clone(&win);
            let id = *id;
            b.connect_clicked(move |_| w.show_page(id));
        }
        {
            let w = Rc::clone(&win);
            avatar.connect_clicked(move |_| w.show_page(PageId::Settings));
            let w = Rc::clone(&win);
            search.connect_clicked(move |_| w.show_palette());
            let w = Rc::clone(&win);
            rail_search.connect_clicked(move |_| w.show_palette());
            let w = Rc::clone(&win);
            rail_theme.connect_clicked(move |_| w.toggle_theme());
        }
        for (id, b) in &win.rail_btns {
            let w = Rc::clone(&win);
            let id = *id;
            b.connect_clicked(move |_| w.show_page(id));
        }
        for (f, p) in [
            (&win.fact_egress, PageId::Privacy),
            (&win.fact_memory, PageId::Overview),
            (&win.fact_gpu, PageId::Backends),
            (&win.fact_speed, PageId::Activity),
        ] {
            let w = Rc::clone(&win);
            f.connect_clicked(move |_| w.show_page(p));
        }
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &win.scale_provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1,
            );
        }
        win.apply_settings(&loaded);
        win.build_palette();
        if !loaded.welcome_done && !std::env::args().any(|a| a == "--screenshots") {
            let w = Rc::clone(&win);
            glib::idle_add_local_once(move || {
                let w2 = Rc::clone(&w);
                crate::welcome::show(
                    &w.window,
                    Box::new(move || {
                        let mut s = w2.settings.borrow().clone();
                        s.welcome_done = true;
                        w2.save_settings(&s);
                    }),
                );
            });
        }
        win.install_actions(app, controller);
        {
            // Narrow windows: icon-only sidebar and a slimmer voice dock (docs/design/octopit-ux.md §2).
            let w = Rc::downgrade(&win);
            win.window.add_tick_callback(move |window, _| {
                if let Some(w) = w.upgrade() {
                    w.set_compact(window.width() < 1200);
                    w.set_narrow(window.width() < 820);
                }
                glib::ControlFlow::Continue
            });
        }
        GtkWindowExt::set_focus(&win.window, None::<&gtk::Widget>);
        win.show_page(PageId::Overview);
        win.redraw_all();

        // Apply updates as they arrive.
        let w = Rc::clone(&win);
        let mut rx = rx;
        glib::spawn_future_local(async move {
            while let Some(update) = rx.recv().await {
                w.apply(update);
            }
        });
        win
    }

    fn install_actions(self: &Rc<Self>, app: &gtk::Application, controller: &Controller) {
        let add = |name: &str, accel: &str, f: Box<dyn Fn()>| {
            let a = gio::SimpleAction::new(name, None);
            a.connect_activate(move |_, _| f());
            self.window.add_action(&a);
            app.set_accels_for_action(&format!("win.{name}"), &[accel]);
        };
        for (i, id) in PageId::SIDEBAR.iter().enumerate() {
            let w = Rc::clone(self);
            let id = *id;
            add(
                &format!("page-{}", id.key()),
                &format!("<Ctrl>{}", i + 1),
                Box::new(move || w.show_page(id)),
            );
        }
        let w = Rc::clone(self);
        add(
            "page-settings",
            "<Ctrl>comma",
            Box::new(move || w.show_page(PageId::Settings)),
        );
        let w = Rc::clone(self);
        add("palette", "<Ctrl>k", Box::new(move || w.show_palette()));
        let w = Rc::clone(self);
        add("mic", "<Ctrl>space", Box::new(move || w.mic.press()));
        let ctl = controller.clone();
        add(
            "stop",
            "Escape",
            Box::new(move || {
                let _ = ctl.send(Command::StopAsk);
            }),
        );
        let w = Rc::clone(self);
        add(
            "theme-dark",
            "<Ctrl><Shift>d",
            Box::new(move || w.set_theme(Theme::Dark)),
        );
        let w = Rc::clone(self);
        add(
            "theme-light",
            "<Ctrl><Shift>l",
            Box::new(move || w.set_theme(Theme::Light)),
        );
    }

    /// Very narrow windows: the brand name and the status words go, the dots and icons stay.
    fn set_narrow(&self, narrow: bool) {
        if self.narrow.get() == narrow {
            return;
        }
        self.narrow.set(narrow);
        self.brand_label.set_visible(!narrow);
        self.pill_text.set_visible(!narrow);
    }

    fn set_compact(&self, compact: bool) {
        if self.compact.get() == compact {
            return;
        }
        self.compact.set(compact);
        if compact {
            self.window.add_css_class("compact");
        } else {
            self.window.remove_css_class("compact");
        }
        for l in &self.nav_labels {
            l.set_visible(!compact);
        }
        self.search_label.set_visible(!compact);
        self.fact_speed.set_visible(!compact);
        self.fact_gpu.set_visible(!compact);
    }

    /// Asks a question as if it had been typed (the screenshot review uses it).
    pub fn ask(&self, text: &str) {
        let q = self.state.borrow_mut().conversation.begin(text);
        if let Some(q) = q {
            self.redraw(Changed {
                chat: true,
                ..Changed::default()
            });
            let _ = self.controller.send(Command::Ask(q));
        }
    }

    /// Scrolls the visible page down by most of a screen; returns false at the end (or when it
    /// does not scroll at all). The screenshot review pages through tall pages with it.
    pub fn scroll_page_down(&self) -> bool {
        let Some(child) = self.stack.visible_child() else {
            return false;
        };
        let Some(sw) = child.downcast_ref::<gtk::ScrolledWindow>() else {
            return false;
        };
        let adj = sw.vadjustment();
        let end = adj.upper() - adj.page_size();
        if adj.value() + 1.0 >= end {
            return false;
        }
        adj.set_value((adj.value() + adj.page_size() * 0.85).min(end));
        true
    }

    fn build_palette(self: &Rc<Self>) {
        use crate::palette::{Command as PaletteCommand, Palette};
        let mut commands: Vec<PaletteCommand> = Vec::new();
        for (i, id) in PageId::SIDEBAR.iter().enumerate() {
            let w = Rc::clone(self);
            let id = *id;
            commands.push(PaletteCommand {
                title: format!("Go to {}", id.title()),
                hint: format!("Ctrl+{}", i + 1),
                run: Rc::new(move || w.show_page(id)),
            });
        }
        let w = Rc::clone(self);
        commands.push(PaletteCommand {
            title: "Open Settings".to_owned(),
            hint: "Ctrl+,".to_owned(),
            run: Rc::new(move || w.show_page(PageId::Settings)),
        });
        let w = Rc::clone(self);
        commands.push(PaletteCommand {
            title: "Start or stop listening".to_owned(),
            hint: "Ctrl+Space".to_owned(),
            run: Rc::new(move || w.mic.press()),
        });
        let w = Rc::clone(self);
        commands.push(PaletteCommand {
            title: "Ask a question".to_owned(),
            hint: "Chat".to_owned(),
            run: Rc::new(move || w.show_page(PageId::Chat)),
        });
        let ctl = self.controller.clone();
        commands.push(PaletteCommand {
            title: "Stop the answer".to_owned(),
            hint: "Esc".to_owned(),
            run: Rc::new(move || {
                let _ = ctl.send(Command::StopAsk);
            }),
        });
        for (title, choice) in [
            ("Theme: follow the desktop", ThemeChoice::System),
            ("Theme: dark", ThemeChoice::Dark),
            ("Theme: light", ThemeChoice::Light),
        ] {
            let w = Rc::clone(self);
            commands.push(PaletteCommand {
                title: title.to_owned(),
                hint: "Appearance".to_owned(),
                run: Rc::new(move || {
                    let mut s = w.settings.borrow().clone();
                    s.theme = choice;
                    w.apply_settings(&s);
                    w.save_settings(&s);
                }),
            });
        }
        *self.palette.borrow_mut() = Some(Palette::new(&self.window, commands));
    }

    /// Opens the command palette.
    pub fn show_palette(&self) {
        if let Some(p) = self.palette.borrow().as_ref() {
            p.show();
        }
    }

    /// Applies settings to the window (theme, text size, motion).
    fn apply_settings(&self, s: &Settings) {
        let theme = match s.theme {
            ThemeChoice::System => Theme::System,
            ThemeChoice::Light => Theme::Light,
            ThemeChoice::Dark => Theme::Dark,
        };
        let dark = theme::apply(&self.provider, theme);
        if dark {
            self.window.add_css_class("dark");
        } else {
            self.window.remove_css_class("dark");
        }
        self.scale_provider.load_from_string(&format!(
            "window.octopit {{ font-size: {}%; }}",
            s.text_scale
        ));
        if s.reduce_motion || !theme::animations_enabled() {
            self.window.add_css_class("reduce-motion");
        } else {
            self.window.remove_css_class("reduce-motion");
        }
    }

    /// The settings page changed something: apply it and keep it for the next start.
    fn change_settings(&self, s: &Settings) {
        let mut new = s.clone();
        new.welcome_done = self.settings.borrow().welcome_done;
        self.apply_settings(&new);
        self.save_settings(&new);
    }

    fn save_settings(&self, s: &Settings) {
        *self.settings.borrow_mut() = s.clone();
        if let Some(path) = &self.settings_path
            && let Err(e) = s.save(path)
        {
            tracing::warn!("settings were not saved: {e}");
        }
    }

    /// Flips between light and dark and keeps the choice.
    fn toggle_theme(&self) {
        let mut s = self.settings.borrow().clone();
        let dark_now = self.window.has_css_class("dark");
        s.theme = if dark_now {
            ThemeChoice::Light
        } else {
            ThemeChoice::Dark
        };
        self.apply_settings(&s);
        self.save_settings(&s);
    }

    /// Switches the theme.
    pub fn set_theme(&self, theme: Theme) {
        if theme::apply(&self.provider, theme) {
            self.window.add_css_class("dark");
        } else {
            self.window.remove_css_class("dark");
        }
    }

    /// Shows a page and moves the sidebar highlight.
    pub fn show_page(&self, id: PageId) {
        self.current.set(id);
        self.stack.set_visible_child_name(id.key());
        for (nid, b) in &self.nav {
            if *nid == id {
                b.add_css_class("active");
            } else {
                b.remove_css_class("active");
            }
        }
        for (rid, b) in &self.rail_btns {
            if *rid == id {
                b.add_css_class("active");
            } else {
                b.remove_css_class("active");
            }
        }
        self.mic
            .set_visible(!(id == PageId::Overview && OVERVIEW_HAS_VOICE_CARD));
        if let Some((_, p)) = self.pages.iter().find(|(pid, _)| *pid == id) {
            p.shown();
        }
    }

    /// Shows a short message for a few seconds.
    pub fn show_toast(self: &Rc<Self>, text: &str) {
        self.toast.set_text(text);
        self.toast_box.set_visible(true);
        let gen_id = self.toast_gen.get() + 1;
        self.toast_gen.set(gen_id);
        self.toast
            .announce(text, gtk::AccessibleAnnouncementPriority::Medium);
        let w = Rc::clone(self);
        glib::timeout_add_local_once(std::time::Duration::from_secs(5), move || {
            if w.toast_gen.get() == gen_id {
                w.toast_box.set_visible(false);
            }
        });
    }

    fn apply(self: &Rc<Self>, update: Update) {
        let changed = self.state.borrow_mut().apply(update);
        self.redraw(changed);
        if changed.notice {
            let n = self.state.borrow_mut().notice.take();
            if let Some(n) = n {
                self.show_toast(&n);
            }
        }
    }

    fn redraw_all(&self) {
        self.redraw(Changed {
            health: true,
            voice: true,
            gpu: true,
            chat: true,
            ledgers: true,
            notice: false,
        });
    }

    fn redraw(&self, changed: Changed) {
        let st = self.state.borrow();
        for (_, p) in &self.pages {
            p.refresh(&st, changed);
        }
        if changed.voice {
            self.mic.refresh(&st.voice);
        }
        if changed.health {
            use octopit_core::health::Headline;
            let state = if st.have_snapshot {
                match st.snapshot.headline() {
                    Headline::Healthy => octopit_core::health::PartState::Ok,
                    Headline::Unreachable => octopit_core::health::PartState::Down,
                    Headline::Attention { state, .. } => {
                        if state == octopit_core::health::PartState::Down {
                            octopit_core::health::PartState::Down
                        } else {
                            octopit_core::health::PartState::Degraded
                        }
                    }
                }
            } else {
                octopit_core::health::PartState::Starting
            };
            set_dot(&self.pill_dot, state);
            self.pill_text.set_text(&st.headline_text());
            for c in ["ok", "degraded", "down", "loading"] {
                self.pill.remove_css_class(c);
            }
            self.pill.add_css_class(if st.have_snapshot {
                crate::widgets::state_class(state)
            } else {
                "loading"
            });
            set_fact(
                &self.fact_memory,
                &format!(
                    "Memory: {}",
                    format::bytes_iec(st.snapshot.total_rss_mib * 1024 * 1024)
                ),
            );
            let speed = st
                .snapshot
                .parts
                .iter()
                .find(|p| p.part == octoplus_common::paths::Part::Inference)
                .map_or(0, |p| p.p99_ns);
            set_fact(
                &self.fact_speed,
                &format!(
                    "Model speed: {}",
                    if speed > 0 {
                        format!("{}/token", format::latency(speed))
                    } else {
                        "\u{2014}".to_owned()
                    }
                ),
            );
        }
        if changed.ledgers || changed.health {
            set_fact(
                &self.fact_egress,
                &format!(
                    "Sent to network: {}",
                    st.ledgers
                        .egress_bytes_out()
                        .map_or_else(|| "\u{2014}".to_owned(), format::bytes_iec)
                ),
            );
        }
        if changed.gpu {
            let text = match &st.gpu {
                Some(GpuState::Present(r)) => r
                    .utilization_pct
                    .map_or_else(|| "\u{2014}".to_owned(), |u| format::percent(u / 100.0)),
                Some(GpuState::NotAvailable) => "none".to_owned(),
                None => "\u{2014}".to_owned(),
            };
            set_fact(&self.fact_gpu, &format!("GPU: {text}"));
        }
    }
}
