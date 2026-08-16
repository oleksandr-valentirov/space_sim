//! The instrument palette (ROADMAP-UI.md, U7c).
//!
//! PROJECT.md §2 calls mission-control aesthetics **a style, not a
//! compromise**. So this is not "make it prettier" but a decision about what
//! colours **mean** in this game, and it is one decision for the scene and the
//! interface alike.
//!
//! ## The main decision: the palette derives from the trajectories
//!
//! The line colours existed before this step (`view.rs`, H5) and already
//! carried meaning: orange for prediction, muted blue for history, green for
//! an unconfirmed preview, white for the vessel itself. An interface coloured
//! separately would start speaking a **second** language of colour over the
//! first: a "fly this" button in blue while the preview it confirms is green.
//!
//! So the interface's accents are those same four colours, and no new one:
//!
//! | colour | in the scene | in the interface |
//! |---|---|---|
//! | amber | prediction, the future | active, what it costs |
//! | blue | history, the past | reference, disabled |
//! | green | plan preview | an action not yet confirmed |
//! | white | the vessel | what is in focus |
//!
//! That is checked by a number rather than by eye: [`ACCENT`] must equal the
//! prediction's colour, and the test fails if they are separated.
//!
//! ## One colour space, one conversion
//!
//! The palette was chosen by eye, i.e. lives in **sRGB**, and holds eight bits
//! per channel. The frame works in **linear light**, and the target encodes
//! gamma in hardware (T5a). So on the way into the scene a colour must be
//! decoded exactly once: [`Colour::scene`] calls `srgb::to_linear`, while
//! [`Colour::egui`] returns the same bytes, because egui knows the target's
//! format and encodes them itself.
//!
//! WARNING: **before T5a this said "no conversions", and that was true exactly
//! for the capture.** The window picks its surface with F1's `is_srgb()`
//! filter, i.e. always encoded gamma -- and byte 200 in the scene came out on
//! screen as byte 229, while the same interface panel stayed 200. So the
//! panel's accent was not the prediction line's colour **in the window**,
//! though it was in the PNG, and the U7c2 check did not see it because it
//! looked at the PNG. Changing the capture's format moved that discrepancy
//! from the window into the test -- which it did, on the first run.

use engine::egui;

/// A palette colour: eight bits per channel, sRGB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colour(pub u8, pub u8, pub u8);

impl Colour {
    /// For the scene: `[f32; 4]` in **linear light**, as `Polyline` wants.
    ///
    /// The decoding is here rather than in the engine, and that is the
    /// boundary: the engine works in linear light and need not know that
    /// someone chose colours by eye. The converse is [`Colour::egui`]: the
    /// interface stays in sRGB, because egui encodes for the target's format
    /// itself.
    pub fn scene(self) -> [f32; 4] {
        let linear = |c: u8| engine::srgb::byte_to_linear(c) as f32;
        [linear(self.0), linear(self.1), linear(self.2), 1.0]
    }

    /// For the interface: the same bytes, unconverted.
    pub fn egui(self) -> egui::Color32 {
        egui::Color32::from_rgb(self.0, self.1, self.2)
    }

    /// The same colour dimmed to a fraction `k` of full brightness.
    ///
    /// Needed where the hue must stay the same while the weight drops: a
    /// border against a fill, a disabled button against a live one. A separate
    /// colour in the table for this would introduce a second amber that would
    /// eventually diverge from the first.
    pub fn dim(self, k: f32) -> Colour {
        let scale = |c: u8| (c as f32 * k).round().clamp(0.0, 255.0) as u8;
        Colour(scale(self.0), scale(self.1), scale(self.2))
    }
}

// ---------------------------------------------------------------------------
// The four colours that carry meaning. The rest of the palette is their
// background.

/// Prediction: what lies ahead. Also the interface's accent -- [`ACCENT`].
pub const PREDICTION: Colour = Colour(229, 153, 51);

/// History: what has already been flown. Deliberately quieter than the
/// prediction -- the past should not pull the eye from where the boundary
/// moves.
pub const HISTORY: Colour = Colour(89, 115, 153);

