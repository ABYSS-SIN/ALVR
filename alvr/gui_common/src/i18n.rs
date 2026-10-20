mod ja;

use std::{
    borrow::Cow,
    collections::HashMap,
    fs,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, Ordering},
    },
};

// Japanese by default; set ALVR_LANG=en to keep the original English text.
static JAPANESE: AtomicBool = AtomicBool::new(true);

static JA_TABLE: LazyLock<HashMap<&'static str, &'static str>> =
    LazyLock::new(|| ja::TABLE.iter().copied().collect());

pub fn init() {
    if std::env::var("ALVR_LANG").is_ok_and(|lang| !lang.starts_with("ja")) {
        JAPANESE.store(false, Ordering::Relaxed);
    }
}

pub fn is_japanese() -> bool {
    JAPANESE.load(Ordering::Relaxed)
}

pub fn set_japanese(value: bool) {
    JAPANESE.store(value, Ordering::Relaxed);
}

// Translates a UI string. Unknown strings are returned unchanged.
pub fn tr(text: &str) -> String {
    tr_cow(text).into_owned()
}

pub fn tr_cow(text: &str) -> Cow<'_, str> {
    if is_japanese() {
        if let Some(translated) = JA_TABLE.get(text) {
            return Cow::Borrowed(translated);
        }
    }

    Cow::Borrowed(text)
}

// egui's default fonts have no CJK glyphs: add the first system Japanese font found as fallback.
pub fn load_cjk_font(ctx: &egui::Context) {
    const CANDIDATES: &[&str] = &[
        "C:/Windows/Fonts/YuGothM.ttc",
        "C:/Windows/Fonts/meiryo.ttc",
        "C:/Windows/Fonts/NotoSansJP-VF.ttf",
        "C:/Windows/Fonts/msgothic.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
    ];

    let Some(data) = CANDIDATES.iter().find_map(|path| fs::read(path).ok()) else {
        return;
    };

    let mut fonts = egui::FontDefinitions::default();
    // Yu Gothic & co. are taller than egui's default font: shrink a bit to keep the layout height.
    let mut font_data = egui::FontData::from_owned(data);
    font_data.tweak.scale = 0.9;
    fonts.font_data.insert("cjk".into(), Arc::new(font_data));
    // Primary for proportional text so Latin and Japanese share one baseline (mixing fonts
    // misaligns them vertically); only a fallback for monospace.
    let proportional = fonts.families.entry(egui::FontFamily::Proportional).or_default();
    proportional.insert(0, "cjk".into());
    fonts.families.entry(egui::FontFamily::Monospace).or_default().push("cjk".into());
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_known_and_keeps_unknown() {
        set_japanese(true);
        assert_eq!(tr("Maximum bitrate"), "最大ビットレート");
        assert_eq!(tr("not in the table"), "not in the table");
        set_japanese(false);
        assert_eq!(tr("Maximum bitrate"), "Maximum bitrate");
        set_japanese(true);
    }

    #[test]
    fn table_has_no_duplicate_keys() {
        assert_eq!(JA_TABLE.len(), ja::TABLE.len());
    }
}
