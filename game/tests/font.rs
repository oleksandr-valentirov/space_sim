//! The font really does have glyphs for everything we write (ROADMAP-UI.md,
//! U7b).
//!
//! The step's question: egui's stock fonts most likely cover Cyrillic -- but
//! **check, do not assume**. A missing glyph is drawn as an empty rectangle,
//! so the bug is silent until someone looks at the screen, and one has to
//! look at each panel separately, because a string only appears in its own
//! state.
//!
//! Why this is not a screenshot: a screenshot would say "something is drawn",
//! and it is green on tofu too -- an empty rectangle is pixels as well. The
//! question is not whether there is paint but whether **different** characters
//! give **different** rasters.
//!
//! Hence a sharper oracle: for an unknown character egui substitutes one and
//! the same fallback raster. So take a character the font deliberately lacks
//! (the Unicode private use area), remember its corner in the atlas -- and no
//! glyph of any of our strings may coincide with it. This is cheaper too: no
//! GPU is needed at all, so the check runs where there is no adapter instead
//! of being skipped in silence.
//!
//! Both tables are checked, not only the Ukrainian one: language names are
//! endonyms (U7a), so "Українська" sits in the **English** table too.

use engine::egui;

use game::text::{tr, Language, ALL};

/// Characters from the Unicode private use area: no sensible font carries
/// them.
///
/// Two, not one, so that the check can fail. If these two give different
/// rasters then "the fallback raster" is not a constant, and the oracle below
/// means nothing.
const MISSING: [char; 2] = ['\u{E000}', '\u{E001}'];

/// The corner of a glyph in the font atlas -- exactly what tells one raster
/// from another.
///
/// A pair of `[u16; 2]` rather than `UvRect`: the type itself is not
/// re-exported from `epaint`, and the comparison needs exactly these four
/// numbers.
type Raster = ([u16; 2], [u16; 2]);

/// Lays out the strings and returns the glyphs of each.
///
/// All of them in one frame, because the font atlas is built inside a frame:
/// `Context::fonts` does not exist at all before the first `run_ui`, and
/// `layout_no_wrap` wants mutable access that the `fonts` reader does not
/// give. Going through `Painter` is the same path a panel lays text out by,
/// so what is checked is what the player sees.
fn rasters(family: egui::FontFamily, texts: &[String]) -> Vec<Vec<(char, Raster)>> {
    let context = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(300.0, 300.0),
        )),
        ..Default::default()
    };

    let mut out = Vec::new();
    let mut output = context.run_ui(input, |ui| {
        out = texts
            .iter()
            .map(|text| {
                let galley = ui.painter().layout_no_wrap(
                    text.clone(),
                    egui::FontId::new(14.0, family.clone()),
                    egui::Color32::WHITE,
                );
                galley
                    .rows
                    .iter()
                    .flat_map(|row| row.glyphs.iter())
                    .map(|glyph| (glyph.chr, (glyph.uv_rect.min, glyph.uv_rect.max)))
                    .collect()
            })
            .collect();
    });
    // `TexturesDelta` panics in `Drop` if it is not applied (U1a).
    output.textures_delta.clear();
    out
}

/// The raster of the first character of a string.
fn one(family: egui::FontFamily, text: &str) -> (char, Raster) {
    let mut glyphs = rasters(family, &[text.to_string()]).remove(0);
    assert_eq!(glyphs.len(), 1, "{text:?} laid out as more than one glyph");
    glyphs.remove(0)
}

/// Both families egui ships out of the box.
///
/// Panels do not set a font today, i.e. they take the proportional one. The
/// monospace one is checked **ahead of time**: U7c is about the instrument
/// palette, and instruments want a monospace face -- finding out it has no
/// Cyrillic is cheaper now than in the middle of that step.
fn families() -> [egui::FontFamily; 2] {
    [egui::FontFamily::Proportional, egui::FontFamily::Monospace]
}

/// First, that the oracle works at all: a missing character gives the
/// fallback raster, and there is only one of it.
///
/// Without this the test below would be green on a font with no glyphs at
/// all: if every unknown character gave its own raster, "did not match the
/// fallback" would mean nothing.
#[test]
fn a_missing_glyph_falls_back_to_one_and_the_same_raster() {
    for family in families() {
        let first = one(family.clone(), &MISSING[0].to_string());
        let second = one(family.clone(), &MISSING[1].to_string());

        assert_eq!(
            first.1, second.1,
            "{family:?}: two different missing characters gave different rasters -- \
             so egui does not substitute a fallback, and the check below has no oracle"
        );

        // And that fallback draws something. A zero-sized raster would mean
        // the unknown is simply invisible -- then the match above would be a
        // match of two nothings.
        assert_ne!(
            first.1 .0, first.1 .1,
            "{family:?}: the fallback raster is empty -- the oracle would be \
             comparing two voids"
        );
    }
}