/// A plan preview: computed but not confirmed.
pub const PREVIEW: Colour = Colour(102, 229, 128);

/// The vessel itself.
pub const VESSEL: Colour = Colour(255, 255, 255);

/// The interface's accent -- **the same colour as the prediction**, and that
/// is checked.
pub const ACCENT: Colour = PREDICTION;

/// An action not yet confirmed -- the same colour as a preview.
pub const ACTION: Colour = PREVIEW;

// ---------------------------------------------------------------------------
// Background. Darker than the frame's sky so the panel reads over it.

/// The frame's sky in the same units -- `engine::frame::CLEAR_BYTES`.
///
/// The copy here is for checking rather than use: the panel must be **darker**
/// than the sky, or it glows against space instead of lying on it. The test
/// compares it against the engine, so a divergence will not survive.
pub const SKY: Colour = Colour(5, 8, 20);

/// The panel's fill.
///
/// Darker than the sky on every channel, and that is not a taste: under the
/// panel the frame holds not only space but also a lit planetary disc. A panel
/// designed for a dark background would stop reading over that -- so it is
/// dense and dark, and against the bare sky it therefore reads as a well. That
/// is intended; `the_panel_is_darker_than_the_sky_behind_it` checks it, and
/// its first edition failed exactly here.
pub const PANEL: Colour = Colour(4, 6, 12);

/// A widget's surface at rest.
pub const SURFACE: Colour = Colour(24, 31, 43);

/// The same under the cursor.
pub const SURFACE_HOVER: Colour = Colour(38, 48, 65);

/// And pressed.
pub const SURFACE_ACTIVE: Colour = Colour(52, 65, 86);

/// Lines: borders, separators.
pub const LINE: Colour = Colour(48, 60, 78);

/// Primary text.
pub const TEXT: Colour = Colour(201, 211, 223);

/// Secondary text: units, hints, whatever is not a number.
pub const TEXT_DIM: Colour = Colour(124, 136, 152);

/// A refusal: a plan rejected, a vessel stopped by an error.
///
/// The only colour outside the four meaningful ones, and it earned its own
/// line: "something went wrong" cannot be said in any of them without lying
/// about meaning. The red here is muted -- a panel should not shout.
pub const ALARM: Colour = Colour(214, 97, 85);

// ---------------------------------------------------------------------------
// The porkchop scale. Its ends come from the same palette, and that is not
// styling.

/// The cheap end of the window scale.
///
/// Kin to [`HISTORY`] rather than a new blue: both mean "calm, nothing to look
/// at".
pub const CHEAP: Colour = Colour(30, 70, 160);

/// The expensive end is [`PREDICTION`], because amber in this game always
/// means "what it costs".
pub const COSTLY: Colour = PREDICTION;

/// Installs the palette into a context: theme, style, both theme branches.
///
/// It lives here rather than in `app`, because everyone drawing the interface
/// without a window needs the same call -- tests and captures. A panel
/// captured with the default style would show something other than what the
/// player sees, and as an oracle would be worse than none.
pub fn apply(context: &egui::Context) {
    let style = style();
    context.set_theme(egui::ThemePreference::Dark);
    // The same style for both themes: this game has no light theme, and an
    // instrument panel gone white from a system setting is a broken frame
    // rather than a styling variant.
    context.set_style_of(egui::Theme::Dark, style.clone());
    context.set_style_of(egui::Theme::Light, style);
}

