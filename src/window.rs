use anyhow::{anyhow, Context, Result};
use std::borrow::Cow;
use x11rb::connection::Connection;
use x11rb::image::{BitsPerPixel, Image, ImageOrder as XImageOrder, ScanlinePad};
use x11rb::protocol::randr::ConnectionExt as _;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::COPY_DEPTH_FROM_PARENT;

pub struct Geometry {
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
}

pub struct Window {
    pub conn: RustConnection,
    #[allow(dead_code)]
    pub screen_num: usize,
    pub win: u32,
    pub gc: u32,
    pub w: u16,
    pub h: u16,
    pub depth: u8,
    pub byte_order: XImageOrder,
    pub atom_clipboard: u32,
    pub atom_primary: u32,
    pub atom_utf8: u32,
    pub atom_paste_prop: u32,
}

impl Window {
    pub fn open(width: u16, height: u16, bg_pixel: u32, opacity: f64) -> Result<Self> {
        let (conn, screen_num) = RustConnection::connect(None).context("X11 connect")?;
        let setup = conn.setup().clone();
        let screen = &setup.roots[screen_num];
        let root = screen.root;
        let depth = screen.root_depth;

        let mon = monitor_under_pointer(&conn, root)?
            .unwrap_or(Geometry { x: 0, y: 0, w: screen.width_in_pixels, h: screen.height_in_pixels });

        let x = mon.x + ((mon.w as i32 - width as i32) / 2) as i16;
        let y = mon.y + ((mon.h as i32 - height as i32) / 2) as i16;

        let win = conn.generate_id()?;
        let aux = CreateWindowAux::new()
            .background_pixel(bg_pixel)
            .border_pixel(bg_pixel)
            .override_redirect(1)
            .event_mask(
                EventMask::EXPOSURE
                    | EventMask::KEY_PRESS
                    | EventMask::KEY_RELEASE
                    | EventMask::STRUCTURE_NOTIFY
                    | EventMask::FOCUS_CHANGE,
            );
        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            win,
            root,
            x, y, width, height,
            0,
            WindowClass::INPUT_OUTPUT,
            screen.root_visual,
            &aux,
        )?;

        // Hint to compositors that we're a popup.
        set_window_type_dialog(&conn, win)?;
        set_wm_class(&conn, win, "gmenu", "gmenu")?;
        set_wm_name(&conn, win, "gmenu")?;
        set_window_opacity(&conn, win, opacity)?;

        let gc = conn.generate_id()?;
        conn.create_gc(gc, win, &CreateGCAux::new().graphics_exposures(0))?;

        conn.map_window(win)?;
        conn.flush()?;

