//! The GTK4 window (design.md §6). Behaviour rules from design.md §6.4:
//!
//! 1. Deny has default focus; Enter never approves; Esc denies.
//! 2. The approve button is inactive for `approve_delay_ms` after the window is
//!    shown **and again every time it regains focus**, so a keystroke meant for
//!    another window cannot approve.
//! 3. The approve button says what it does.
//! 4. DANGEROUS: solid danger button, disabled until the phrase is typed.
//! 5. Countdown; at zero the dialog closes and answers `expired`.
//! 7. Only fixed wording plus data; labels never interpret markup.
//! 9. The accessible name announces tier, egress, title and taint first.
//! 10. `app_id` is `org.neuroos.Confirm`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use gtk4 as gtk;

use crate::format;
use crate::protocol::{Decision, DialogRequest, DialogTier, ItemKind};
use crate::wording::{self, Wording};

include!(concat!(env!("OUT_DIR"), "/tokens.rs"));

const APP_ID: &str = "org.neuroos.Confirm";
const CSS_DARK: &str = include_str!(concat!(env!("OUT_DIR"), "/style-dark.css"));
const CSS_LIGHT: &str = include_str!(concat!(env!("OUT_DIR"), "/style-light.css"));
/// How often the countdown and the approve-button state refresh.
const TICK: Duration = Duration::from_millis(100);
/// Paths longer than this are truncated in the middle (full value in the tooltip).
const PATH_CHARS: usize = 44;

/// What the user decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The answer.
    pub decision: Decision,
    /// Final query text for an editable search form, on allow only.
    pub query: Option<String>,
}

/// Scripted checks run against the real window (`--self-test`), so the
/// behaviour rules are tested on the actual widgets, not on a copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelfTest {
    /// Press Enter (activate-default and activate-focus) right after opening: must deny.
    Enter,
    /// Try to approve inside the delay (must do nothing), then after it: must allow.
    AllowEarlyThenLate,
    /// The Esc shortcut: must deny.
    Escape,
    /// Do nothing: must expire.
    Expire,
    /// Edit the query, then approve: must allow with the edited query.
    EditQuery(String),
    /// DANGEROUS: approve without the phrase (nothing), type it, approve: must allow.
    DangerPhrase,
    /// Render the window to a PNG at this path, then deny.
    Screenshot(String),
}

struct Ui {
    window: gtk::Window,
    allow: gtk::Button,
    deny: gtk::Button,
    query: Option<gtk::Entry>,
    phrase: Option<(gtk::Entry, String)>,
    countdown: gtk::Label,
    delay: Duration,
    armed_at: Cell<Instant>,
    deadline: Instant,
    outcome: RefCell<Option<Outcome>>,
    failures: RefCell<Vec<String>>,
}

impl Ui {
    fn approvable(&self) -> bool {
        let delay_over = self.armed_at.get().elapsed() >= self.delay;
        let phrase_ok = self
            .phrase
            .as_ref()
            .is_none_or(|(entry, want)| entry.text().as_str() == want);
        let query_ok = self
            .query
            .as_ref()
            .is_none_or(|e| !e.text().trim().is_empty());
        delay_over && phrase_ok && query_ok
    }

    fn refresh(&self) {
        self.allow.set_sensitive(self.approvable());
        let left = self.deadline.saturating_duration_since(Instant::now());
        let ms = u64::try_from(left.as_millis()).unwrap_or(u64::MAX);
        self.countdown
            .set_text(&format!("Expires in {}", format::countdown(ms)));
    }

    /// Records the first answer only and closes the window.
    fn finish(&self, decision: Decision) {
        if self.outcome.borrow().is_some() {
            return;
        }
        let query = match (decision, &self.query) {
            (Decision::Allow, Some(e)) => Some(e.text().to_string()),
            _ => None,
        };
        *self.outcome.borrow_mut() = Some(Outcome { decision, query });
        self.window.close();
    }

    fn try_allow(&self) {
        // Re-check here too: a click that raced the sensitivity update must not approve.
        if self.approvable() {
            self.finish(Decision::Allow);
        }
    }