/// And now the point: no string of either table is drawn with the fallback.
#[test]
fn every_string_in_both_tables_has_real_glyphs() {
    let mut texts = Vec::new();
    let mut labels = Vec::new();
    for key in ALL {
        for language in [Language::English, Language::Ukrainian] {
            texts.push(tr(language, key).to_string());
            labels.push(format!("{key:?} ({language:?})"));
        }
    }

    for family in families() {
        let tofu = one(family.clone(), &MISSING[0].to_string()).1;

        let mut checked = 0;
        for (glyphs, label) in rasters(family.clone(), &texts).iter().zip(&labels) {
            for (chr, raster) in glyphs {
                // Whitespace legitimately has no raster, and comparing it with
                // the fallback is meaningless.
                if chr.is_whitespace() {
                    continue;
                }
                assert_ne!(
                    *raster, tofu,
                    "{family:?}: in {label} the character {chr:?} is drawn as an \
                     empty rectangle -- the font does not carry it"
                );
                checked += 1;
            }
        }

        // A check that the check checked something: an empty table would pass
        // the loop above in silence.
        assert!(
            checked > 500,
            "{family:?}: only {checked} glyphs checked -- the table has shrunk or \
             the layout returned nothing"
        );
        println!("  {family:?}: glyphs checked: {checked}");
    }
}

/// The monospace font stays monospace in Cyrillic too.
///
/// The test above does not prove that, which is why this one exists: egui,
/// finding no glyph in `Hack`, silently takes it from the proportional face
/// -- the raster is real, the tofu check is green, and the letter arrives at
/// **a different width**. For an instrument panel that means a column that
/// falls apart on exactly the Ukrainian labels, i.e. a bug visible in only
/// one of the two languages.
///
/// This needs to be known before U7c, not inside it.
#[test]
fn the_monospace_family_is_monospace_in_cyrillic_too() {
    let context = egui::Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(300.0, 300.0),
        )),
        ..Default::default()
    };

    // Latin, digits and Cyrillic in one string: all widths must agree.
    let text = "iWm019аійЩ";
    let mut widths: Vec<(char, f32)> = Vec::new();
    let mut output = context.run_ui(input, |ui| {
        let galley = ui.painter().layout_no_wrap(
            text.to_string(),
            egui::FontId::monospace(14.0),
            egui::Color32::WHITE,
        );
        widths = galley
            .rows
            .iter()
            .flat_map(|row| row.glyphs.iter())
            .map(|glyph| (glyph.chr, glyph.advance_width))
            .collect();
    });
    output.textures_delta.clear();

    let (first_chr, first) = widths[0];
    for (chr, width) in &widths {
        assert!(
            (width - first).abs() < 0.01,
            "in monospace {chr:?} is {width} wide while {first_chr:?} is {first}: \
             egui substituted a glyph from another family, and the column will \
             fall apart on exactly this letter"
        );
    }
    println!(
        "  monospace width: {first} across all {} characters",
        widths.len()
    );
}

/// And separately, Cyrillic named letter by letter.
///
/// The test above would also fail on missing Latin, so its red board does not
/// say what happened. This one narrows it down: the whole Ukrainian alphabet,
/// including g with upturn, ye, i and yi, which fall outside the "Russian"
/// set and are therefore lost in fonts assembled for someone else.
#[test]
fn the_ukrainian_alphabet_is_covered_letter_by_letter() {
    let alphabet = "абвгґдеєжзиіїйклмнопрстуфхцчшщьюя\
                    АБВГҐДЕЄЖЗИІЇЙКЛМНОПРСТУФХЦЧШЩЬЮЯ";
    // Delta from the plan's dv, the multiplication sign from "warp x1000", the
    // em dash from the warnings: neither Cyrillic nor Latin, so they are lost
    // separately from both.
    let symbols = "Δ×—";

    for family in families() {
        let tofu = one(family.clone(), &MISSING[0].to_string()).1;

        for (glyphs, what) in rasters(family.clone(), &[alphabet.to_string(), symbols.to_string()])
            .iter()
            .zip(["letter", "symbol"])
        {
            for (chr, raster) in glyphs {
                assert_ne!(*raster, tofu, "{family:?}: the font has no {what} {chr:?}");
            }
        }
    }
}
