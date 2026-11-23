//! Chart and decoration widgets for the OctoPit v2 visual language (docs/design/octopit-ux.md §10).

#![allow(dead_code)] // used by the v2 pages

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;

/// Returns the RGBA colour for chart series index `i` (0..8, wrapping).
#[must_use]
pub fn series_rgba(i: usize) -> gtk::gdk::RGBA {
    let hex = crate::tokens::SERIES[i % crate::tokens::SERIES.len()];
    parse_hex_rgba(hex)
}

/// Parses a `#RRGGBB` hex colour into a `gtk::gdk::RGBA`.
#[must_use]
pub fn parse_hex_rgba(hex: &str) -> gtk::gdk::RGBA {
    let s = hex.trim_start_matches('#');
    if s.len() == 6
        && let Ok(r) = u8::from_str_radix(&s[0..2], 16)
        && let Ok(g) = u8::from_str_radix(&s[2..4], 16)
        && let Ok(b) = u8::from_str_radix(&s[4..6], 16)
    {
        return gtk::gdk::RGBA::new(
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
            1.0,
        );
    }
    gtk::gdk::RGBA::new(0.5, 0.5, 0.5, 1.0)
}

/// Returns the CSS class name for series `i` ("series-0" .. "series-7").
#[must_use]
pub fn series_css_class(i: usize) -> String {
    format!("series-{}", i % 8)
}

/// Computes arc intervals `(start_rad, end_rad)` for segments starting at 12 o'clock and moving clockwise.
///
/// Handles negative and NaN values (treated as 0). Empty or all-zero values produce 0-span arcs or an empty vector.
/// Gaps are inserted between adjacent positive segments when there is more than one segment.
#[must_use]
pub fn donut_arcs(values: &[f64], gap_deg: f64) -> Vec<(f64, f64)> {
    if values.is_empty() {
        return Vec::new();
    }
    let sanitized: Vec<f64> = values
        .iter()
        .map(|&v| if v.is_finite() && v > 0.0 { v } else { 0.0 })
        .collect();
    let total: f64 = sanitized.iter().sum();
    if total <= 0.0 {
        return vec![(0.0, 0.0); values.len()];
    }

    let positive_count = sanitized.iter().filter(|&&v| v > 0.0).count();
    let gap_rad = if positive_count > 1 {
        gap_deg.to_radians().max(0.0)
    } else {
        0.0
    };

    #[allow(clippy::cast_precision_loss)]
    let total_gap_rad = gap_rad * positive_count as f64;
    let (effective_gap_rad, available_rad) = if total_gap_rad >= std::f64::consts::TAU {
        (0.0, std::f64::consts::TAU)
    } else {
        (gap_rad, std::f64::consts::TAU - total_gap_rad)
    };

    let start_offset = -std::f64::consts::FRAC_PI_2; // 12 o'clock
    let mut current_angle = start_offset;
    let mut result = Vec::with_capacity(values.len());

    for &val in &sanitized {
        if val <= 0.0 {
            result.push((current_angle, current_angle));
        } else {
            let sweep = (val / total) * available_rad;
            let end_angle = current_angle + sweep;
            result.push((current_angle, end_angle));
            current_angle = end_angle + effective_gap_rad;
        }
    }

    result
}

/// A ring (donut) chart widget.
#[derive(Clone)]
pub struct Donut {
    area: gtk::DrawingArea,
    segments: Rc<RefCell<Vec<(f64, usize)>>>,
}

