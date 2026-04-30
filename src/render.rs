use crate::filter::{match_spans, Filter, Items};
use cairo::{Context, Format, ImageSurface};
use pango::{EllipsizeMode, FontDescription, Layout};

#[derive(Clone, Copy)]
pub struct Color(pub f64, pub f64, pub f64);

impl Color {
    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.trim_start_matches('#');
        if s.len() != 6 { return None; }
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        Some(Color(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0))
    }
    pub fn pixel(&self) -> u32 {
        let r = (self.0 * 255.0).round() as u32;
        let g = (self.1 * 255.0).round() as u32;
        let b = (self.2 * 255.0).round() as u32;
        (r << 16) | (g << 8) | b
    }
}

pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub selbg: Color,
    pub selfg: Color,
    pub matchbg: Color,
    pub matchfg: Color,
    pub border: Color,
    pub border_width: i32,
    pub font: String,
    pub prompt: String,
    pub padding_x: i32, // pixels
    pub padding_y: i32,
    pub row_padding_y: i32, // extra vertical space added to each row
}

pub struct Renderer {
    surface: ImageSurface,
    pub w: i32,
    pub h: i32,
    pub text_height: i32,
    pub row_height: i32,
    pub visible_rows: usize,
    fd: FontDescription,
}

impl Renderer {
    pub fn new(w: i32, h: i32, theme: &Theme) -> Self {
        let surface = ImageSurface::create(Format::Rgb24, w, h).expect("cairo surface");
        let ctx = Context::new(&surface).expect("cairo ctx");
        let fd = FontDescription::from_string(&theme.font);
        let layout = pangocairo::functions::create_layout(&ctx);
        layout.set_font_description(Some(&fd));
        layout.set_text("Mg");
        let (_, lh_pu) = layout.size();
        let text_height = (lh_pu / pango::SCALE).max(12);
        let row_height = text_height + theme.row_padding_y.max(0);
        let bw = theme.border_width.max(0);
        let usable_h = h - row_height - theme.padding_y * 2 - bw * 2;
        let visible_rows = (usable_h / row_height).max(1) as usize;
        drop(ctx);
        Self { surface, w, h, text_height, row_height, visible_rows, fd }
    }

    pub fn pixels(&mut self) -> Vec<u8> {
        self.surface.flush();
        let data = self.surface.data().expect("cairo lock");
        data.to_vec()
    }