    fn fail(&self, msg: &str) {
        self.failures.borrow_mut().push(msg.to_owned());
    }
}

fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_use_markup(false);
    l.set_use_underline(false);
    l.set_wrap(true);
    l.set_xalign(0.0);
    l.set_css_classes(classes);
    l
}

fn prefers_dark() -> bool {
    // design.md §3.3: follow the desktop's dark preference; dark-first when unknown.
    let Some(settings) = gtk::Settings::default() else {
        return true;
    };
    if settings.is_gtk_application_prefer_dark_theme() {
        return true;
    }
    let theme = settings.gtk_theme_name().map(|t| t.to_lowercase());
    !matches!(theme, Some(t) if !t.contains("dark") && t != "adwaita")
}

fn install_css(window: &gtk::Window) {
    let dark = prefers_dark();
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(dark);
    }
    let provider = gtk::CssProvider::new();
    provider.load_from_string(if dark { CSS_DARK } else { CSS_LIGHT });
    gtk::style_context_add_provider_for_display(
        &WidgetExt::display(window),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn badges(req: &DialogRequest) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let (text, class) = match req.tier {
        DialogTier::Review => (format!("🛡 {}", wording::BADGE_REVIEW), "review"),
        DialogTier::Dangerous => (format!("🛡 {}", wording::BADGE_DANGEROUS), "dangerous"),
    };
    let badge = |text: &str, classes: &[&str]| {
        // Badges never wrap; a wrapping label in a horizontal box breaks height-for-width.
        let l = label(text, classes);
        l.set_wrap(false);
        l
    };
    row.append(&badge(&text, &["badge", class]));
    if req.egress.is_some() {
        row.append(&badge(
            &format!("↗ {}", wording::BADGE_EGRESS),
            &["badge", "egress"],
        ));
    }
    row
}

fn items_list(req: &DialogRequest) -> Option<gtk::Widget> {
    if req.items.is_empty() {
        return None;
    }
    let grid = gtk::Grid::builder()
        .column_spacing(16)
        .row_spacing(4)
        .build();
    grid.add_css_class("items");
    for (row, item) in (0_i32..).zip(&req.items) {
        let shown = match item.kind {
            ItemKind::MoveToTrash => format::middle_truncate(&item.path, PATH_CHARS),
            ItemKind::DeleteForever => format!(
                "{} {}",
                format::middle_truncate(&item.path, PATH_CHARS - 20),
                wording::DELETE_FOREVER_SUFFIX
            ),
        };
        let path = label(&shown, &["mono"]);
        path.set_wrap(false);
        path.set_hexpand(true);
        path.set_tooltip_text(Some(&item.path));
        let size = label(&format::size(item.size_bytes), &["mono", "size"]);
        size.set_xalign(1.0);
        grid.attach(&path, 0, row, 1, 1);
        grid.attach(&size, 1, row, 1, 1);
    }
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(240)
        .child(&grid)
        .build();
    Some(scroll.upcast())
}

fn params_box(req: &DialogRequest) -> Option<gtk::Widget> {
    if req.params.is_empty() {
        return None;
    }
    let grid = gtk::Grid::builder()
        .column_spacing(16)
        .row_spacing(4)
        .build();
    grid.add_css_class("items");
    for (row, p) in (0_i32..).zip(&req.params) {
        grid.attach(&label(&p.name, &["mono", "size"]), 0, row, 1, 1);
        let v = label(&p.value, &["mono"]);
        v.set_hexpand(true);
        grid.attach(&v, 1, row, 1, 1);
    }
    Some(grid.upcast())
}

fn taint_panel(req: &DialogRequest) -> Option<gtk::Widget> {
    let taint = req.taint.as_ref()?;
    let hosts = if taint.hosts.is_empty() {
        "the web".to_owned()
    } else {
        taint.hosts.join(", ")
    };
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 4);
    panel.add_css_class("taint");
    panel.append(&label(
        &format!("◆ {}", wording::BADGE_TAINT),
        &["badge-text"],
    ));
    panel.append(&label(&wording::TAINT_BODY.replace("{hosts}", &hosts), &[]));
    Some(panel.upcast())
}

