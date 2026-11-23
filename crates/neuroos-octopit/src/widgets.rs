//! Small reusable pieces: status dot, card, sparkline, gauge ring, skeleton line.
//! Every piece draws from the theme's colours and has a text equivalent for screen readers.

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk4 as gtk;

use neuroos_octopit_core::health::PartState;

/// CSS class suffix for a part state.
pub const fn state_class(s: PartState) -> &'static str {
    match s {
        PartState::Ok => "ok",
        PartState::Degraded => "degraded",
        PartState::Down => "down",
        PartState::Starting => "starting",
    }
}

/// Plain word for a part state (never colour alone).
pub const fn state_word(s: PartState) -> &'static str {
    match s {
        PartState::Ok => "Running",
        PartState::Degraded => "Needs attention",
        PartState::Down => "Not running",
        PartState::Starting => "Starting",
    }
}

/// A round status dot. Call [`set_dot`] to change it.
pub fn dot(s: PartState) -> gtk::Box {
    let d = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    d.add_css_class("dot");
    d.set_valign(gtk::Align::Center);
    d.set_accessible_role(gtk::AccessibleRole::Presentation);
    set_dot(&d, s);
    d
}

/// Sets a dot's state.
pub fn set_dot(d: &gtk::Box, s: PartState) {
    for c in ["ok", "degraded", "down", "starting"] {
        d.remove_css_class(c);
    }
    d.add_css_class(state_class(s));
}

/// A card container with the given extra classes.
pub fn card(classes: &[&str]) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 8);
    b.add_css_class("card");
    for c in classes {
        b.add_css_class(c);
    }
    b
}

/// A label with CSS classes.
pub fn label(text: &str, classes: &[&str]) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_xalign(0.0);
    l.set_use_markup(false);
    for c in classes {
        l.add_css_class(c);
    }
    l
}

/// A wrapped, selectable label that never reads markup (model text goes here).
pub fn plain_text(text: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.set_use_markup(false);
    l.set_wrap(true);
    l.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    l.set_xalign(0.0);
    l.set_selectable(true);
    l
}

/// Keeps `target` at most `max_px` wide and centred inside `tracker` (GTK has no clamp widget
/// without libadwaita): the margins follow the tracker's width on every frame it changes.
/// `min_start` and `min_end` are the smallest margins (the floating mic needs room at the end).
pub fn clamp_width(
    tracker: &impl IsA<gtk::Widget>,
    target: &impl IsA<gtk::Widget>,
    max_px: i32,
    min_start: i32,
    min_end: i32,
) {
    let target = target.clone().upcast::<gtk::Widget>();
    let last = Cell::new(-1);
    tracker.add_tick_callback(move |t, _| {
        let w = t.width();
        if w != last.get() {
            last.set(w);
            let side = ((w - max_px) / 2).max(0);
            target.set_margin_start(side.max(min_start));
            target.set_margin_end(side.max(min_end));
        }
        gtk::glib::ControlFlow::Continue
    });
}

/// A themed icon from the bundled set.
pub fn icon(name: &str, size: i32) -> gtk::Image {
    let i = gtk::Image::from_icon_name(name);
    i.set_pixel_size(size);
    i
}

/// A grey bar that stands in for text while data loads.
pub fn skeleton(width: i32) -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.add_css_class("skeleton");
    b.set_size_request(width, 14);
    b.set_halign(gtk::Align::Start);
    b
}

/// A slim progress bar for a fraction `0.0..=1.0`; turns amber at 80% and red at 95%.
pub fn set_bar(bar: &gtk::ProgressBar, fraction: f64) {
    bar.set_fraction(fraction.clamp(0.0, 1.0));
    bar.remove_css_class("warn");
    bar.remove_css_class("hot");
    if fraction >= 0.95 {
        bar.add_css_class("hot");
    } else if fraction >= 0.8 {
        bar.add_css_class("warn");
    }
}

/// A new slim bar.
pub fn bar() -> gtk::ProgressBar {
    let b = gtk::ProgressBar::new();
    b.add_css_class("slim");
    b
}

/// The colours a drawing needs: the widget's own text colour (set per class in the stylesheet),
/// so light, dark and state colours all work without hard-coded values.
#[derive(Clone, Copy)]
struct Palette {
    line: gtk::gdk::RGBA,
    track: gtk::gdk::RGBA,
}

fn palette(w: &impl IsA<gtk::Widget>) -> Palette {
    let line = w.color();
    let track = gtk::gdk::RGBA::new(line.red(), line.green(), line.blue(), 0.22);
    Palette { line, track }
}

/// A sparkline: the last samples, scaled to `0.0..=1.0`, drawn as a smooth line with a soft fill.
#[derive(Clone)]
pub struct Spark {
    area: gtk::DrawingArea,
    data: Rc<std::cell::RefCell<Vec<f64>>>,
}

