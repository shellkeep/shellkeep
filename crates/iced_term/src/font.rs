// SPDX-FileCopyrightText: 2026 shellkeep contributors
// SPDX-License-Identifier: MIT

use crate::settings::FontSettings;
use iced::{Font, Size};
use iced_core::{
    alignment::Vertical,
    text::{Alignment, LineHeight, Paragraph, Shaping as TextShaping},
    Text,
};
use iced_graphics::text::paragraph;

#[derive(Debug, Clone)]
pub struct TermFont {
    pub(crate) size: f32,
    pub(crate) font_type: Font,
    pub(crate) scale_factor: f32,
    pub(crate) measure: Size<f32>,
}

impl TermFont {
    pub fn new(settings: FontSettings) -> Self {
        // FR-TERMINAL-10: If a font family name is specified, create a named font.
        let font_type = if let Some(ref family) = settings.font_family {
            Font {
                family: iced::font::Family::Name(
                    Box::leak(family.clone().into_boxed_str()),
                ),
                ..Font::MONOSPACE
            }
        } else {
            settings.font_type
        };
        Self {
            size: settings.size,
            font_type,
            scale_factor: settings.scale_factor,
            measure: font_measure(
                settings.size,
                settings.scale_factor,
                font_type,
            ),
        }
    }

    pub fn sync(&mut self) {
        self.measure =
            font_measure(self.size, self.scale_factor, self.font_type)
    }
}

fn font_measure(
    font_size: f32,
    scale_factor: f32,
    font_type: Font,
) -> Size<f32> {
    let paragraph = paragraph::Paragraph::with_text(Text {
        content: "m",
        font: font_type,
        size: iced_core::Pixels(font_size),
        align_y: Vertical::Center,
        align_x: Alignment::Center,
        shaping: TextShaping::Advanced,
        line_height: LineHeight::Relative(scale_factor),
        bounds: Size::INFINITE,
        wrapping: iced_core::text::Wrapping::Glyph,
    });

    paragraph.min_bounds()
}
