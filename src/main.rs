mod config;
mod filter;
mod keymap;
mod render;
mod window;

use anyhow::Result;
use clap::Parser;
use std::io::{Read, Write};
use x11rb::protocol::Event;

use crate::config::Config;
use crate::filter::{Filter, Items};
use crate::keymap::{Key, Keymap};
use crate::render::{Renderer};
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
    let mut out: Vec<String> = buf.lines().map(|s| s.to_string()).collect();
    // Drop trailing empty line if input ended with \n (already handled by .lines())
    let _ = &mut out;
    Ok(out)
}

struct App {
    items: Items,
    filter: Filter,
    query: String,
    cursor: usize, // index into matches
    scroll: usize,
}

impl App {
    fn new(items: Items, max_matches: usize, case_sensitive: bool) -> Self {
        let mut filter = Filter::new(case_sensitive, max_matches);
        filter.update(&items, "");
        Self { items, filter, query: String::new(), cursor: 0, scroll: 0 }
    }

    fn refilter(&mut self) {
        self.filter.update(&self.items, &self.query);
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
        // Clamp
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
                &app.query,
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
                let key = keymap.lookup(ev.detail, ev.state.into());
                let ctrl = (u16::from(ev.state) & u16::from(x11rb::protocol::xproto::KeyButMask::CONTROL)) != 0;
                match key {
                    Key::Escape => {
                        std::process::exit(1);
                    }
                    Key::Return => {
                        if let Some(sel) = app.selected() {
                            let mut out = std::io::stdout().lock();
                            writeln!(out, "{}", sel)?;
                        } else if !app.query.is_empty() {
                            // No match: print the query (dmenu-ish behavior)
                            let mut out = std::io::stdout().lock();
                            writeln!(out, "{}", app.query)?;
                        }
                        return Ok(());
                    }
                    Key::Backspace => {
                        if ctrl {
                            // Word delete
                            while app.query.pop().is_some_and(|c| c.is_whitespace()) {}
                            while let Some(c) = app.query.chars().last() {
                                if c.is_whitespace() { break; }
                                app.query.pop();
                            }
                        } else {
                            app.query.pop();
                        }
                        app.refilter();
                        needs_repaint = true;
                    }
                    Key::Up => { app.move_cursor(-1, renderer.visible_rows); needs_repaint = true; }
                    Key::Down => { app.move_cursor(1, renderer.visible_rows); needs_repaint = true; }
                    Key::Tab => { app.move_cursor(1, renderer.visible_rows); needs_repaint = true; }
                    Key::PageUp => { app.move_cursor(-(renderer.visible_rows as isize), renderer.visible_rows); needs_repaint = true; }
                    Key::PageDown => { app.move_cursor(renderer.visible_rows as isize, renderer.visible_rows); needs_repaint = true; }
                    Key::Home => { app.cursor = 0; app.adjust_scroll(renderer.visible_rows); needs_repaint = true; }
                    Key::End => {
                        let n = app.filter.matches.len();
                        app.cursor = n.saturating_sub(1);
                        app.adjust_scroll(renderer.visible_rows);
                        needs_repaint = true;
                    }
                    Key::Char(c) => {
                        if ctrl {
                            match c.to_ascii_lowercase() {
                                'l' => { app.query.clear(); app.refilter(); needs_repaint = true; }
                                'w' => {
                                    while app.query.pop().is_some_and(|x| x.is_whitespace()) {}
                                    while let Some(c) = app.query.chars().last() {
                                        if c.is_whitespace() { break; }
                                        app.query.pop();
                                    }
                                    app.refilter(); needs_repaint = true;
                                }
                                'k' | 'p' => { app.move_cursor(-1, renderer.visible_rows); needs_repaint = true; }
                                'j' | 'n' => { app.move_cursor(1, renderer.visible_rows); needs_repaint = true; }
                                _ => {}
                            }
                        } else if !c.is_control() {
                            app.query.push(c);
                            app.refilter();
                            needs_repaint = true;
                        }
                    }
                    _ => {}
                }
            }
            Event::FocusOut(_) => {
                // Lost focus to another window: exit, mimicking dmenu.
                std::process::exit(1);
            }
            _ => {}
        }
    }
}