/// The search/fetch form; returns the box and the editable entry, if any.
fn egress_form(req: &DialogRequest) -> Option<(gtk::Box, Option<gtk::Entry>)> {
    let e = req.egress.as_ref()?;
    let col = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let entry = gtk::Entry::new();
    entry.set_text(&e.query);
    entry.add_css_class("mono");
    entry.set_max_length(i32::try_from(crate::protocol::MAX_QUERY_LEN).unwrap_or(256));
    entry.set_editable(e.editable);
    // Enter inside the box must not approve either (rule 1).
    entry.set_activates_default(false);
    entry.update_property(&[gtk::accessible::Property::Label("Text that will be sent")]);
    col.append(&entry);
    if e.editable {
        col.append(&label(wording::EDIT_HINT, &["hint"]));
    }
    if e.pages > 0 {
        col.append(&label(
            &wording::PAGES_LINE.replace("{n}", &e.pages.to_string()),
            &[],
        ));
    }
    col.append(&label(wording::EGRESS_FOOTER, &[]));
    Some((col, e.editable.then_some(entry)))
}

fn details(req: &DialogRequest) -> gtk::Expander {
    let grid = gtk::Grid::builder()
        .column_spacing(16)
        .row_spacing(4)
        .build();
    grid.attach(&label("Capability", &["hint"]), 0, 0, 1, 1);
    grid.attach(&label(&req.capability, &["mono"]), 1, 0, 1, 1);
    grid.attach(&label("Action", &["hint"]), 0, 1, 1, 1);
    grid.attach(&label(&req.action_id, &["mono"]), 1, 1, 1, 1);
    let exp = gtk::Expander::new(Some(wording::DETAILS));
    exp.set_child(Some(&grid));
    exp
}

fn lead_text(w: &Wording, req: &DialogRequest) -> String {
    match &req.egress {
        Some(e) if w.lead.ends_with(" to") || w.lead.ends_with(" from") => {
            format!("{} {}:", w.lead, e.provider)
        }
        _ => w.lead.to_owned(),
    }
}

fn accessible_name(w: &Wording, req: &DialogRequest) -> String {
    let mut parts = vec![match req.tier {
        DialogTier::Review => wording::BADGE_REVIEW,
        DialogTier::Dangerous => wording::BADGE_DANGEROUS,
    }];
    if req.egress.is_some() {
        parts.push(wording::BADGE_EGRESS);
    }
    parts.push(w.title);
    if req.taint.is_some() {
        parts.push(wording::BADGE_TAINT);
    }
    parts.join(". ")
}

/// Builds the body and returns (body, query entry, phrase entry).
fn body(req: &DialogRequest, w: &Wording) -> (gtk::Box, Option<gtk::Entry>, Option<gtk::Entry>) {
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.add_css_class("body");
    body.append(&badges(req));
    body.append(&label(w.title, &["title"]));
    if let Some(t) = taint_panel(req) {
        body.append(&t);
    }
    body.append(&label(&lead_text(w, req), &["lead"]));
    if let Some(list) = items_list(req) {
        body.append(&list);
        let total: u64 = req
            .items
            .iter()
            .map(|i| i.size_bytes)
            .fold(0, u64::saturating_add);
        body.append(&label(
            &format!("{} {}", wording::TOTAL, format::size(total)),
            &[],
        ));
    }
    if let Some(p) = params_box(req) {
        body.append(&p);
    }
    let mut query = None;
    if let Some((form, entry)) = egress_form(req) {
        body.append(&form);
        query = entry;
    }
    let mut phrase = None;
    if let Some(want) = &req.danger_phrase {
        body.append(&label(&format!("{} {want}", wording::DANGER_PROMPT), &[]));
        let entry = gtk::Entry::new();
        entry.add_css_class("mono");
        entry.set_activates_default(false);
        entry.update_property(&[gtk::accessible::Property::Label(wording::DANGER_PROMPT)]);
        body.append(&entry);
        phrase = Some(entry);
    }
    body.append(&details(req));
    (body, query, phrase)
}

