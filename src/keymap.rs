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
        let per = self.keysyms_per_keycode as usize;
        if per == 0 || keycode < self.min_keycode { return Key::Other; }
        let start = (keycode - self.min_keycode) as usize * per;
        let row = match self.table.get(start..start + per) {
            Some(r) => r,
            None => return Key::Other,
        };

        // ISO_Level3_Shift (AltGr) is conventionally bound to Mod5 on X11.
        let shift = (state & u16::from(KeyButMask::SHIFT)) != 0;
        let lock = (state & u16::from(KeyButMask::LOCK)) != 0;
        let altgr = (state & u16::from(KeyButMask::MOD5)) != 0;
        let is_letter = (0x61..=0x7a).contains(&row[0]); // lowercase ascii at base
        let effective_shift = shift ^ (lock && is_letter);

        // XKB on X11 typically reports keysyms_per_keycode of 6+ with level 3
        // (AltGr) at cols 4/5; minimal mappings pack it at 2/3.
        let altgr_base = if per >= 6 { 4 } else { 2 };
        let col = match (altgr, effective_shift) {
            (false, false) => 0,
            (false, true)  => 1,
            (true,  false) => altgr_base,
            (true,  true)  => altgr_base + 1,
        };
        let sym = row.get(col).copied().filter(|&s| s != 0)
            .or_else(|| if effective_shift { row.get(1).copied().filter(|&s| s != 0) } else { None })
            .unwrap_or(row[0]);
        keysym_to_key(sym)
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
