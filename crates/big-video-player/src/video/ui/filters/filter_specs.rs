//! Pure, GTK-free filter/shader value mapping for the video filters dialog.
//!
//! Holds the UI↔shader percentage conversions (with their round-trip tests)
//! and the single
//! source of truth for each single-parameter `vf` filter's mpv string. Keeping
//! the math here lets the Relm4 dialog stay a thin widget layer and lets the
//! mappings be unit-tested without a running mpv backend.

/// FSR shader sharpness (0..=2, lower = sharper) → UI percent (0..=100).
#[must_use]
pub fn fsr_shader_to_ui_pct(sharpness: f64) -> f64 {
    (1.0 - sharpness / 2.0) * 100.0
}

/// UI percent (0..=100) → FSR shader sharpness (0..=2).
#[must_use]
pub fn fsr_ui_pct_to_shader(pct: f64) -> f64 {
    2.0 * (1.0 - pct / 100.0)
}

/// CAS shader sharpening (0..=1) → UI percent (0..=100).
#[must_use]
pub fn cas_shader_to_ui_pct(sharpening: f64) -> f64 {
    sharpening * 100.0
}

/// UI percent (0..=100) → CAS shader sharpening (0..=1).
#[must_use]
pub fn cas_ui_pct_to_shader(pct: f64) -> f64 {
    pct / 100.0
}

/// `colortemperature` vf string for a Kelvin temperature.
#[must_use]
pub fn colortemp_vf(kelvin: f64) -> String {
    format!("colortemperature=temperature={kelvin:.0}")
}

/// `colorize` vf string from hue (deg), saturation (0..1), mix (0..1).
#[must_use]
pub fn colorize_vf(hue: f64, saturation: f64, mix: f64) -> String {
    format!("colorize=hue={hue:.0}:saturation={saturation:.2}:mix={mix:.2}")
}

/// `hqdn3d` denoise vf string from a single strength value.
#[must_use]
pub fn hqdn3d_vf(strength: f64) -> String {
    format!(
        "hqdn3d=luma_spatial={strength}:chroma_spatial={}:luma_tmp={}",
        strength * 0.75,
        strength * 0.5
    )
}

/// `pixelize` vf string from a block size (rounded to an integer).
#[must_use]
pub fn pixelize_vf(block: f64) -> String {
    let block = block as i32;
    format!("pixelize=width={block}:height={block}")
}

