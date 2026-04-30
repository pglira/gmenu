mod config;
mod filter;
mod input;
mod keymap;
mod render;
mod window;

use anyhow::Result;
use clap::Parser;
use std::io::{Read, Write};
use x11rb::protocol::xproto::KeyButMask;
use x11rb::protocol::Event;

use crate::config::Config;
use crate::filter::{Filter, Items};
use crate::input::Input;
use crate::keymap::{Key, Keymap};
use crate::render::Renderer;
use crate::window::Window;

/// Per-invocation flags. Visual styling lives in
/// `$XDG_CONFIG_HOME/gmenu/config.toml` (default `~/.config/gmenu/config.toml`).
#[derive(Parser, Debug)]
#[command(name = "gmenu", about = "A centered dmenu-like launcher")]
struct Cli {
    /// Prompt shown before the input.
    #[arg(short = 'p', long, default_value = "")]
    prompt: String,

    /// Case-sensitive matching (default is case-insensitive).
    #[arg(short = 's', long, default_value_t = false)]
    case_sensitive: bool,

    /// Maximum matches to retain (the visible list only shows ~15 rows).
    #[arg(long, default_value_t = 100_000)]
    max_matches: usize,
}

fn read_stdin_lines() -> Result<Vec<String>> {
    let mut buf = String::new();
    std::io::stdin().lock().read_to_string(&mut buf)?;
    Ok(buf.lines().map(|s| s.to_string()).collect())
}

struct App {
    items: Items,
    filter: Filter,
    input: Input,
    cursor: usize, // index into matches
    scroll: usize,
}

impl App {
    fn new(items: Items, max_matches: usize, case_sensitive: bool) -> Self {
        let mut filter = Filter::new(case_sensitive, max_matches);
        filter.update(&items, "");
        Self { items, filter, input: Input::new(), cursor: 0, scroll: 0 }
    }

    fn refilter(&mut self) {
        self.filter.update(&self.items, &self.input.text);
        if self.cursor >= self.filter.matches.len() {
            self.cursor = self.filter.matches.len().saturating_sub(1);
        }
        self.adjust_scroll(0);
    }

    fn adjust_scroll(&mut self, visible: usize) {
        let visible = visible.max(1);
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + visible {
            self.scroll = self.cursor + 1 - visible;
        }
        let max_scroll = self.filter.matches.len().saturating_sub(visible);
        if self.scroll > max_scroll { self.scroll = max_scroll; }
    }

    fn move_cursor(&mut self, delta: isize, visible: usize) {
        let n = self.filter.matches.len() as isize;
        if n == 0 { self.cursor = 0; return; }
        let mut c = self.cursor as isize + delta;
        if c < 0 { c = 0; }
        if c >= n { c = n - 1; }
        self.cursor = c as usize;
        self.adjust_scroll(visible);
    }