impl Spark {
    /// A sparkline of the given height with a description for screen readers.
    pub fn new(height: i32, description: &str) -> Self {
        let area = gtk::DrawingArea::new();
        area.add_css_class("spark");
        area.set_content_height(height);
        area.set_hexpand(true);
        area.update_property(&[gtk::accessible::Property::Label(description)]);
        let data: Rc<std::cell::RefCell<Vec<f64>>> = Rc::default();
        let d = Rc::clone(&data);
        area.set_draw_func(move |a, cr, w, h| {
            let pal = palette(a);
            let pts = d.borrow();
            let (w, h) = (f64::from(w), f64::from(h));
            // baseline
            cr.set_source_rgba(
                f64::from(pal.track.red()),
                f64::from(pal.track.green()),
                f64::from(pal.track.blue()),
                f64::from(pal.track.alpha()),
            );
            cr.set_line_width(1.0);
            cr.move_to(0.0, h - 0.5);
            cr.line_to(w, h - 0.5);
            let _ = cr.stroke();
            if pts.len() < 2 {
                return;
            }
            #[allow(clippy::cast_precision_loss)]
            let step = w / (pts.len() - 1) as f64;
            let y = |v: f64| (h - 3.0) - v.clamp(0.0, 1.0) * (h - 8.0);
            cr.move_to(0.0, y(pts[0]));
            for (i, v) in pts.iter().enumerate().skip(1) {
                #[allow(clippy::cast_precision_loss)]
                let x = step * i as f64;
                cr.line_to(x, y(*v));
            }
            let (r, g, b) = (
                f64::from(pal.line.red()),
                f64::from(pal.line.green()),
                f64::from(pal.line.blue()),
            );
            cr.set_source_rgba(r, g, b, 1.0);
            cr.set_line_width(1.75);
            cr.set_line_join(gtk::cairo::LineJoin::Round);
            let _ = cr.stroke_preserve();
            cr.line_to(w, h);
            cr.line_to(0.0, h);
            cr.close_path();
            cr.set_source_rgba(r, g, b, 0.14);
            let _ = cr.fill();
        });
        Self { area, data }
    }

    /// The widget to place in a container.
    pub fn widget(&self) -> &gtk::DrawingArea {
        &self.area
    }

    /// Replaces the samples (already scaled to `0.0..=1.0`) and redraws.
    pub fn set(&self, normalized: Vec<f64>) {
        *self.data.borrow_mut() = normalized;
        self.area.queue_draw();
    }
}

/// A ring gauge for a fraction `0.0..=1.0` with a centre text.
#[derive(Clone)]
pub struct Ring {
    area: gtk::DrawingArea,
    fraction: Rc<Cell<f64>>,
}

impl Ring {
    /// A square ring of `size` pixels.
    pub fn new(size: i32, description: &str) -> Self {
        let area = gtk::DrawingArea::new();
        area.add_css_class("ring");
        area.set_content_width(size);
        area.set_content_height(size);
        area.set_halign(gtk::Align::Center);
        area.update_property(&[gtk::accessible::Property::Label(description)]);
        let fraction = Rc::new(Cell::new(0.0_f64));
        let f = Rc::clone(&fraction);
        area.set_draw_func(move |a, cr, w, h| {
            let pal = palette(a);
            let (w, h) = (f64::from(w), f64::from(h));
            let radius = (w.min(h) / 2.0) - 6.0;
            let (cx, cy) = (w / 2.0, h / 2.0);
            let start = 0.75 * std::f64::consts::PI;
            let sweep = 1.5 * std::f64::consts::PI;
            cr.set_line_width(8.0);
            cr.set_line_cap(gtk::cairo::LineCap::Round);
            cr.set_source_rgba(
                f64::from(pal.track.red()),
                f64::from(pal.track.green()),
                f64::from(pal.track.blue()),
                f64::from(pal.track.alpha()),
            );
            cr.arc(cx, cy, radius, start, start + sweep);
            let _ = cr.stroke();
            let v = f.get().clamp(0.0, 1.0);
            if v > 0.0 {
                cr.set_source_rgba(
                    f64::from(pal.line.red()),
                    f64::from(pal.line.green()),
                    f64::from(pal.line.blue()),
                    1.0,
                );
                cr.arc(cx, cy, radius, start, start + sweep * v);
                let _ = cr.stroke();
            }
        });
        Self { area, fraction }
    }

    /// The widget.
    pub fn widget(&self) -> &gtk::DrawingArea {
        &self.area
    }

    /// Sets the filled fraction and redraws.
    pub fn set(&self, fraction: f64) {
        self.fraction.set(fraction);
        self.area.queue_draw();
    }
}
