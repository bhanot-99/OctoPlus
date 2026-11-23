//! The Command Palette dialog (Ctrl+K, docs/design/octopit-ux.md §2).

#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk4 as gtk;

use crate::widgets::label;

/// A command item displayed in the command palette.
#[derive(Clone)]
pub struct Command {
    /// Title of the command shown in the palette.
    pub title: String,
    /// Keyboard shortcut or hint (e.g. "Ctrl+2"), right-aligned in caption style.
    pub hint: String,
    /// Closure to run when the command is activated.
    pub run: Rc<dyn Fn()>,
}

/// Filters command titles against `query`.
///
/// Matches are case-insensitive. Every whitespace-separated query word must appear
/// either as a prefix of a word in the title or as a substring anywhere in the title.
/// All-prefix matches are ranked first, followed by substring matches.
/// Order within each rank is stable. Empty or whitespace-only queries return all indices in order.
#[must_use]
pub fn filter(commands_titles: &[String], query: &str) -> Vec<usize> {
    let q_words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();
    if q_words.is_empty() {
        return (0..commands_titles.len()).collect();
    }

    let mut prefix_matches = Vec::new();
    let mut substring_matches = Vec::new();

    for (idx, title) in commands_titles.iter().enumerate() {
        let t_lower = title.to_lowercase();
        let t_words: Vec<&str> = t_lower.split_whitespace().collect();

        let mut matches_all_words = true;
        let mut all_prefix = true;

        for qw in &q_words {
            let is_prefix = t_words.iter().any(|tw| tw.starts_with(qw));
            if is_prefix {
                continue;
            }
            let is_sub = t_lower.contains(qw);
            if is_sub {
                all_prefix = false;
            } else {
                matches_all_words = false;
                break;
            }
        }

        if matches_all_words {
            if all_prefix {
                prefix_matches.push(idx);
            } else {
                substring_matches.push(idx);
            }
        }
    }

    prefix_matches.extend(substring_matches);
    prefix_matches
}

/// The Command Palette modal window.
pub struct Palette {
    window: gtk::Window,
    entry: gtk::Entry,
    commands: Rc<Vec<Command>>,
    titles: Rc<Vec<String>>,
    list: gtk::ListBox,
    filtered_indices: Rc<RefCell<Vec<usize>>>,
}

impl Palette {
    /// Creates a new Command Palette transient for `parent`.
    #[must_use]
    pub fn new(parent: &gtk::ApplicationWindow, commands: Vec<Command>) -> Self {
        let window = gtk::Window::new();
        window.add_css_class("octopit");
        window.set_transient_for(Some(parent));
        window.set_modal(true);
        window.set_title(Some("Command palette"));
        window.set_default_size(520, 360);
        window.set_resizable(false);

        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.add_css_class("palette-window");

        let entry = gtk::Entry::new();
        entry.set_placeholder_text(Some("Type a command\u{2026}"));
        entry.update_property(&[gtk::accessible::Property::Label("Search commands")]);
        entry.add_css_class("palette-search");
        root.append(&entry);

        let list = gtk::ListBox::new();
        list.add_css_class("palette-list");
        list.set_selection_mode(gtk::SelectionMode::Single);

        let scroll = gtk::ScrolledWindow::new();
        scroll.set_child(Some(&list));
        scroll.set_vexpand(true);
        scroll.set_hscrollbar_policy(gtk::PolicyType::Never);
        root.append(&scroll);

        window.set_child(Some(&root));

        let titles: Vec<String> = commands.iter().map(|c| c.title.clone()).collect();
        let commands = Rc::new(commands);
        let titles = Rc::new(titles);
        let filtered_indices = Rc::new(RefCell::new(Vec::new()));

        let rebuild_list = {
            let list = list.clone();
            let commands = Rc::clone(&commands);
            let titles = Rc::clone(&titles);
            let filtered_indices = Rc::clone(&filtered_indices);
            move |query: &str| {
                let indices = filter(&titles, query);
                *filtered_indices.borrow_mut() = indices.clone();

                while let Some(ch) = list.first_child() {
                    list.remove(&ch);
                }

                for &idx in &indices {
                    if let Some(cmd) = commands.get(idx) {
                        let row = gtk::ListBoxRow::new();
                        let row_box = gtk::Box::new(gtk::Orientation::Horizontal, 12);
                        row_box.add_css_class("palette-row");

                        let title_lbl = label(&cmd.title, &["body"]);
                        title_lbl.set_hexpand(true);
                        row_box.append(&title_lbl);

                        if !cmd.hint.is_empty() {
                            let hint_lbl = label(&cmd.hint, &["caption", "muted", "num"]);
                            hint_lbl.set_halign(gtk::Align::End);
                            row_box.append(&hint_lbl);
                        }

                        row.set_child(Some(&row_box));
                        list.append(&row);
                    }
                }

                if let Some(first_row) = list.row_at_index(0) {
                    list.select_row(Some(&first_row));
                }
            }
        };

        // Initial population
        rebuild_list("");

        // Run selected command closure
        let run_selected = {
            let win = window.clone();
            let list = list.clone();
            let commands = Rc::clone(&commands);
            let filtered_indices = Rc::clone(&filtered_indices);
            move || {
                let selected_idx = list.selected_row().map(|r| r.index());
                if let Some(row_idx) = selected_idx {
                    #[allow(clippy::cast_sign_loss)]
                    let row_usize = row_idx as usize;
                    let indices = filtered_indices.borrow();
                    if let Some(cmd) = indices.get(row_usize).and_then(|&i| commands.get(i)) {
                        win.set_visible(false);
                        (cmd.run)();
                    }
                }
            }
        };

        // Row activation (double click or Enter on row)
        {
            let run_sel = run_selected.clone();
            list.connect_row_activated(move |_, _| {
                run_sel();
            });
        }

        // Query change handling
        {
            let rebuild = rebuild_list;
            entry.connect_changed(move |ent| {
                rebuild(&ent.text());
            });
        }

        // Keyboard navigation and shortcuts
        {
            let key_ctrl = gtk::EventControllerKey::new();
            let win = window.clone();
            let list = list.clone();
            let filtered_indices = Rc::clone(&filtered_indices);
            let run_sel = run_selected;

            key_ctrl.connect_key_pressed(move |_, keyval, _, _| match keyval {
                gtk::gdk::Key::Escape => {
                    win.set_visible(false);
                    gtk::glib::Propagation::Stop
                }
                gtk::gdk::Key::Return | gtk::gdk::Key::KP_Enter => {
                    run_sel();
                    gtk::glib::Propagation::Stop
                }
                gtk::gdk::Key::Up => {
                    let count = filtered_indices.borrow().len();
                    if count > 0 {
                        let cur = list.selected_row().map_or(0, |r| r.index());
                        let next = (cur - 1).max(0);
                        if let Some(row) = list.row_at_index(next) {
                            list.select_row(Some(&row));
                        }
                    }
                    gtk::glib::Propagation::Stop
                }
                gtk::gdk::Key::Down => {
                    let count = filtered_indices.borrow().len();
                    if count > 0 {
                        #[allow(clippy::cast_possible_wrap)]
                        let max_idx = (count as i32) - 1;
                        let cur = list.selected_row().map_or(-1, |r| r.index());
                        let next = (cur + 1).min(max_idx);
                        if let Some(row) = list.row_at_index(next) {
                            list.select_row(Some(&row));
                        }
                    }
                    gtk::glib::Propagation::Stop
                }
                _ => gtk::glib::Propagation::Proceed,
            });
            window.add_controller(key_ctrl);
        }

        Self {
            window,
            entry,
            commands,
            titles,
            list,
            filtered_indices,
        }
    }