        // Try to grab the keyboard. If another client has it, retry briefly.
        let mut grabbed = false;
        for _ in 0..50 {
            let r = conn.grab_keyboard(false, win, x11rb::CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC)?.reply()?;
            if r.status == GrabStatus::SUCCESS {
                grabbed = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if !grabbed {
            return Err(anyhow!("could not grab keyboard"));
        }

        let atom_clipboard = conn.intern_atom(false, b"CLIPBOARD")?.reply()?.atom;
        let atom_utf8 = conn.intern_atom(false, b"UTF8_STRING")?.reply()?.atom;
        let atom_paste_prop = conn.intern_atom(false, b"GMENU_PASTE")?.reply()?.atom;

        Ok(Self {
            conn,
            screen_num,
            win,
            gc,
            w: width,
            h: height,
            depth,
            byte_order: setup.image_byte_order.try_into().unwrap_or(XImageOrder::LsbFirst),
            atom_clipboard,
            atom_primary: u32::from(AtomEnum::PRIMARY),
            atom_utf8,
            atom_paste_prop,
        })
    }

    /// Asynchronously request the contents of an X11 selection. The result
    /// arrives later as a `SelectionNotify` event; call [`Self::read_pasted`]
    /// when that event matches our paste property.
    pub fn request_paste(&self, selection: u32, time: u32) -> Result<()> {
        self.conn.convert_selection(
            self.win,
            selection,
            self.atom_utf8,
            self.atom_paste_prop,
            time,
        )?;
        self.conn.flush()?;
        Ok(())
    }

    /// Read the property where pasted bytes were stored and delete it.
    /// Returns `None` if the selection owner declined or the bytes aren't UTF-8.
    pub fn read_pasted(&self) -> Result<Option<String>> {
        let r = self.conn.get_property(
            true, // delete after read
            self.win,
            self.atom_paste_prop,
            u32::from(AtomEnum::ANY),
            0,
            u32::MAX / 4,
        )?.reply()?;
        if r.value_len == 0 || r.type_ == 0 {
            return Ok(None);
        }
        Ok(String::from_utf8(r.value).ok())
    }

    /// Send the given pre-rendered ARGB/Rgb24 buffer (Cairo `Format::Rgb24`,
    /// 32 bits per pixel, native byte order) to the window.
    pub fn put(&self, buf: &[u8]) -> Result<()> {
        let img = Image::new(
            self.w,
            self.h,
            ScanlinePad::Pad32,
            self.depth,
            BitsPerPixel::B32,
            self.byte_order,
            Cow::Borrowed(buf),
        ).map_err(|e| anyhow!("Image::new: {e:?}"))?;
        let cookies = img.put(&self.conn, self.win, self.gc, 0, 0)?;
        for c in cookies {
            c.check()?;
        }
        Ok(())
    }

    pub fn next_event(&self) -> Result<Event> {
        Ok(self.conn.wait_for_event()?)
    }

    pub fn flush(&self) -> Result<()> {
        self.conn.flush()?;
        Ok(())
    }
}

fn monitor_under_pointer(conn: &RustConnection, root: u32) -> Result<Option<Geometry>> {
    // Try RandR monitors first.
    if let Ok(reply) = conn.randr_get_monitors(root, true)?.reply() {
        if !reply.monitors.is_empty() {
            // Pointer position
            let pq = conn.query_pointer(root)?.reply()?;
            for m in &reply.monitors {
                let inside = pq.root_x >= m.x
                    && pq.root_x < m.x + m.width as i16
                    && pq.root_y >= m.y
                    && pq.root_y < m.y + m.height as i16;
                if inside {
                    return Ok(Some(Geometry { x: m.x, y: m.y, w: m.width, h: m.height }));
                }
            }
            // Fallback to primary or first monitor
            let primary = reply.monitors.iter().find(|m| m.primary).unwrap_or(&reply.monitors[0]);
            return Ok(Some(Geometry { x: primary.x, y: primary.y, w: primary.width, h: primary.height }));
        }
    }
    Ok(None)
}

fn set_wm_name(conn: &RustConnection, win: u32, name: &str) -> Result<()> {
    conn.change_property8(
        PropMode::REPLACE,
        win,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        name.as_bytes(),
    )?;
    Ok(())
}

fn set_wm_class(conn: &RustConnection, win: u32, instance: &str, class: &str) -> Result<()> {
    let mut data: Vec<u8> = Vec::with_capacity(instance.len() + class.len() + 2);
    data.extend_from_slice(instance.as_bytes());
    data.push(0);
    data.extend_from_slice(class.as_bytes());
    data.push(0);
    conn.change_property8(
        PropMode::REPLACE,
        win,
        AtomEnum::WM_CLASS,
        AtomEnum::STRING,
        &data,
    )?;
    Ok(())
}

fn set_window_opacity(conn: &RustConnection, win: u32, opacity: f64) -> Result<()> {
    // _NET_WM_WINDOW_OPACITY: a 32-bit value where 0xFFFFFFFF means fully
    // opaque and 0 means fully transparent. Requires a running compositor.
    let opacity = opacity.clamp(0.0, 1.0);
    let value = (opacity * u32::MAX as f64).round() as u32;
    let atom = conn.intern_atom(false, b"_NET_WM_WINDOW_OPACITY")?.reply()?.atom;
    conn.change_property32(
        PropMode::REPLACE,
        win,
        atom,
        AtomEnum::CARDINAL,
        &[value],
    )?;
    Ok(())
}

fn set_window_type_dialog(conn: &RustConnection, win: u32) -> Result<()> {
    let net_wm_window_type = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE")?.reply()?.atom;
    let net_wm_window_type_dialog = conn.intern_atom(false, b"_NET_WM_WINDOW_TYPE_DIALOG")?.reply()?.atom;
    conn.change_property32(
        PropMode::REPLACE,
        win,
        net_wm_window_type,
        AtomEnum::ATOM,
        &[net_wm_window_type_dialog],
    )?;
    Ok(())
}