    pub fn draw(
        &self,
        items: &Items,
        filter: &Filter,
        query: &str,
        cursor: usize,
        scroll: usize,
        case_sensitive: bool,
        theme: &Theme,
    ) {
        let ctx = Context::new(&self.surface).expect("cairo ctx");
        let ctx = &ctx;
        // bg
        set_color(ctx, theme.bg);
        ctx.paint().unwrap();

        let bw = theme.border_width.max(0) as f64;
        let pad_x = theme.padding_x as f64 + bw;
        let pad_y = theme.padding_y as f64 + bw;

        // Vertical centering offset within a row
        let text_y_off = ((self.row_height - self.text_height) / 2).max(0) as f64;

        // Input row
        let input_text = format!("{}{}", theme.prompt, query);
        let input_layout = self.layout_for(ctx, &input_text);
        set_color(ctx, theme.fg);
        ctx.move_to(pad_x, pad_y + text_y_off);
        pangocairo::functions::show_layout(ctx, &input_layout);

        // Counter (right side): "n/total"
        let total = items.len();
        let shown = filter.matches.len();
        let counter = if shown >= total {
            format!("{shown}")
        } else {
            format!("{shown}/{total}")
        };
        let cl = self.layout_for(ctx, &counter);
        let (cw_pu, _) = cl.size();
        let cw = cw_pu / pango::SCALE;
        ctx.move_to((self.w as f64) - pad_x - cw as f64, pad_y + text_y_off);
        pangocairo::functions::show_layout(ctx, &cl);

        // Separator under input
        let sep_y = pad_y + self.row_height as f64;
        set_color(ctx, theme.fg);
        ctx.set_line_width(1.0);
        ctx.move_to(bw, sep_y);
        ctx.line_to(self.w as f64 - bw, sep_y);
        ctx.stroke().unwrap();

        // Items
        let list_top = sep_y + 1.0;
        let list_h = self.h as f64 - list_top - pad_y;
        let row_h = self.row_height as f64;
        let visible = ((list_h / row_h).floor() as usize).max(1);

        for row in 0..visible {
            let mi = scroll + row;
            if mi >= filter.matches.len() { break; }
            let item_idx = filter.matches[mi] as usize;
            let text = &items.raw[item_idx];

            let y = list_top + row as f64 * row_h;
            let selected = mi == cursor;
            if selected {
                set_color(ctx, theme.selbg);
                ctx.rectangle(bw, y, self.w as f64 - 2.0 * bw, row_h);
                ctx.fill().unwrap();
            }

            // Layout the line (single-line, ellipsized)
            let layout = self.layout_for(ctx, text);
            layout.set_width((self.w as f64 - pad_x * 2.0) as i32 * pango::SCALE);
            layout.set_ellipsize(EllipsizeMode::End);
            layout.set_height(0); // single line

            // Highlight match spans (under text)
            if !query.is_empty() {
                let spans = match_spans(text, query, case_sensitive);
                for (s, e) in spans {
                    if let Some((sx, ex)) = layout_span_x(&layout, s, e) {
                        // Skip if span is past ellipsis
                        let max_x = (self.w as f64 - pad_x * 2.0) as i32;
                        if sx >= max_x { continue; }
                        let ex = ex.min(max_x);
                        set_color(ctx, theme.matchbg);
                        ctx.rectangle(pad_x + sx as f64, y, (ex - sx) as f64, row_h);
                        ctx.fill().unwrap();
                    }
                }
            }

            // Text
            let fg = if selected { theme.selfg } else { theme.fg };
            set_color(ctx, fg);
            ctx.move_to(pad_x, y + text_y_off);
            pangocairo::functions::show_layout(ctx, &layout);

            // Re-draw matched glyphs in matchfg on top so they read above the
            // highlight rect. We do this by clipping to each span's x-range.
            if !query.is_empty() {
                let spans = match_spans(text, query, case_sensitive);
                for (s, e) in spans {
                    if let Some((sx, ex)) = layout_span_x(&layout, s, e) {
                        let max_x = (self.w as f64 - pad_x * 2.0) as i32;
                        if sx >= max_x { continue; }
                        let ex = ex.min(max_x);
                        ctx.save().unwrap();
                        ctx.rectangle(pad_x + sx as f64, y, (ex - sx) as f64, row_h);
                        ctx.clip();
                        set_color(ctx, theme.matchfg);
                        ctx.move_to(pad_x, y + text_y_off);
                        pangocairo::functions::show_layout(ctx, &layout);
                        ctx.restore().unwrap();
                    }
                }
            }
        }

        // Border (drawn last, on top of everything)
        if bw > 0.0 {
            set_color(ctx, theme.border);
            ctx.set_line_width(bw);
            // Stroke is centered on the path; offset by bw/2 to keep it inside.
            let off = bw / 2.0;
            ctx.rectangle(off, off, self.w as f64 - bw, self.h as f64 - bw);
            ctx.stroke().unwrap();
        }
    }

    fn layout_for(&self, ctx: &Context, text: &str) -> Layout {
        let l = pangocairo::functions::create_layout(ctx);
        l.set_font_description(Some(&self.fd));
        l.set_text(text);
        l
    }
}

fn set_color(ctx: &Context, c: Color) {
    ctx.set_source_rgb(c.0, c.1, c.2);
}

/// Return (start_x_px, end_x_px) on the first line, in layout-local coords.
fn layout_span_x(layout: &Layout, s: usize, e: usize) -> Option<(i32, i32)> {
    let line = layout.line_readonly(0)?;
    let sx = line.index_to_x(s as i32, false);
    let ex = line.index_to_x(e as i32, false);
    Some((sx / pango::SCALE, ex / pango::SCALE))
}