    /// Shows the palette, clearing and focusing the search entry.
    pub fn show(&self) {
        self.entry.set_text("");
        self.window.present();
        self.entry.grab_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_empty() {
        let titles = vec![
            "Overview".to_owned(),
            "Chat".to_owned(),
            "Settings".to_owned(),
        ];
        assert_eq!(filter(&titles, ""), vec![0, 1, 2]);
        assert_eq!(filter(&titles, "   "), vec![0, 1, 2]);
    }

    #[test]
    fn test_filter_prefix_ranking() {
        let titles = vec![
            "Developers".to_owned(),
            "Open Chat".to_owned(),
            "Options".to_owned(),
        ];
        // "op" is a word prefix of "Open Chat" and "Options", and a substring of "Developers"
        let res = filter(&titles, "op");
        assert_eq!(res, vec![1, 2, 0]);
    }

    #[test]
    fn test_filter_substring() {
        let titles = vec![
            "Overview".to_owned(),
            "Backends".to_owned(),
            "Activity".to_owned(),
        ];
        assert_eq!(filter(&titles, "view"), vec![0]);
        assert_eq!(filter(&titles, "ken"), vec![1]);
        assert_eq!(filter(&titles, "tiv"), vec![2]);
    }

    #[test]
    fn test_filter_multi_word() {
        let titles = vec![
            "Go to Chat".to_owned(),
            "Chat Overview".to_owned(),
            "Go to Backends".to_owned(),
        ];
        assert_eq!(filter(&titles, "chat go"), vec![0]);
        assert_eq!(filter(&titles, "to backends"), vec![2]);
    }

    #[test]
    fn test_filter_case_insensitivity() {
        let titles = vec!["Overview".to_owned(), "CHAT".to_owned()];
        assert_eq!(filter(&titles, "chat"), vec![1]);
        assert_eq!(filter(&titles, "CHAT"), vec![1]);
        assert_eq!(filter(&titles, "OVERVIEW"), vec![0]);
    }

    #[test]
    fn test_filter_unicode() {
        let titles = vec![
            "\u{00C9}cran".to_owned(),
            "System".to_owned(),
            "नमस्ते दुनिया".to_owned(),
        ];
        // Exact Unicode match does not panic
        assert_eq!(filter(&titles, "नमस्ते"), vec![2]);
        assert_eq!(filter(&titles, "दुनिया"), vec![2]);
        // \u{00C9}cran vs ecran is not required to match
        assert_eq!(filter(&titles, "ecran"), Vec::<usize>::new());
        assert_eq!(filter(&titles, "\u{00E9}cran"), vec![0]);
    }

    #[test]
    fn test_filter_query_longer_than_title() {
        let titles = vec!["Chat".to_owned()];
        assert_eq!(
            filter(&titles, "chat and extra words longer than title"),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn test_filter_many_commands() {
        let titles: Vec<String> = (0..100).map(|i| format!("Command number {i}")).collect();
        let res = filter(&titles, "number 42");
        assert_eq!(res, vec![42]);
    }
}