/// The whole egui style (ROADMAP-UI.md, U7c).
///
/// ## Why everything is monospaced
///
/// An instrument panel is columns of numbers the eye compares top to bottom. A
/// proportional font makes "7" narrower than "0", i.e. shifts the digits, and
/// an altitude of 412 km becomes wider than 400 km. U7b measured that egui's
/// monospace family carries Cyrillic in **its own glyphs of the same width**,
/// so this decision costs zero, and it is available only because that step
/// went first.
///
/// ## Density
///
/// The spacing is meaner than egui's default: the left panel is 220 points
/// wide (`app::draw`), and the default 8 points between elements eat the
/// screen in rows of air. Instrument density is not cosmetic but the number of
/// rows visible at once.
pub fn style() -> egui::Style {
    let mut style = egui::Style {
        visuals: visuals(),
        ..Default::default()
    };

    style.spacing.item_spacing = egui::vec2(6.0, 3.0);
    style.spacing.button_padding = egui::vec2(6.0, 2.0);
    style.spacing.indent = 12.0;
    style.spacing.interact_size.y = 18.0;

    // Everything monospaced except the panel's heading: it is one line anyway,
    // and a slightly larger size separates it from the numbers without a
    // rule.
    use egui::{FontFamily, FontId, TextStyle};
    style.text_styles = [
        (TextStyle::Heading, FontId::new(15.0, FontFamily::Monospace)),
        (TextStyle::Body, FontId::new(12.5, FontFamily::Monospace)),
        (TextStyle::Button, FontId::new(12.5, FontFamily::Monospace)),
        (TextStyle::Small, FontId::new(10.5, FontFamily::Monospace)),
        (
            TextStyle::Monospace,
            FontId::new(12.5, FontFamily::Monospace),
        ),
    ]
    .into();

    style
}