impl Donut {
    /// Creates a square donut chart with diameter `size` pixels and an accessible description.
    #[must_use]
    pub fn new(size: i32, description: &str) -> Self {
        let area = gtk::DrawingArea::new();
        area.add_css_class("donut");
        area.set_content_width(size);
        area.set_content_height(size);
        area.set_halign(gtk::Align::Center);
        area.set_valign(gtk::Align::Center);
        area.update_property(&[gtk::accessible::Property::Label(description)]);

        let segments: Rc<RefCell<Vec<(f64, usize)>>> = Rc::default();
        let segs_draw = Rc::clone(&segments);

        area.set_draw_func(move |a, cr, w, h| {
            let (w, h) = (f64::from(w), f64::from(h));
            let dim = w.min(h);
            if dim <= 0.0 {
                return;
            }
            let stroke = (dim * 0.14).max(2.0);
            let radius = (dim - stroke) / 2.0;
            let (cx, cy) = (w / 2.0, h / 2.0);

            let data = segs_draw.borrow();
            let values: Vec<f64> = data.iter().map(|(v, _)| *v).collect();
            let arcs = donut_arcs(&values, 2.0);

            let col = a.color();
            let (r, g, b) = (
                f64::from(col.red()),
                f64::from(col.green()),
                f64::from(col.blue()),
            );

            // Track background ring
            cr.set_line_width(stroke);
            cr.set_line_cap(gtk::cairo::LineCap::Round);
            cr.set_source_rgba(r, g, b, 0.15);
            cr.arc(cx, cy, radius, 0.0, std::f64::consts::TAU);
            let _ = cr.stroke();

            // Active segments
            for (i, &(start, end)) in arcs.iter().enumerate() {
                if end > start {
                    let series = data.get(i).map_or(0, |(_, s)| *s);
                    let rgba = series_rgba(series);
                    cr.set_source_rgba(
                        f64::from(rgba.red()),
                        f64::from(rgba.green()),
                        f64::from(rgba.blue()),
                        1.0,
                    );
                    cr.arc(cx, cy, radius, start, end);
                    let _ = cr.stroke();
                }
            }
        });

        Self { area, segments }
    }

    /// Returns the underlying GTK DrawingArea.
    #[must_use]
    pub fn widget(&self) -> &gtk::DrawingArea {
        &self.area
    }

    /// Sets the chart segments `(value, series_index)` and requests a redraw.
    pub fn set(&self, segments: &[(f64, usize)]) {
        *self.segments.borrow_mut() = segments.to_vec();
        self.area.queue_draw();
    }
}

/// One bar in a [`BarChart`].
#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    /// Label shown under the bar.
    pub label: String,
    /// Fraction of height `0.0..=1.0`.
    pub fraction: f64,
    /// Series index for colour selection (0..8).
    pub series: usize,
}

/// Computes rectangle positions `(x, y, w, h)` for `n` vertical bars.
///
/// Width is capped up to 28 px without overlapping. Height is scaled by fraction `0.0..=1.0`
/// from baseline at `y = height`.
#[must_use]
pub fn bar_rects(
    width: f64,
    height: f64,
    n: usize,
    fractions: &[f64],
) -> Vec<(f64, f64, f64, f64)> {
    if n == 0 || width <= 0.0 || height <= 0.0 {
        return Vec::new();
    }
    #[allow(clippy::cast_precision_loss)]
    let slot_w = width / n as f64;
    let max_w = 28.0_f64;
    let bar_w = (slot_w * 0.6).min(max_w).max(0.0);

    let mut rects = Vec::with_capacity(n);
    for i in 0..n {
        let frac = fractions.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
        #[allow(clippy::cast_precision_loss)]
        let slot_x = i as f64 * slot_w;
        let x = slot_x + (slot_w - bar_w) / 2.0;
        let bar_h = frac * height;
        let y = height - bar_h;
        rects.push((x, y, bar_w, bar_h));
    }
    rects
}

