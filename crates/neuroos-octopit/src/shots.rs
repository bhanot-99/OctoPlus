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

fn save(win: &impl IsA<gtk::Widget>, path: &str) -> Result<(), String> {
    let paintable = gtk::WidgetPaintable::new(Some(win));
    let snap = gtk::Snapshot::new();
    let (w, h) = (f64::from(win.width()), f64::from(win.height()));
    gtk::gdk::prelude::PaintableExt::snapshot(
        &paintable,
        snap.upcast_ref::<gtk::gdk::Snapshot>(),
        w,
        h,
    );
    let node = snap.to_node().ok_or("empty snapshot")?;
    let renderer = win
        .native()
        .and_then(|n| n.renderer())
        .ok_or("no renderer")?;
    renderer
        .render_texture(node, None)
        .save_to_png(path)
        .map_err(|e| e.to_string())
}

type Steps = Rc<std::cell::RefCell<std::vec::IntoIter<(Theme, PageId)>>>;

/// Runs the whole review, then quits the application.
pub fn run(win: &Rc<Window>, dir: String, app: &gtk::Application) {
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
    let steps: Steps = Rc::new(std::cell::RefCell::new(steps.into_iter()));
    let (w, app) = (Rc::clone(win), app.clone());
    // Give the first poll time to arrive, then one page at a time.
    glib::timeout_add_local_once(Duration::from_millis(2500), move || {
        next(w, steps, dir, app)
    });
}

fn next(w: Rc<Window>, steps: Steps, dir: String, app: gtk::Application) {
    let step = steps.borrow_mut().next();
    let Some((theme, page)) = step else {
        extras(w, dir, app);
        return;
    };
    w.set_theme(theme);
    w.show_page(page);
    // One real question through the whole stack, so the Chat shot shows a real answer.
    let wait = if page == PageId::Chat && theme == Theme::Dark {
        w.ask("What is two plus two? Answer in one short sentence.");
        25_000
    } else {
        1500
    };
    glib::timeout_add_local_once(Duration::from_millis(wait), move || {
        let name = format!(
            "{dir}/{}-{}.png",
            if theme == Theme::Dark {
                "dark"
            } else {
                "light"
            },
            page.key()
        );
        match save(&w.window, &name) {
            Ok(()) => println!("saved {name}"),
            Err(e) => eprintln!("screenshot {name}: {e}"),
        }
        // A page taller than the window gets a second shot of its end.
        if w.scroll_to_end() {
            let end_name = name.replace(".png", "-end.png");
            glib::timeout_add_local_once(Duration::from_millis(500), move || {
                match save(&w.window, &end_name) {
                    Ok(()) => println!("saved {end_name}"),
                    Err(e) => eprintln!("screenshot {end_name}: {e}"),
                }
                next(w, steps, dir, app);
            });
        } else {
            next(w, steps, dir, app);
        }
    });
}

/// The palette and the welcome window are separate windows: save each one on its own.
fn extras(w: Rc<Window>, dir: String, app: gtk::Application) {
    w.set_theme(Theme::Dark);
    w.show_page(PageId::Overview);
    w.show_palette();
    glib::timeout_add_local_once(Duration::from_millis(800), move || {
        save_other_windows(&w, &format!("{dir}/palette.png"));
        for t in gtk::Window::list_toplevels() {
            if let Some(win) = t.downcast_ref::<gtk::Window>()
                && win.title().as_deref() == Some("Command palette")
            {
                win.close();
            }
        }
        crate::welcome::show(&w.window, Box::new(|| {}));
        glib::timeout_add_local_once(Duration::from_millis(800), move || {
            save_other_windows(&w, &format!("{dir}/welcome.png"));
            app.quit();
        });
    });
}

fn save_other_windows(main: &Rc<Window>, name: &str) {
    for t in gtk::Window::list_toplevels() {
        let Some(win) = t.downcast_ref::<gtk::Window>() else {
            continue;
        };
        if win == main.window.upcast_ref::<gtk::Window>() || !win.is_visible() {
            continue;
        }
        match save(win, name) {
            Ok(()) => println!("saved {name}"),
            Err(e) => eprintln!("screenshot {name}: {e}"),
        }
    }
}