fn visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();

    visuals.panel_fill = PANEL.egui();
    visuals.window_fill = PANEL.egui();
    visuals.extreme_bg_color = SKY.egui();
    visuals.override_text_color = Some(TEXT.egui());
    visuals.window_stroke = egui::Stroke::new(1.0, LINE.egui());

    // Selection in amber, i.e. the same as the prediction. The text on it is
    // dark: amber is light, and light text does not read on it.
    visuals.selection.bg_fill = ACCENT.dim(0.55).egui();
    visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT.egui());

    let widget = |bg: Colour, stroke: Colour, text: Colour| egui::style::WidgetVisuals {
        bg_fill: bg.egui(),
        weak_bg_fill: bg.egui(),
        bg_stroke: egui::Stroke::new(1.0, stroke.egui()),
        fg_stroke: egui::Stroke::new(1.0, text.egui()),
        corner_radius: egui::CornerRadius::ZERO,
        expansion: 0.0,
    };

    // Square corners deliberately: rounding is the language of a soft
    // interface, and this is an instrument panel. Exactly the place where "a
    // style, not a compromise" shows in a single field.
    visuals.widgets.noninteractive = widget(PANEL, LINE.dim(0.6), TEXT_DIM);
    visuals.widgets.inactive = widget(SURFACE, LINE, TEXT);
    visuals.widgets.hovered = widget(SURFACE_HOVER, ACCENT.dim(0.7), TEXT);
    visuals.widgets.active = widget(SURFACE_ACTIVE, ACCENT, ACCENT);
    visuals.widgets.open = widget(SURFACE_HOVER, LINE, TEXT);

    visuals
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The interface's accent is the prediction's colour, not one like it.
    ///
    /// The whole idea of the palette rests on this line: interface and scene
    /// speak one language of colour. Two "nearly identical" ambers are the
    /// same error, merely invisible, so equality is checked rather than
    /// closeness.
    #[test]
    fn the_accent_is_the_colour_of_the_forecast() {
        assert_eq!(ACCENT, PREDICTION);
        assert_eq!(ACTION, PREVIEW);
        assert_eq!(COSTLY, PREDICTION);
    }

    /// The panel is darker than the frame's sky.
    ///
    /// Otherwise it glows against space instead of lying on it -- the kind of
    /// error invisible on a black monitor and obvious on a bright one. The sky
    /// comes from the engine rather than the copy beside it: the copy would
    /// diverge silently.
    #[test]
    fn the_panel_is_darker_than_the_sky_behind_it() {
        assert_eq!(
            [SKY.0, SKY.1, SKY.2],
            engine::frame::CLEAR_BYTES,
            "the copy of the sky colour diverged from the engine"
        );

        let weight = |c: Colour| c.0 as u32 + c.1 as u32 + c.2 as u32;
        assert!(
            weight(PANEL) < weight(SKY),
            "panel {PANEL:?} is not darker than sky {SKY:?}"
        );
    }

    /// Text reads against what it lies on.
    ///
    /// Contrast is a formula rather than a taste: WCAG computes a ratio of
    /// relative luminances, and 4.5:1 is the bound for primary text. The check
    /// is here precisely because "I can see it" on one monitor proves nothing
    /// about another.
    #[test]
    fn the_text_has_enough_contrast_against_its_background() {
        // sRGB relative luminance per the WCAG definition.
        fn luminance(c: Colour) -> f64 {
            let channel = |v: u8| {
                let v = v as f64 / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(c.0) + 0.7152 * channel(c.1) + 0.0722 * channel(c.2)
        }

        fn contrast(a: Colour, b: Colour) -> f64 {
            let (x, y) = (luminance(a), luminance(b));
            let (hi, lo) = if x > y { (x, y) } else { (y, x) };
            (hi + 0.05) / (lo + 0.05)
        }

        for (text, background, floor, what) in [
            (TEXT, PANEL, 4.5, "primary text on the panel"),
            (TEXT, SURFACE, 4.5, "primary text on a button"),
            (TEXT, SURFACE_HOVER, 4.5, "text on a hovered button"),
            // Secondary is hints and units; WCAG allows 3:1 for those as for
            // large text, and below that it stops being text.
            (TEXT_DIM, PANEL, 3.0, "secondary text"),
            (ACCENT, PANEL, 4.5, "accent on the panel"),
            (ALARM, PANEL, 4.5, "a refusal on the panel"),
            (PREVIEW, PANEL, 4.5, "an action on the panel"),
        ] {
            let ratio = contrast(text, background);
            assert!(
                ratio >= floor,
                "{what}: contrast {ratio:.2} against the required {floor} \
                 ({text:?} on {background:?})"
            );
        }
    }

    /// [`apply`] really does reach the context -- and changes what was there.
    ///
    /// The second half keeps the test from being a tautology: checking that
    /// `style().visuals.panel_fill == PANEL` would check the assignment
    /// operator. What matters is that default egui gives something **else**,
    /// i.e. the palette actually does something.
    #[test]
    fn the_style_reaches_the_context_and_differs_from_the_default() {
        let plain = egui::Context::default();
        let styled = egui::Context::default();
        apply(&styled);

        let fill = |context: &egui::Context| context.style_of(egui::Theme::Dark).visuals.panel_fill;
        assert_eq!(fill(&styled), PANEL.egui());
        assert_ne!(
            fill(&plain),
            fill(&styled),
            "the palette gave what default egui gives -- i.e. gave nothing"
        );

        // Both themes rather than only the active one: this game has no light
        // theme.
        for theme in [egui::Theme::Dark, egui::Theme::Light] {
            assert_eq!(
                styled.style_of(theme).visuals.panel_fill,
                PANEL.egui(),
                "{theme:?} kept the default style"
            );
        }
    }

    /// The panel is set in monospace -- everything, without exception.
    ///
    /// A decision easily lost at the first "let me fix the size", and it costs
    /// columns of numbers that stop lining up. U7b proved the Cyrillic in this
    /// family is its own and of the same width, so the decision costs zero.
    #[test]
    fn every_text_style_is_monospace() {
        let style = style();
        for (which, font) in &style.text_styles {
            assert_eq!(
                font.family,
                egui::FontFamily::Monospace,
                "{which:?} is not set in monospace -- a column of numbers will spread"
            );
        }
        assert!(
            style.text_styles.len() >= 5,
            "the text styles disappeared from the table"
        );
    }

    /// Dimming does not change the hue -- it changes the weight.
    #[test]
    fn dimming_keeps_the_hue() {
        let dim = ACCENT.dim(0.5);
        assert!(dim.0 < ACCENT.0 && dim.1 < ACCENT.1 && dim.2 < ACCENT.2);
        // The channel ratio is preserved to within byte rounding.
        let ratio = |c: Colour| c.0 as f32 / c.1 as f32;
        assert!((ratio(dim) - ratio(ACCENT)).abs() < 0.05);
    }
}