/// Draws a vertical rounded rectangle (capsule/pill) with radius = `w / 2.0`.
fn rounded_bar(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let r = (w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(
        x + r,
        y + h - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        1.5 * std::f64::consts::PI,
    );
    cr.close_path();
}

/// A vertical bar chart with guide lines and labels.
#[derive(Clone)]
pub struct BarChart {
    area: gtk::DrawingArea,
    bars: Rc<RefCell<Vec<Bar>>>,
}

impl BarChart {
    /// Creates a new bar chart of the given height with an accessible description.
    #[must_use]
    pub fn new(height: i32, description: &str) -> Self {
        let area = gtk::DrawingArea::new();
        area.add_css_class("barchart");
        area.set_content_height(height);
        area.set_hexpand(true);
        area.update_property(&[gtk::accessible::Property::Label(description)]);

        let bars: Rc<RefCell<Vec<Bar>>> = Rc::default();
        let bars_draw = Rc::clone(&bars);

        area.set_draw_func(move |a, cr, w, h| {
            let (w, h) = (f64::from(w), f64::from(h));
            if w <= 0.0 || h <= 0.0 {
                return;
            }

            let label_h = 22.0_f64;
            let chart_h = (h - label_h).max(10.0);

            let col = a.color();
            let (cr_r, cr_g, cr_b) = (
                f64::from(col.red()),
                f64::from(col.green()),
                f64::from(col.blue()),
            );

            // Guide lines at 25%, 50%, 75% and baseline at chart_h (100% scale)
            cr.set_dash(&[2.0, 4.0], 0.0);
            cr.set_line_width(1.0);
            cr.set_source_rgba(cr_r, cr_g, cr_b, 0.15);
            for frac in [0.25, 0.50, 0.75, 1.0] {
                let y = chart_h * (1.0 - frac);
                cr.move_to(0.0, y + 0.5);
                cr.line_to(w, y + 0.5);
                let _ = cr.stroke();
            }
            // Baseline at chart_h
            cr.move_to(0.0, chart_h + 0.5);
            cr.line_to(w, chart_h + 0.5);
            let _ = cr.stroke();
            cr.set_dash(&[], 0.0);

            let bar_list = bars_draw.borrow();
            let n = bar_list.len();
            if n == 0 {
                return;
            }

            let fractions: Vec<f64> = bar_list.iter().map(|b| b.fraction).collect();
            let rects = bar_rects(w, chart_h, n, &fractions);

            #[allow(clippy::cast_precision_loss)]
            let slot_w = w / n as f64;

            // Setup font for labels
            cr.select_font_face(
                "sans-serif",
                gtk::cairo::FontSlant::Normal,
                gtk::cairo::FontWeight::Normal,
            );
            cr.set_font_size(9.5);

            for (i, b) in bar_list.iter().enumerate() {
                if let Some(&(bx, by, bw, bh)) = rects.get(i) {
                    if bw > 0.0 && bh > 0.0 {
                        let rgba = series_rgba(b.series);
                        cr.set_source_rgba(
                            f64::from(rgba.red()),
                            f64::from(rgba.green()),
                            f64::from(rgba.blue()),
                            1.0,
                        );
                        rounded_bar(cr, bx, by, bw, bh);
                        let _ = cr.fill();
                    }

                    #[allow(clippy::cast_precision_loss)]
                    let slot_x = i as f64 * slot_w;

                    // Also create pango layout as required by pango text handling
                    let layout = a.create_pango_layout(Some(&b.label));
                    let mut font_desc = gtk::pango::FontDescription::new();
                    font_desc.set_size(9 * gtk::pango::SCALE);
                    layout.set_font_description(Some(&font_desc));
                    layout.set_ellipsize(gtk::pango::EllipsizeMode::End);
                    layout.set_alignment(gtk::pango::Alignment::Center);
                    #[allow(clippy::cast_possible_truncation)]
                    layout.set_width((slot_w * f64::from(gtk::pango::SCALE)).max(1.0) as i32);

                    // Cut to fit with ellipsis if needed
                    let mut label_text = b.label.clone();
                    if let Ok(ext) = cr.text_extents(&label_text)
                        && ext.width() > slot_w
                    {
                        while !label_text.is_empty() {
                            label_text.pop();
                            let candidate = format!("{label_text}\u{2026}");
                            if let Ok(c_ext) = cr.text_extents(&candidate)
                                && c_ext.width() <= slot_w
                            {
                                label_text = candidate;
                                break;
                            }
                        }
                    }

                    cr.set_source_rgba(cr_r, cr_g, cr_b, 0.70);
                    let text_w = cr.text_extents(&label_text).map_or(0.0, |e| e.width());
                    let tx = (slot_x + (slot_w - text_w) / 2.0).max(slot_x);
                    let ty = chart_h + 15.0;
                    cr.move_to(tx, ty);
                    let _ = cr.show_text(&label_text);
                }
            }
        });

        Self { area, bars }
    }

    /// Returns the underlying DrawingArea.
    #[must_use]
    pub fn widget(&self) -> &gtk::DrawingArea {
        &self.area
    }

    /// Replaces the bars and requests a redraw.
    pub fn set(&self, bars: &[Bar]) {
        *self.bars.borrow_mut() = bars.to_vec();
        self.area.queue_draw();
    }
}

/// Tint state for the 3D glass [`Orb`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OrbTint {
    /// Healthy pastel gradient (violet -> pink -> peach with blue highlight).
    #[default]
    Healthy,
    /// Peach/amber-to-pink gradient for attention.
    Attention,
    /// Soft grey gradient when systems are down.
    Down,
}