fn build(app: &gtk::Application, req: &DialogRequest) -> Rc<Ui> {
    let w = wording::for_capability(&req.capability);
    let window = gtk::Window::builder()
        .application(app)
        .title(wording::WINDOW_TITLE)
        .default_width(DIALOG_WIDTH)
        .resizable(false)
        .modal(true)
        .build();
    window.add_css_class("neuroos-confirm");
    install_css(&window);
    window.update_property(&[gtk::accessible::Property::Label(&accessible_name(&w, req))]);

    let (content, query, phrase) = body(req, &w);
    let deny = gtk::Button::with_label(wording::DENY);
    let allow = gtk::Button::with_label(w.approve);
    allow.add_css_class("approve");
    if req.tier == DialogTier::Dangerous {
        allow.add_css_class("danger");
    }
    allow.set_receives_default(false);
    allow.set_sensitive(false);
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    buttons.set_halign(gtk::Align::End);
    buttons.append(&deny);
    buttons.append(&allow);
    content.append(&buttons);
    let countdown = label("", &["countdown"]);
    content.append(&countdown);
    window.set_child(Some(&content));
    // Rule 1: Deny is the default widget, so Enter anywhere activates Deny.
    window.set_default_widget(Some(&deny));

    let now = Instant::now();
    Rc::new(Ui {
        window,
        allow,
        deny,
        query,
        phrase: phrase.map(|e| (e, req.danger_phrase.clone().unwrap_or_default())),
        countdown,
        delay: Duration::from_millis(req.approve_delay_ms),
        armed_at: Cell::new(now),
        deadline: now + Duration::from_millis(req.expires_in_ms),
        outcome: RefCell::new(None),
        failures: RefCell::new(Vec::new()),
    })
}

fn wire(ui: &Rc<Ui>) {
    let u = Rc::clone(ui);
    ui.deny.connect_clicked(move |_| u.finish(Decision::Deny));
    let u = Rc::clone(ui);
    ui.allow.connect_clicked(move |_| u.try_allow());

    // Esc denies (rule 1) through a named action so it is also reachable from tests.
    let group = gio::SimpleActionGroup::new();
    let deny_action = gio::SimpleAction::new("deny", None);
    let u = Rc::clone(ui);
    deny_action.connect_activate(move |_, _| u.finish(Decision::Deny));
    group.add_action(&deny_action);
    ui.window.insert_action_group("confirm", Some(&group));
    let shortcuts = gtk::ShortcutController::new();
    shortcuts.set_scope(gtk::ShortcutScope::Global);
    shortcuts.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Escape"),
        Some(gtk::NamedAction::new("confirm.deny")),
    ));
    ui.window.add_controller(shortcuts);

    // Closing the window without an answer is a denial.
    let u = Rc::clone(ui);
    ui.window.connect_close_request(move |_| {
        if u.outcome.borrow().is_none() {
            *u.outcome.borrow_mut() = Some(Outcome {
                decision: Decision::Deny,
                query: None,
            });
        }
        glib::Propagation::Proceed
    });

    // Rule 2: re-arm the delay whenever the window becomes active again.
    let u = Rc::clone(ui);
    ui.window.connect_is_active_notify(move |w| {
        if w.is_active() {
            u.armed_at.set(Instant::now());
            u.refresh();
        }
    });
    for entry in ui.query.iter().chain(ui.phrase.iter().map(|(e, _)| e)) {
        let u = Rc::clone(ui);
        entry.connect_changed(move |_| u.refresh());
    }

    let u = Rc::clone(ui);
    glib::timeout_add_local(TICK, move || {
        if u.outcome.borrow().is_some() {
            return glib::ControlFlow::Break;
        }
        if Instant::now() >= u.deadline {
            u.finish(Decision::Expired);
            return glib::ControlFlow::Break;
        }
        u.refresh();
        glib::ControlFlow::Continue
    });
}

fn after(ms: u64, f: impl FnOnce() + 'static) {
    glib::timeout_add_local_once(Duration::from_millis(ms), f);
}

