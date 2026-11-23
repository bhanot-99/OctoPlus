//! `neuroos-octopit --screenshots <dir>`: opens the window, lets real data arrive, then saves a PNG
//! of every page in both themes (the UX acceptance review, docs/design/octopit-ux.md §9). The
//! window is a real one on the user's desktop, so this is a manual or release-gate tool.

use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use gtk4 as gtk;

use crate::app::Window;
use crate::pages::PageId;
use crate::theme::Theme;

/// Pages shown in the review, in order.
const PAGES: [PageId; 6] = [
    PageId::Overview,
    PageId::Chat,
    PageId::Backends,
    PageId::Activity,
    PageId::Privacy,
    PageId::Settings,
];

fn save(win: &gtk::ApplicationWindow, path: &str) -> Result<(), String> {
    let paintable = gtk::WidgetPaintable::new(Some(win));
    let snap = gtk::Snapshot::new();
    let (w, h) = (f64::from(win.width()), f64::from(win.height()));
    gtk::gdk::prelude::PaintableExt::snapshot(&paintable, snap.upcast_ref::<gtk::gdk::Snapshot>(), w, h);
    let node = snap.to_node().ok_or("empty snapshot")?;
    let renderer = win.native().and_then(|n| n.renderer()).ok_or("no renderer")?;
    renderer
        .render_texture(node, None)
        .save_to_png(path)
        .map_err(|e| e.to_string())
}

/// Runs the whole review, then quits the application.
pub fn run(win: &Rc<Window>, dir: String, app: &gtk::Application) {
    let w = Rc::clone(win);
    let app = app.clone();
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("cannot create {dir}: {e}");
        app.quit();
        return;
    }
    let mut steps: Vec<(Theme, PageId)> = Vec::new();
    for theme in [Theme::Dark, Theme::Light] {
        for page in PAGES {
            steps.push((theme, page));
        }
    }
    let steps = Rc::new(std::cell::RefCell::new(steps.into_iter()));
    // Give the first poll time to arrive, then one shot every 700 ms.
    glib::timeout_add_local(Duration::from_millis(2500), move || {
        let Some((theme, page)) = steps.borrow_mut().next() else {
            app.quit();
            return glib::ControlFlow::Break;
        };
        w.set_theme(theme);
        w.show_page(page);
        let (w2, dir) = (Rc::clone(&w), dir.clone());
        glib::timeout_add_local_once(Duration::from_millis(450), move || {
            let name = format!(
                "{dir}/{}-{}.png",
                if theme == Theme::Dark { "dark" } else { "light" },
                page.key()
            );
            match save(&w2.window, &name) {
                Ok(()) => println!("saved {name}"),
                Err(e) => eprintln!("screenshot {name}: {e}"),
            }
        });
        glib::ControlFlow::Continue
    });
}