/// mpv's `vf` property split into its entries. A graph comes either as
/// `%N%` followed by N bytes taken as they are, or in brackets; a comma
/// inside either belongs to the graph.
fn vf_entries(vf: &str) -> Vec<&str> {
    let bytes = vf.as_bytes();
    let (mut entries, mut start, mut depth, mut i) = (Vec::new(), 0, 0_usize, 0);
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let digits = bytes[i + 1..]
                    .iter()
                    .take_while(|b| b.is_ascii_digit())
                    .count();
                let close = i + 1 + digits;
                if digits > 0 && bytes.get(close) == Some(&b'%') {
                    let length: usize = vf[i + 1..close].parse().unwrap_or(0);
                    i = (close + 1 + length).min(bytes.len());
                    continue;
                }
            }
            b'[' => depth += 1,
            b']' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                entries.push(&vf[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < vf.len() {
        entries.push(&vf[start..]);
    }
    entries
}

/// The lavfi graph of the `@label` entry of mpv's `vf` property, which mpv
/// lists as `@label:lavfi=graph=%N%<graph>`; `None` when no such entry is on.
#[must_use]
pub fn active_vf_graph(vf: &str, label: &str) -> Option<String> {
    let prefix = format!("@{label}:");
    let entry = vf_entries(vf)
        .into_iter()
        .find_map(|entry| entry.strip_prefix(&prefix))?;
    let graph = entry.strip_prefix("lavfi=").unwrap_or(entry);
    let graph = graph.strip_prefix("graph=").unwrap_or(graph);
    if let Some(rest) = graph.strip_prefix('%')
        && let Some((length, rest)) = rest.split_once('%')
        && let Ok(length) = length.parse::<usize>()
    {
        return rest.get(..length).map(str::to_owned);
    }
    if let Some(inner) = graph.strip_prefix('[') {
        return Some(inner.strip_suffix(']').unwrap_or(inner).to_owned());
    }
    Some(graph.to_owned())
}

/// The value of `key` in a single-filter graph such as `cas=strength=0.50`.
#[must_use]
pub fn vf_graph_param(graph: &str, key: &str) -> Option<String> {
    let (_, options) = graph.split_once('=')?;
    options.split(':').find_map(|option| {
        let (name, value) = option.split_once('=')?;
        (name == key).then(|| value.to_owned())
    })
}

/// [`vf_graph_param`] as a number.
#[must_use]
pub fn vf_graph_number(graph: &str, key: &str) -> Option<f64> {
    vf_graph_param(graph, key)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VF: &str = "@cas:lavfi=graph=%17%cas=strength=0.50,\
@colorize:lavfi=graph=%55%colorize=hue=120:saturation=0.80:lightness=0.5:mix=0.30,\
@curves_preset:lavfi=graph=%21%curves=preset=vintage";

    #[test]
    fn active_filters_read_back_from_mpv() {
        let cas = active_vf_graph(VF, "cas").unwrap();
        assert_eq!(cas, "cas=strength=0.50");
        assert_eq!(vf_graph_number(&cas, "strength"), Some(0.5));
        let colorize = active_vf_graph(VF, "colorize").unwrap();
        assert_eq!(vf_graph_number(&colorize, "hue"), Some(120.0));
        assert_eq!(vf_graph_number(&colorize, "mix"), Some(0.3));
        let curves = active_vf_graph(VF, "curves_preset").unwrap();
        assert_eq!(
            vf_graph_param(&curves, "preset").as_deref(),
            Some("vintage")
        );
    }

    #[test]
    fn a_label_matches_only_its_own_entry() {
        // "cas" is in "@cascade" and in the graphs: neither is the CAS filter.
        let vf = "@cascade:lavfi=graph=%6%null=1,@x:lavfi=graph=%13%cas=strength=1";
        assert_eq!(active_vf_graph(vf, "cas"), None);
        assert_eq!(active_vf_graph("", "cas"), None);
    }

    #[test]
    fn a_comma_inside_a_graph_does_not_split_it() {
        let vf = "@a:lavfi=graph=%11%x=1,y=2:z=3,@b:lavfi=[b=k=7]";
        assert_eq!(active_vf_graph(vf, "a").as_deref(), Some("x=1,y=2:z=3"));
        assert_eq!(active_vf_graph(vf, "b").as_deref(), Some("b=k=7"));
    }

    #[test]
    fn fsr_ui_roundtrip_keeps_percentage() {
        let pct = 72.0;
        let sharpness = fsr_ui_pct_to_shader(pct);
        assert!((fsr_shader_to_ui_pct(sharpness) - pct).abs() < f64::EPSILON);
    }

    #[test]
    fn fsr_zero_ui_means_max_shader_sharpness() {
        assert!((fsr_ui_pct_to_shader(0.0) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cas_ui_roundtrip_keeps_percentage() {
        let pct = 37.0;
        let sharpening = cas_ui_pct_to_shader(pct);
        assert!((cas_shader_to_ui_pct(sharpening) - pct).abs() < f64::EPSILON);
    }

    #[test]
    fn cas_full_ui_means_full_shader_strength() {
        assert!((cas_ui_pct_to_shader(100.0) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn colortemp_rounds_kelvin() {
        assert_eq!(colortemp_vf(6500.4), "colortemperature=temperature=6500");
    }

    #[test]
    fn colorize_formats_all_params() {
        assert_eq!(
            colorize_vf(30.0, 0.5, 0.75),
            "colorize=hue=30:saturation=0.50:mix=0.75"
        );
    }

    #[test]
    fn hqdn3d_derives_secondary_strengths() {
        assert_eq!(
            hqdn3d_vf(4.0),
            "hqdn3d=luma_spatial=4:chroma_spatial=3:luma_tmp=2"
        );
    }

    #[test]
    fn pixelize_truncates_block_size() {
        assert_eq!(pixelize_vf(16.9), "pixelize=width=16:height=16");
    }
}
