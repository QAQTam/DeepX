//! Visual theme and CJK font loading for the egui context.

use egui::Color32;

/// Apply the DeepX warm light theme.
pub(crate) fn apply_theme(ctx: &egui::Context) {
    let mut v = egui::Visuals::light();
    v.window_fill = Color32::from_rgb(0xFA, 0xF8, 0xF5);
    v.panel_fill = Color32::from_rgb(0xF3, 0xEF, 0xE9);
    v.hyperlink_color = Color32::from_rgb(0xD4, 0x78, 0x3C);
    ctx.set_visuals(v);
}

/// Load a CJK-capable font from system font directories.
/// Falls back silently if no font found (egui defaults handle the rest).
pub(crate) fn load_cjk_font(ctx: &egui::Context) {
    let mut f = egui::FontDefinitions::default();

    let found_cjk = try_load_cjk(&mut f);
    let found_mono = try_load_mono(&mut f);

    if found_cjk || found_mono {
        ctx.set_fonts(f);
    }
}

fn try_load_cjk(f: &mut egui::FontDefinitions) -> bool {
    #[cfg(target_os = "windows")]
    let paths = &[
        // Noto Sans SC — best quality, but not pre-installed on clean Windows
        "C:/Windows/Fonts/NotoSansSC-VF.ttf",
        // System defaults — always available on Chinese Windows
        "C:/Windows/Fonts/msyh.ttc",
        "C:/Windows/Fonts/simsun.ttc",
    ];
    #[cfg(target_os = "macos")]
    let paths = &[
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
    ];
    #[cfg(target_os = "linux")]
    let paths = &[
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/wqy-microhei/wqy-microhei.ttc",
    ];
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let paths: &[&str] = &[];

    for p in paths {
        if let Ok(b) = std::fs::read(p) {
            let data = egui::FontData::from_owned(b).tweak(egui::FontTweak {
                hinting: Some(false), // grayscale AA works better without ClearType hinting
                scale: 1.02,          // slightly thicker stems
                y_offset_factor: -0.02, // minor vertical correction
                coords: egui::epaint::text::VariationCoords::new([(b"wght", 450.0)]), // Medium weight for VF fonts
                ..Default::default()
            });
            f.font_data
                .insert("cjk".into(), std::sync::Arc::new(data));
            f.families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "cjk".into());
            return true;
        }
    }
    false
}

/// Load a high-quality monospace font for code blocks.
fn try_load_mono(f: &mut egui::FontDefinitions) -> bool {
    #[cfg(target_os = "windows")]
    let paths = &[
        "C:/Windows/Fonts/CascadiaMono.ttf",
        "C:/Windows/Fonts/CascadiaCode.ttf",
    ];
    #[cfg(target_os = "macos")]
    let paths = &[
        "/System/Library/Fonts/Menlo.ttc",
        "/System/Library/Fonts/SFMono-Regular.otf",
    ];
    #[cfg(target_os = "linux")]
    let paths = &[
        "/usr/share/fonts/truetype/cascadia-code/CascadiaMono.ttf",
        "/usr/share/fonts/truetype/firacode/FiraCode-Regular.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    ];
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let paths: &[&str] = &[];

    for p in paths {
        if let Ok(b) = std::fs::read(p) {
            f.font_data
                .insert("mono".into(), std::sync::Arc::new(egui::FontData::from_owned(b)));
            f.families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .insert(0, "mono".into());
            return true;
        }
    }
    false
}
