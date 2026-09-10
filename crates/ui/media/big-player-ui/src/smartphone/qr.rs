//! QR code rendering for a `gtk::DrawingArea`.

use gtk::prelude::*;
use qrcode::{EcLevel, QrCode};

/// Build a drawing area that renders `qr_payload` as a high-contrast QR code.
pub fn build_qr_widget(qr_payload: &str, side_px: i32) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.set_content_width(side_px);
    area.set_content_height(side_px);
    area.set_halign(gtk::Align::Center);
    area.set_valign(gtk::Align::Center);

    let code = QrCode::with_error_correction_level(qr_payload.as_bytes(), EcLevel::M);
    let (modules, width): (Vec<bool>, usize) = match code {
        Ok(code) => {
            let width = code.width();
            (
                code.to_colors()
                    .iter()
                    .map(|module| matches!(module, qrcode::Color::Dark))
                    .collect(),
                width,
            )
        }
        Err(_) => (Vec::new(), 0),
    };

    area.set_draw_func(move |_, ctx, width_px, height_px| {
        ctx.set_source_rgb(1.0, 1.0, 1.0);
        let _ = ctx.paint();
        if width == 0 || modules.is_empty() {
            return;
        }
        let side = f64::from(width_px.min(height_px));
        let quiet_zone = 4.0_f64;
        let total = width as f64 + quiet_zone * 2.0;
        let scale = side / total;
        let offset = quiet_zone * scale;
        ctx.set_source_rgb(0.0, 0.0, 0.0);
        for y in 0..width {
            for x in 0..width {
                if modules[y * width + x] {
                    ctx.rectangle(
                        offset + (x as f64) * scale,
                        offset + (y as f64) * scale,
                        scale.ceil(),
                        scale.ceil(),
                    );
                }
            }
        }
        let _ = ctx.fill();
    });

    area
}