/// Normalized offsets `(dx, dy)` for 3 swirling blobs, bounded to `+-0.06` (+-6% of size) and strictly periodic.
#[must_use]
pub fn blob_offsets(t_seconds: f64) -> [(f64, f64); 3] {
    let freq = 1.0_f64; // Period = 2*PI seconds (~6.28 s)
    [
        (
            0.050 * (freq * t_seconds).sin(),
            0.050 * (freq * t_seconds).cos(),
        ),
        (
            0.055 * (freq * t_seconds + 2.094_395_102_393_195_3).sin(), // + 2*PI/3
            0.055 * (freq * t_seconds + 2.094_395_102_393_195_3).cos(),
        ),
        (
            0.045 * (freq * t_seconds + 4.188_790_204_786_390_5).sin(), // + 4*PI/3
            0.045 * (freq * t_seconds + 4.188_790_204_786_390_5).cos(),
        ),
    ]
}

/// An iridescent glass orb that breathes when motion is enabled.
#[derive(Clone)]
pub struct Orb {
    area: gtk::DrawingArea,
    tint: Rc<Cell<OrbTint>>,
}

impl Orb {
    /// Creates a square orb widget of `size` pixels.
    #[must_use]
    pub fn new(size: i32) -> Self {
        let area = gtk::DrawingArea::new();
        area.add_css_class("orb");
        area.set_content_width(size);
        area.set_content_height(size);
        area.set_halign(gtk::Align::Center);
        area.set_valign(gtk::Align::Center);
        area.update_property(&[gtk::accessible::Property::Label("System status orb")]);

        let tint: Rc<Cell<OrbTint>> = Rc::default();
        let tint_draw = Rc::clone(&tint);

        let start_time = Instant::now();
        let elapsed_secs: Rc<Cell<f64>> = Rc::default();
        let elapsed_draw = Rc::clone(&elapsed_secs);

        area.set_draw_func(move |_a, cr, w, h| {
            let (w, h) = (f64::from(w), f64::from(h));
            let dim = w.min(h);
            if dim <= 0.0 {
                return;
            }

            let cx = w / 2.0;
            let cy = h * 0.46;
            let r = dim * 0.38;

            // Soft shadow ellipse under the sphere (radial, alpha 0.18)
            let shadow_y = cy + r * 1.05;
            let shadow_rx = r * 0.85;
            let shadow_ry = r * 0.22;
            if shadow_rx > 0.0 && shadow_ry > 0.0 {
                let _ = cr.save();
                cr.translate(cx, shadow_y);
                cr.scale(1.0, shadow_ry / shadow_rx);
                let shadow_pat =
                    gtk::cairo::RadialGradient::new(0.0, 0.0, 0.0, 0.0, 0.0, shadow_rx);
                shadow_pat.add_color_stop_rgba(0.0, 0.0, 0.0, 0.0, 0.18);
                shadow_pat.add_color_stop_rgba(0.6, 0.0, 0.0, 0.0, 0.08);
                shadow_pat.add_color_stop_rgba(1.0, 0.0, 0.0, 0.0, 0.0);
                let _ = cr.set_source(&shadow_pat);
                cr.arc(0.0, 0.0, shadow_rx, 0.0, std::f64::consts::TAU);
                let _ = cr.fill();
                let _ = cr.restore();
            }

            // Base sphere radial gradient
            let base_pat =
                gtk::cairo::RadialGradient::new(cx - r * 0.35, cy - r * 0.35, r * 0.05, cx, cy, r);
            match tint_draw.get() {
                OrbTint::Healthy => {
                    // violet SERIES[0] -> pink SERIES[1] -> peach SERIES[2] with blue SERIES[3] highlight
                    let c0 = series_rgba(0);
                    let c1 = series_rgba(1);
                    let c2 = series_rgba(2);
                    let c3 = series_rgba(3);
                    base_pat.add_color_stop_rgba(
                        0.0,
                        f64::from(c3.red()),
                        f64::from(c3.green()),
                        f64::from(c3.blue()),
                        0.95,
                    );
                    base_pat.add_color_stop_rgba(
                        0.28,
                        f64::from(c0.red()),
                        f64::from(c0.green()),
                        f64::from(c0.blue()),
                        0.90,
                    );
                    base_pat.add_color_stop_rgba(
                        0.68,
                        f64::from(c1.red()),
                        f64::from(c1.green()),
                        f64::from(c1.blue()),
                        0.88,
                    );
                    base_pat.add_color_stop_rgba(
                        1.0,
                        f64::from(c2.red()),
                        f64::from(c2.green()),
                        f64::from(c2.blue()),
                        0.85,
                    );
                }
                OrbTint::Attention => {
                    // peach/amber-to-pink
                    let c1 = series_rgba(1);
                    let c2 = series_rgba(2);
                    let c6 = series_rgba(6);
                    base_pat.add_color_stop_rgba(0.0, 1.0, 0.95, 0.88, 0.95);
                    base_pat.add_color_stop_rgba(
                        0.35,
                        f64::from(c2.red()),
                        f64::from(c2.green()),
                        f64::from(c2.blue()),
                        0.90,
                    );
                    base_pat.add_color_stop_rgba(
                        0.72,
                        f64::from(c6.red()),
                        f64::from(c6.green()),
                        f64::from(c6.blue()),
                        0.88,
                    );
                    base_pat.add_color_stop_rgba(
                        1.0,
                        f64::from(c1.red()),
                        f64::from(c1.green()),
                        f64::from(c1.blue()),
                        0.85,
                    );
                }
                OrbTint::Down => {
                    // soft greys
                    base_pat.add_color_stop_rgba(0.0, 0.88, 0.90, 0.94, 0.95);
                    base_pat.add_color_stop_rgba(0.40, 0.65, 0.68, 0.74, 0.90);
                    base_pat.add_color_stop_rgba(1.0, 0.35, 0.38, 0.44, 0.85);
                }
            }

            cr.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
            let _ = cr.set_source(&base_pat);
            let _ = cr.fill();

            // Clip inner highlights and blobs to the sphere
            let _ = cr.save();
            cr.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
            cr.clip();

            // 3 overlapping translucent blobs (alpha 0.55) offset around the sphere to fake the swirl
            let offsets = blob_offsets(elapsed_draw.get());
            let blob_colors = match tint_draw.get() {
                OrbTint::Healthy => [series_rgba(1), series_rgba(3), series_rgba(0)],
                OrbTint::Attention => [series_rgba(6), series_rgba(2), series_rgba(1)],
                OrbTint::Down => [
                    gtk::gdk::RGBA::new(0.75, 0.78, 0.82, 1.0),
                    gtk::gdk::RGBA::new(0.55, 0.58, 0.64, 1.0),
                    gtk::gdk::RGBA::new(0.45, 0.48, 0.54, 1.0),
                ],
            };

            let blob_pos = [
                (
                    cx + r * 0.25 + offsets[0].0 * dim,
                    cy - r * 0.15 + offsets[0].1 * dim,
                    r * 0.65,
                ),
                (
                    cx - r * 0.25 + offsets[1].0 * dim,
                    cy + r * 0.22 + offsets[1].1 * dim,
                    r * 0.60,
                ),
                (
                    cx + r * 0.10 + offsets[2].0 * dim,
                    cy + r * 0.30 + offsets[2].1 * dim,
                    r * 0.55,
                ),
            ];

            for (i, &(bx, by, br)) in blob_pos.iter().enumerate() {
                let blob_pat = gtk::cairo::RadialGradient::new(bx, by, 0.0, bx, by, br);
                let col = blob_colors[i];
                let (cr_r, cr_g, cr_b) = (
                    f64::from(col.red()),
                    f64::from(col.green()),
                    f64::from(col.blue()),
                );
                blob_pat.add_color_stop_rgba(0.0, cr_r, cr_g, cr_b, 0.55);
                blob_pat.add_color_stop_rgba(0.7, cr_r, cr_g, cr_b, 0.25);
                blob_pat.add_color_stop_rgba(1.0, cr_r, cr_g, cr_b, 0.0);
                let _ = cr.set_source(&blob_pat);
                cr.arc(bx, by, br, 0.0, std::f64::consts::TAU);
                let _ = cr.fill();
            }

            // Soft white specular highlight at the top-left (radial gradient white alpha 0.85 -> 0)
            let spec_x = cx - r * 0.36;
            let spec_y = cy - r * 0.38;
            let spec_r = r * 0.48;
            let spec_pat =
                gtk::cairo::RadialGradient::new(spec_x, spec_y, r * 0.02, spec_x, spec_y, spec_r);
            spec_pat.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.85);
            spec_pat.add_color_stop_rgba(0.4, 1.0, 1.0, 1.0, 0.35);
            spec_pat.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.0);
            let _ = cr.set_source(&spec_pat);
            cr.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();

            // Faint rim light
            let rim_pat = gtk::cairo::RadialGradient::new(cx, cy, r * 0.82, cx, cy, r);
            rim_pat.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.0);
            rim_pat.add_color_stop_rgba(0.85, 1.0, 1.0, 1.0, 0.10);
            rim_pat.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.35);
            let _ = cr.set_source(&rim_pat);
            cr.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
            let _ = cr.fill();

            let _ = cr.restore(); // restore clip
        });

        // Slow breathing animation (every 60 ms) only when reduce-motion is not set
        let timer_running = Rc::new(Cell::new(false));
        let setup_timer = {
            let area_weak = area.downgrade();
            let elapsed = Rc::clone(&elapsed_secs);
            let running = Rc::clone(&timer_running);

            move || {
                if running.get() {
                    return;
                }
                running.set(true);

                let area_weak = area_weak.clone();
                let elapsed = Rc::clone(&elapsed);
                let running = Rc::clone(&running);

                glib::timeout_add_local(Duration::from_millis(60), move || {
                    let Some(a) = area_weak.upgrade() else {
                        running.set(false);
                        return glib::ControlFlow::Break;
                    };

                    if !a.is_mapped() {
                        running.set(false);
                        return glib::ControlFlow::Break;
                    }

                    // Check if window has reduce-motion
                    let reduce_motion = a.root().is_some_and(|r| r.has_css_class("reduce-motion"));

                    if !reduce_motion {
                        elapsed.set(start_time.elapsed().as_secs_f64());
                        a.queue_draw();
                    }

                    glib::ControlFlow::Continue
                });
            }
        };

        // Connect map / unmap signals to manage timer lifecycle and prevent leaks
        {
            let timer_starter = setup_timer.clone();
            area.connect_map(move |_| {
                timer_starter();
            });
        }
        {
            let running = Rc::clone(&timer_running);
            area.connect_unmap(move |_| {
                running.set(false);
            });
        }

        Self { area, tint }
    }

    /// Returns the underlying DrawingArea.
    #[must_use]
    pub fn widget(&self) -> &gtk::DrawingArea {
        &self.area
    }

    /// Sets the tint colour of the orb and requests a redraw.
    pub fn set_tint(&self, tint: OrbTint) {
        self.tint.set(tint);
        self.area.queue_draw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_donut_arcs_empty_and_zeros() {
        assert!(donut_arcs(&[], 2.0).is_empty());
        let arcs = donut_arcs(&[0.0, 0.0], 2.0);
        assert_eq!(arcs.len(), 2);
        assert_eq!(arcs[0], (0.0, 0.0));
        assert_eq!(arcs[1], (0.0, 0.0));
    }

    #[test]
    fn test_donut_arcs_nan_and_negative() {
        let arcs = donut_arcs(&[-5.0, f64::NAN, 0.0], 2.0);
        assert_eq!(arcs.len(), 3);
        for arc in arcs {
            assert_eq!(arc, (0.0, 0.0));
        }
    }

    #[test]
    fn test_donut_arcs_one_segment() {
        let arcs = donut_arcs(&[100.0], 2.0);
        assert_eq!(arcs.len(), 1);
        let span = arcs[0].1 - arcs[0].0;
        assert!((span - std::f64::consts::TAU).abs() < 1e-9);
    }

    #[test]
    fn test_donut_arcs_sums_to_less_than_or_equal_tau() {
        let values = [10.0, 20.0, 30.0, 40.0];
        let arcs = donut_arcs(&values, 2.0);
        assert_eq!(arcs.len(), 4);
        let sum_spans: f64 = arcs.iter().map(|(s, e)| e - s).sum();
        assert!(sum_spans <= std::f64::consts::TAU);
        assert!(sum_spans > 0.0);
    }

    #[test]
    fn test_bar_rects_empty_and_zero_n() {
        assert!(bar_rects(100.0, 50.0, 0, &[]).is_empty());
        assert!(bar_rects(0.0, 50.0, 4, &[0.5]).is_empty());
        assert!(bar_rects(100.0, 0.0, 4, &[0.5]).is_empty());
    }

    #[test]
    fn test_bar_rects_n_one() {
        let rects = bar_rects(100.0, 50.0, 1, &[0.8]);
        assert_eq!(rects.len(), 1);
        let (x, y, w, h) = rects[0];
        assert!(x >= 0.0);
        assert!(x + w <= 100.0);
        assert!((y + h - 50.0).abs() < 1e-9);
        assert!((h - 40.0).abs() < 1e-9);
        assert!(w <= 28.0);
    }

    #[test]
    fn test_bar_rects_no_overlap_and_inside_bounds() {
        let fractions = [0.1, 0.5, 0.8, 1.0, 0.3];
        let width = 200.0;
        let height = 80.0;
        let rects = bar_rects(width, height, fractions.len(), &fractions);
        assert_eq!(rects.len(), 5);

        for (i, &(x, y, w, h)) in rects.iter().enumerate() {
            assert!(x >= 0.0, "bar {i} x >= 0");
            assert!(x + w <= width + 1e-9, "bar {i} x + w <= width");
            assert!(y >= 0.0, "bar {i} y >= 0");
            assert!(y + h <= height + 1e-9, "bar {i} y + h <= height");

            if i > 0 {
                let (prev_x, _, prev_w, _) = rects[i - 1];
                assert!(
                    prev_x + prev_w <= x + 1e-9,
                    "bar {i} overlaps with bar {}",
                    i - 1
                );
            }
        }
    }

    #[test]
    fn test_bar_rects_width_tiny() {
        let fractions = [0.2, 0.4, 0.6, 0.8];
        let rects = bar_rects(5.0, 20.0, 4, &fractions);
        assert_eq!(rects.len(), 4);
        for (i, &(x, _, w, _)) in rects.iter().enumerate() {
            assert!(x >= 0.0);
            assert!(x + w <= 5.0 + 1e-9);
            if i > 0 {
                let (px, _, pw, _) = rects[i - 1];
                assert!(px + pw <= x + 1e-9);
            }
        }
    }

    #[test]
    fn test_blob_offsets_bounded_and_periodic() {
        let period = std::f64::consts::TAU;
        for step in 0..100 {
            #[allow(clippy::cast_precision_loss)]
            let t = step as f64 * 0.1;
            let offsets = blob_offsets(t);
            for &(dx, dy) in &offsets {
                assert!(dx.abs() <= 0.06, "dx={dx} bounded to 6%");
                assert!(dy.abs() <= 0.06, "dy={dy} bounded to 6%");
            }

            let next_offsets = blob_offsets(t + period);
            for i in 0..3 {
                assert!(
                    (offsets[i].0 - next_offsets[i].0).abs() < 1e-9,
                    "periodic dx"
                );
                assert!(
                    (offsets[i].1 - next_offsets[i].1).abs() < 1e-9,
                    "periodic dy"
                );
            }
        }
    }

    #[test]
    fn test_series_helpers() {
        for i in 0..8 {
            let class = series_css_class(i);
            assert_eq!(class, format!("series-{i}"));
            let rgba = series_rgba(i);
            assert_eq!(rgba.alpha(), 1.0);
        }
        assert_eq!(series_css_class(8), "series-0");
    }
}