fn screenshot(ui: &Ui, path: &str) -> Result<(), String> {
    let paintable = gtk::WidgetPaintable::new(Some(&ui.window));
    let snap = gtk::Snapshot::new();
    let (w, h) = (f64::from(ui.window.width()), f64::from(ui.window.height()));
    gdk::prelude::PaintableExt::snapshot(&paintable, snap.upcast_ref::<gdk::Snapshot>(), w, h);
    let node = snap.to_node().ok_or("empty snapshot")?;
    let renderer = ui
        .window
        .native()
        .and_then(|n| n.renderer())
        .ok_or("no renderer")?;
    let texture = renderer.render_texture(node, None);
    texture.save_to_png(path).map_err(|e| e.to_string())
}

fn schedule_self_test(ui: &Rc<Ui>, test: SelfTest) {
    let delay = u64::try_from(ui.delay.as_millis()).unwrap_or(u64::MAX);
    let u = Rc::clone(ui);
    match test {
        SelfTest::Enter => after(300, move || {
            if GtkWindowExt::focus(&u.window).as_ref() != Some(u.deny.upcast_ref()) {
                u.fail("deny does not have focus on open");
            }
            if u.allow.is_sensitive() {
                u.fail("approve button active on open");
            }
            // Exactly what the Enter key binding does on a GtkWindow.
            u.window.emit_by_name::<()>("activate-default", &[]);
            u.window.emit_by_name::<()>("activate-focus", &[]);
        }),
        SelfTest::AllowEarlyThenLate => after(50, move || {
            u.allow.activate();
            u.allow.emit_clicked();
            if u.outcome.borrow().is_some() {
                u.fail("approved inside the delay");
            }
            after(delay + 300, move || {
                u.allow.activate();
            });
        }),
        SelfTest::Escape => after(300, move || {
            let _ = WidgetExt::activate_action(&u.window, "confirm.deny", None);
        }),
        SelfTest::Expire => {}
        SelfTest::EditQuery(text) => after(delay + 300, move || {
            if let Some(e) = &u.query {
                e.set_text(&text);
            } else {
                u.fail("no editable query");
            }
            u.allow.activate();
        }),
        SelfTest::DangerPhrase => after(delay + 300, move || {
            u.allow.activate();
            u.allow.emit_clicked();
            if u.outcome.borrow().is_some() {
                u.fail("approved without the phrase");
            }
            if let Some((entry, want)) = &u.phrase {
                entry.set_text(want);
            } else {
                u.fail("no phrase entry");
            }
            u.allow.activate();
        }),
        SelfTest::Screenshot(path) => after(800, move || {
            if let Err(e) = screenshot(&u, &path) {
                u.fail(&format!("screenshot: {e}"));
            }
            u.finish(Decision::Deny);
        }),
    }
}

/// Shows the dialog and blocks until the user answers, the countdown ends or
/// the window is closed. Returns the outcome and any self-test failures.
#[must_use]
pub fn run(req: &DialogRequest, self_test: Option<SelfTest>) -> (Outcome, Vec<String>) {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let slot: Rc<RefCell<Option<Rc<Ui>>>> = Rc::new(RefCell::new(None));
    let s = Rc::clone(&slot);
    let req = req.clone();
    app.connect_activate(move |app| {
        let ui = build(app, &req);
        wire(&ui);
        ui.window.present();
        ui.deny.grab_focus();
        ui.refresh();
        if let Some(t) = self_test.clone() {
            schedule_self_test(&ui, t);
        }
        *s.borrow_mut() = Some(ui);
    });
    app.run_with_args::<&str>(&[]);
    let ui = slot.borrow_mut().take();
    match ui {
        Some(ui) => {
            let outcome = ui.outcome.borrow_mut().take().unwrap_or(Outcome {
                decision: Decision::Deny,
                query: None,
            });
            let failures = ui.failures.borrow().clone();
            (outcome, failures)
        }
        None => (
            Outcome {
                decision: Decision::Deny,
                query: None,
            },
            vec!["window never opened".to_owned()],
        ),
    }
}
