//! Minimal keycode -> keysym -> char/key decoder using the X11 core
//! GetKeyboardMapping reply. Sufficient for dmenu-style use: ASCII +
//! named control keys. Non-ASCII Unicode keysyms are also decoded.

use anyhow::Result;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, KeyButMask, ModMask};

pub struct Keymap {
    min_keycode: u8,
    keysyms_per_keycode: u8,
    table: Vec<u32>,
}

#[derive(Debug, Clone)]
pub enum Key {
    Char(char),
    Return,
    Escape,
    Backspace,
    Delete,
    Tab,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Other,
}

impl Keymap {
    pub fn fetch<C: Connection>(conn: &C) -> Result<Self> {
        let setup = conn.setup();
        let min = setup.min_keycode;
        let max = setup.max_keycode;
        let count = max - min + 1;
        let r = conn.get_keyboard_mapping(min, count)?.reply()?;
        Ok(Self {
            min_keycode: min,
            keysyms_per_keycode: r.keysyms_per_keycode,
            table: r.keysyms,
        })
    }

    pub fn lookup(&self, keycode: u8, state: u16) -> Key {
        if keycode < self.min_keycode { return Key::Other; }
        let idx = (keycode - self.min_keycode) as usize * self.keysyms_per_keycode as usize;
        if idx >= self.table.len() { return Key::Other; }
        let group_size = self.keysyms_per_keycode.min(4) as usize;
        // Column 0 = unshifted, 1 = shifted (group 1)
        let shift = (state & u16::from(KeyButMask::SHIFT)) != 0;
        let lock = (state & u16::from(KeyButMask::LOCK)) != 0;
        let col = if shift ^ (lock && self.is_letter_keycode(keycode)) { 1 } else { 0 };
        let col = col.min(group_size.saturating_sub(1));
        let sym = self.table[idx + col];
        let sym0 = self.table[idx];
        let sym = if sym == 0 { sym0 } else { sym };
        keysym_to_key(sym)
    }

    fn is_letter_keycode(&self, keycode: u8) -> bool {
        let idx = (keycode - self.min_keycode) as usize * self.keysyms_per_keycode as usize;
        if idx >= self.table.len() { return false; }
        let s = self.table[idx];
        (0x61..=0x7a).contains(&s) // lowercase ascii unshifted
    }
}

fn keysym_to_key(sym: u32) -> Key {
    match sym {
        0xff0d | 0xff8d => Key::Return,         // Return, KP_Enter
        0xff1b => Key::Escape,
        0xff08 => Key::Backspace,
        0xffff => Key::Delete,
        0xff09 => Key::Tab,
        0xff52 => Key::Up,
        0xff54 => Key::Down,
        0xff51 => Key::Left,
        0xff53 => Key::Right,
        0xff50 => Key::Home,
        0xff57 => Key::End,
        0xff55 => Key::PageUp,
        0xff56 => Key::PageDown,
        0xff63 => Key::Insert,
        s if (0x20..0x7f).contains(&s) => Key::Char(char::from(s as u8)),
        s if (0xa0..0x100).contains(&s) => Key::Char(char::from(s as u8)),
        // Direct Unicode encoding: keysym 0x01000000 + ucs codepoint
        s if (0x01000100..=0x0110ffff).contains(&s) => {
            char::from_u32(s - 0x01000000).map(Key::Char).unwrap_or(Key::Other)
        }
        _ => Key::Other,
    }
}

// Compatibility: x11rb 0.13 KeyButMask is a bitflags-like newtype; convert via u16.
#[allow(dead_code)]
fn _bw_check(_m: ModMask) {}
