//! Rasterize compact price text into a 32×32 RGBA tray bitmap (straight alpha).

use crate::poller::AppStatus;
use fontdue::Font;
use std::sync::OnceLock;

pub const ICON_SIZE: u32 = 32;

static FONT: OnceLock<Font> = OnceLock::new();

fn load_font() -> Font {
    let candidates = [
        r"C:\Windows\Fonts\segoeuib.ttf",
        r"C:\Windows\Fonts\arialbd.ttf",
        r"C:\Windows\Fonts\segoeui.ttf",
        r"C:\Windows\Fonts\arial.ttf",
    ];
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(f) = Font::from_bytes(bytes, fontdue::FontSettings::default()) {
                return f;
            }
        }
    }
    panic!("No usable font found under C:\\Windows\\Fonts (need Segoe UI or Arial).");
}

fn font() -> &'static Font {
    FONT.get_or_init(load_font)
}

/// Short display string for the tray icon – always ≤4 chars so it fits in 32px.
pub fn format_price_text(price: f64) -> String {
    let p = price.abs();
    if !p.is_finite() || p == 0.0 {
        return "?".into();
    }
    if p >= 10_000.0 {
        format!("{:.0}k", (p / 1000.0).round())
    } else if p >= 1_000.0 {
        // e.g. 1540 → "1.5k"
        format!("{:.1}k", p / 1000.0)
    } else {
        // drop decimals; we have at most 4px per pixel
        format!("{:.0}", p.round())
    }
}

pub fn render_tray_rgba(status: &AppStatus, price: Option<f64>, prev_close: Option<f64>) -> Vec<u8> {
    let sz = ICON_SIZE as usize;
    let mut buf = vec![0u8; sz * sz * 4];

    let (text, r, g, b) = match status {
        AppStatus::Loading => ("...".to_string(), 180u8, 180u8, 180u8),
        AppStatus::Error(_) => ("Err".into(), 220u8, 60u8, 60u8),
        AppStatus::Stale { .. } | AppStatus::Ok { .. } => match price.filter(|x| x.is_finite()) {
            None => ("?".into(), 180u8, 180u8, 180u8),
            Some(p) => {
                let up = prev_close.map(|pc| p >= pc).unwrap_or(true);
                let s = format_price_text(p);
                let (r, g, b) = if up {
                    (60u8, 220u8, 80u8)
                } else {
                    (240u8, 70u8, 70u8)
                };
                (s, r, g, b)
            }
        },
    };

    let f = font();
    // Use the entire 32px both horizontally (≤30/32 width) and vertically
    // (cap height ≤ 28/32). Pick the size that fits in BOTH constraints.
    let font_px = choose_font_size(f, &text, sz as f32 - 2.0, sz as f32 - 4.0);
    let total_w = measure_width(f, &text, font_px);
    let x = ((sz as f32 - total_w) * 0.5).max(0.0);

    // Baseline placement: cap_height ≈ font_px * 0.72 for the chosen fonts.
    // Center the cap-height block vertically inside the 32px tile.
    let cap = font_px * 0.72;
    let y_base = ((sz as f32 + cap) * 0.5).round();

    // Outline pass (dark background for contrast on both light/dark taskbars)
    for ox in [-1.0f32, 0.0, 1.0] {
        for oy in [-1.0f32, 0.0, 1.0] {
            if ox == 0.0 && oy == 0.0 { continue; }
            let mut xi = x;
            for ch in text.chars() {
                let (m, bitmap) = f.rasterize(ch, font_px);
                // y_base is the baseline in screen coords (Y-down).
                // fontdue ymin is distance from baseline to glyph bottom (Y-up).
                // Top-left of bitmap in screen coords = y_base - ymin - height.
                let gy = y_base - m.ymin as f32 - m.height as f32;
                blit(&mut buf, sz, &bitmap, m.width, m.height,
                     xi + m.xmin as f32 + ox, gy + oy,
                     0, 0, 0, 200);
                xi += m.advance_width;
            }
        }
    }

    // Colour fill pass
    let mut xi = x;
    for ch in text.chars() {
        let (m, bitmap) = f.rasterize(ch, font_px);
        let gy = y_base - m.ymin as f32 - m.height as f32;
        blit(&mut buf, sz, &bitmap, m.width, m.height,
             xi + m.xmin as f32, gy,
             r, g, b, 255);
        xi += m.advance_width;
    }

    buf
}

fn measure_width(f: &Font, text: &str, px: f32) -> f32 {
    text.chars().map(|c| f.metrics(c, px).advance_width).sum()
}

fn choose_font_size(f: &Font, text: &str, max_w: f32, max_cap_h: f32) -> f32 {
    // cap height ~= font_px * 0.72, so font_px <= max_cap_h / 0.72
    let h_limit = max_cap_h / 0.72;
    let mut px = 30.0f32.min(h_limit);
    while px > 8.0 {
        if measure_width(f, text, px) <= max_w {
            return px;
        }
        px -= 1.0;
    }
    px
}

fn blit(buf: &mut [u8], sz: usize, bitmap: &[u8], bw: usize, bh: usize,
        x: f32, y: f32, r: u8, g: u8, b: u8, a: u8) {
    for by in 0..bh as i32 {
        for bx in 0..bw as i32 {
            let px = x.round() as i32 + bx;
            let py = y.round() as i32 + by;
            if px < 0 || py < 0 || px >= sz as i32 || py >= sz as i32 { continue; }
            let idx_b = by as usize * bw + bx as usize;
            if idx_b >= bitmap.len() { continue; }
            let cov = bitmap[idx_b] as u32;
            if cov == 0 { continue; }
            let sa = ((a as u32) * cov / 255).min(255) as u8;
            if sa == 0 { continue; }
            let idx = (py as usize * sz + px as usize) * 4;
            blend_over(&mut buf[idx..idx + 4], r, g, b, sa);
        }
    }
}

fn blend_over(dst: &mut [u8], sr: u8, sg: u8, sb: u8, sa: u8) {
    if sa == 255 || dst[3] == 0 {
        dst[0] = sr; dst[1] = sg; dst[2] = sb; dst[3] = sa;
        return;
    }
    let sau = sa as u32;
    let inv = 255 - sau;
    dst[0] = ((sr as u32 * sau + dst[0] as u32 * inv) / 255).min(255) as u8;
    dst[1] = ((sg as u32 * sau + dst[1] as u32 * inv) / 255).min(255) as u8;
    dst[2] = ((sb as u32 * sau + dst[2] as u32 * inv) / 255).min(255) as u8;
    dst[3] = (sau + (dst[3] as u32 * inv) / 255).min(255) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_price_text_cases() {
        assert_eq!(format_price_text(5.0),   "5");
        assert_eq!(format_price_text(42.7),  "43");
        assert_eq!(format_price_text(189.84),"190");
        assert_eq!(format_price_text(1540.0),"1.5k");
        assert_eq!(format_price_text(12000.0),"12k");
    }
}