    fn selected(&self) -> Option<&str> {
        self.filter.matches.get(self.cursor).map(|&i| self.items.raw[i as usize].as_str())
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load()?;
    let width = cfg.width();
    let height = cfg.height();
    let theme = cfg.into_theme(cli.prompt.clone())?;

    let raw = read_stdin_lines()?;
    let items = Items::new(raw, cli.case_sensitive);
    let mut app = App::new(items, cli.max_matches, cli.case_sensitive);

    let win = Window::open(width, height, theme.bg.pixel())?;
    let mut renderer = Renderer::new(width as i32, height as i32, &theme);
    let keymap = Keymap::fetch(&win.conn)?;

    let mut needs_repaint = true;

    loop {
        if needs_repaint {
            app.adjust_scroll(renderer.visible_rows);
            renderer.draw(
                &app.items,
                &app.filter,
                &app.input,
                app.cursor,
                app.scroll,
                cli.case_sensitive,
                &theme,
            );
            let pixels = renderer.pixels();
            win.put(&pixels)?;
            win.flush()?;
            needs_repaint = false;
        }

        let event = win.next_event()?;
        match event {
            Event::Expose(_) => { needs_repaint = true; }
            Event::KeyPress(ev) => {
                let state = u16::from(ev.state);
                let ctrl = (state & u16::from(KeyButMask::CONTROL)) != 0;
                let shift = (state & u16::from(KeyButMask::SHIFT)) != 0;
                let key = keymap.lookup(ev.detail, ev.state.into());
                let mut text_changed = false;
                match key {
                    Key::Escape => std::process::exit(1),
                    Key::Return => {
                        let mut out = std::io::stdout().lock();
                        if let Some(sel) = app.selected() {
                            writeln!(out, "{}", sel)?;
                        } else if !app.input.text.is_empty() {
                            writeln!(out, "{}", app.input.text)?;
                        }
                        return Ok(());
                    }
                    Key::Backspace => {
                        app.input.delete_left(ctrl);
                        text_changed = true;
                    }
                    Key::Delete => {
                        app.input.delete_right(ctrl);
                        text_changed = true;
                    }
                    Key::Left => {
                        if ctrl { app.input.move_word_left(shift); }
                        else { app.input.move_left(shift); }
                        needs_repaint = true;
                    }
                    Key::Right => {
                        if ctrl { app.input.move_word_right(shift); }
                        else { app.input.move_right(shift); }
                        needs_repaint = true;
                    }
                    Key::Home => {
                        if ctrl {
                            app.cursor = 0;
                            app.adjust_scroll(renderer.visible_rows);
                        } else {
                            app.input.move_home(shift);
                        }
                        needs_repaint = true;
                    }
                    Key::End => {
                        if ctrl {
                            app.cursor = app.filter.matches.len().saturating_sub(1);
                            app.adjust_scroll(renderer.visible_rows);
                        } else {
                            app.input.move_end(shift);
                        }
                        needs_repaint = true;
                    }
                    Key::Up => { app.move_cursor(-1, renderer.visible_rows); needs_repaint = true; }
                    Key::Down => { app.move_cursor(1, renderer.visible_rows); needs_repaint = true; }
                    Key::Tab => { app.move_cursor(1, renderer.visible_rows); needs_repaint = true; }
                    Key::PageUp => { app.move_cursor(-(renderer.visible_rows as isize), renderer.visible_rows); needs_repaint = true; }
                    Key::PageDown => { app.move_cursor(renderer.visible_rows as isize, renderer.visible_rows); needs_repaint = true; }
                    Key::Insert if shift => {
                        win.request_paste(win.atom_primary, ev.time)?;
                    }
                    Key::Char(c) => {
                        if ctrl {
                            match c.to_ascii_lowercase() {
                                'a' => { app.input.select_all(); needs_repaint = true; }
                                'c' | 'x' => { /* copy/cut not implemented in v1 */ }
                                'v' => { win.request_paste(win.atom_clipboard, ev.time)?; }
                                'l' => { app.input.clear(); text_changed = true; }
                                'u' => {
                                    // Delete from caret to start of line
                                    let caret = app.input.caret;
                                    app.input.text.replace_range(..caret, "");
                                    app.input.caret = 0;
                                    app.input.clear_selection();
                                    text_changed = true;
                                }
                                'w' => { app.input.delete_left(true); text_changed = true; }
                                'k' | 'p' => { app.move_cursor(-1, renderer.visible_rows); needs_repaint = true; }
                                'j' | 'n' => { app.move_cursor(1, renderer.visible_rows); needs_repaint = true; }
                                'e' => { app.input.move_end(shift); needs_repaint = true; }
                                _ => {}
                            }
                        } else if !c.is_control() {
                            app.input.insert_char(c);
                            text_changed = true;
                        }
                    }
                    _ => {}
                }
                if text_changed {
                    app.refilter();
                    needs_repaint = true;
                }
            }
            Event::SelectionNotify(ev) => {
                if ev.property != 0 && ev.property == win.atom_paste_prop {
                    if let Some(text) = win.read_pasted()? {
                        // Strip newlines so multi-line pastes don't break the input
                        let cleaned: String = text.chars().filter(|c| *c != '\n' && *c != '\r').collect();
                        if !cleaned.is_empty() {
                            app.input.insert_str(&cleaned);
                            app.refilter();
                            needs_repaint = true;
                        }
                    }
                }
            }
            Event::FocusOut(_) => {
                std::process::exit(1);
            }
            _ => {}
        }
    }
}
