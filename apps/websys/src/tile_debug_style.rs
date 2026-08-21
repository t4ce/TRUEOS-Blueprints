#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CssColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineStyle {
    pub color: CssColor,
    pub width_px: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextStroke {
    pub width_px: u8,
    pub color: CssColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextShadow {
    pub offset_x_px: i8,
    pub offset_y_px: i8,
    pub blur_px: u8,
    pub color: CssColor,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    pub font_family: &'static str,
    pub font_size_px: u16,
    pub font_weight: u16,
    pub letter_spacing_em: f32,
    pub color: CssColor,
    pub stroke: Option<TextStroke>,
    pub shadow: Option<TextShadow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileCellStyle {
    pub row: u8,
    pub column: u8,
    pub fill: CssColor,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileDebugFrameStyle {
    pub name: &'static str,
    pub canvas_width_px: u16,
    pub canvas_height_px: u16,
    pub columns: u8,
    pub rows: u8,
    pub cell_width_px: u16,
    pub cell_height_px: u16,
    pub divider: LineStyle,
    pub center_guides: LineStyle,
    pub cell_label: TextStyle,
    pub title: TextStyle,
    pub subtitle: TextStyle,
    pub note: TextStyle,
    pub palette: [[CssColor; 12]; 6],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorRampStyle {
    pub start: CssColor,
    pub middle: CssColor,
    pub end: CssColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalTestPatternStyle {
    pub name: &'static str,
    pub terminal_width_chars: u16,
    pub terminal_height_rows: u16,
    pub background: CssColor,
    pub panel: CssColor,
    pub text_dark: CssColor,
    pub text_blue: CssColor,
    pub grid: LineStyle,
    pub accent_green: CssColor,
    pub white: CssColor,
    pub black: CssColor,
    pub primary_bars: [CssColor; 8],
    pub grayscale: ColorRampStyle,
    pub red_ramp: ColorRampStyle,
    pub green_ramp: ColorRampStyle,
    pub blue_ramp: ColorRampStyle,
}

pub const TRUEOS_TILE_DEBUG_FRAME_0: TileDebugFrameStyle = TileDebugFrameStyle {
    name: "TRUEOS TILE DEBUG FRAME 0",
    canvas_width_px: 1920,
    canvas_height_px: 1080,
    columns: 12,
    rows: 6,
    cell_width_px: 160,
    cell_height_px: 180,
    divider: LineStyle {
        color: CssColor::rgb(0x10, 0x10, 0x10),
        width_px: 4,
    },
    center_guides: LineStyle {
        color: CssColor::rgba(0x11, 0x11, 0x11, 0xD0),
        width_px: 3,
    },
    cell_label: TextStyle {
        font_family: "\"Arial Black\", \"Segoe UI\", sans-serif",
        font_size_px: 30,
        font_weight: 800,
        letter_spacing_em: 0.02,
        color: CssColor::rgb(0xF4, 0xF4, 0xF4),
        stroke: Some(TextStroke {
            width_px: 3,
            color: CssColor::rgb(0x10, 0x10, 0x10),
        }),
        shadow: Some(TextShadow {
            offset_x_px: 1,
            offset_y_px: 2,
            blur_px: 0,
            color: CssColor::rgba(0x00, 0x00, 0x00, 0x90),
        }),
    },
    title: TextStyle {
        font_family: "\"Arial Black\", \"Segoe UI\", sans-serif",
        font_size_px: 56,
        font_weight: 900,
        letter_spacing_em: 0.01,
        color: CssColor::rgb(0xF4, 0xF4, 0xF4),
        stroke: Some(TextStroke {
            width_px: 4,
            color: CssColor::rgb(0x10, 0x10, 0x10),
        }),
        shadow: Some(TextShadow {
            offset_x_px: 2,
            offset_y_px: 3,
            blur_px: 0,
            color: CssColor::rgba(0x00, 0x00, 0x00, 0x95),
        }),
    },
    subtitle: TextStyle {
        font_family: "\"Arial Black\", \"Segoe UI\", sans-serif",
        font_size_px: 26,
        font_weight: 800,
        letter_spacing_em: 0.01,
        color: CssColor::rgb(0xF4, 0xF4, 0xF4),
        stroke: Some(TextStroke {
            width_px: 2,
            color: CssColor::rgb(0x10, 0x10, 0x10),
        }),
        shadow: None,
    },
    note: TextStyle {
        font_family: "\"Arial Black\", \"Segoe UI\", sans-serif",
        font_size_px: 22,
        font_weight: 800,
        letter_spacing_em: 0.02,
        color: CssColor::rgb(0xF4, 0xF4, 0xF4),
        stroke: Some(TextStroke {
            width_px: 2,
            color: CssColor::rgb(0x10, 0x10, 0x10),
        }),
        shadow: None,
    },
    palette: [
        [
            CssColor::rgb(0xDE, 0x33, 0x29),
            CssColor::rgb(0xDF, 0x82, 0x01),
            CssColor::rgb(0xE0, 0xB1, 0x02),
            CssColor::rgb(0x2C, 0xAE, 0x4C),
            CssColor::rgb(0x00, 0xAC, 0xA6),
            CssColor::rgb(0x2B, 0x97, 0xC7),
            CssColor::rgb(0x01, 0x69, 0xDF),
            CssColor::rgb(0x4C, 0x4A, 0xB9),
            CssColor::rgb(0x98, 0x47, 0xC2),
            CssColor::rgb(0xDC, 0x26, 0x47),
            CssColor::rgb(0x87, 0x86, 0x8A),
            CssColor::rgb(0x8E, 0x73, 0x53),
        ],
        [
            CssColor::rgb(0xDF, 0x5E, 0x5D),
            CssColor::rgb(0xDE, 0x93, 0x43),
            CssColor::rgb(0xDE, 0xB9, 0x32),
            CssColor::rgb(0x5B, 0xBE, 0x6B),
            CssColor::rgb(0x32, 0xAF, 0xBF),
            CssColor::rgb(0x66, 0xA7, 0xDD),
            CssColor::rgb(0x42, 0x96, 0xD8),
            CssColor::rgb(0x64, 0x7A, 0xDC),
            CssColor::rgb(0x99, 0x83, 0xDD),
            CssColor::rgb(0xD2, 0x58, 0x81),
            CssColor::rgb(0xB8, 0xBC, 0xC4),
            CssColor::rgb(0xAD, 0x85, 0x56),
        ],
        [
            CssColor::rgb(0xAF, 0x24, 0x21),
            CssColor::rgb(0xC8, 0x68, 0x01),
            CssColor::rgb(0xDB, 0x99, 0x04),
            CssColor::rgb(0x26, 0x78, 0x36),
            CssColor::rgb(0x08, 0x62, 0x73),
            CssColor::rgb(0x30, 0x65, 0x9B),
            CssColor::rgb(0x10, 0x3E, 0x6C),
            CssColor::rgb(0x48, 0x5C, 0xB3),
            CssColor::rgb(0x6C, 0x51, 0xBE),
            CssColor::rgb(0x9C, 0x35, 0x58),
            CssColor::rgb(0x4E, 0x57, 0x5C),
            CssColor::rgb(0x6F, 0x4A, 0x31),
        ],
        [
            CssColor::rgb(0xE0, 0x76, 0x76),
            CssColor::rgb(0xDF, 0xAA, 0x6A),
            CssColor::rgb(0xC8, 0xB8, 0x76),
            CssColor::rgb(0x9C, 0xCF, 0xA2),
            CssColor::rgb(0x72, 0xAF, 0xB7),
            CssColor::rgb(0x90, 0xBD, 0xDE),
            CssColor::rgb(0x6C, 0x7E, 0xC4),
            CssColor::rgb(0xB6, 0xA6, 0xDE),
            CssColor::rgb(0xC8, 0x86, 0xD8),
            CssColor::rgb(0xDB, 0xA8, 0xBC),
            CssColor::rgb(0xC4, 0xC9, 0xCB),
            CssColor::rgb(0xBB, 0x8E, 0x65),
        ],
        [
            CssColor::rgb(0xC3, 0x2A, 0x2A),
            CssColor::rgb(0xD2, 0x79, 0x02),
            CssColor::rgb(0xDE, 0xAB, 0x18),
            CssColor::rgb(0x2C, 0x9B, 0x42),
            CssColor::rgb(0x0E, 0x84, 0x96),
            CssColor::rgb(0x18, 0x6D, 0xBA),
            CssColor::rgb(0x38, 0x54, 0xCE),
            CssColor::rgb(0x61, 0x3E, 0xC9),
            CssColor::rgb(0x87, 0x2D, 0x9D),
            CssColor::rgb(0xA8, 0x1F, 0x50),
            CssColor::rgb(0x7E, 0x87, 0x8D),
            CssColor::rgb(0x88, 0x59, 0x3B),
        ],
        [
            CssColor::rgb(0xDC, 0x46, 0x47),
            CssColor::rgb(0xDD, 0x6C, 0x10),
            CssColor::rgb(0xCB, 0xAB, 0x5C),
            CssColor::rgb(0x23, 0x8A, 0x7E),
            CssColor::rgb(0x23, 0x6C, 0x8E),
            CssColor::rgb(0x4C, 0x65, 0x7E),
            CssColor::rgb(0x3A, 0x54, 0xD0),
            CssColor::rgb(0x65, 0x06, 0x9E),
            CssColor::rgb(0xA0, 0x14, 0x8C),
            CssColor::rgb(0xD8, 0x20, 0x76),
            CssColor::rgb(0x9E, 0xA6, 0xAC),
            CssColor::rgb(0x98, 0x77, 0x5A),
        ],
    ],
};

pub const BUROSCH_AVEC_TERMINAL_PATTERN: TerminalTestPatternStyle = TerminalTestPatternStyle {
    name: "BUROSCH AVEC TERMINAL TEST PATTERN",
    terminal_width_chars: 88,
    terminal_height_rows: 34,
    background: CssColor::rgb(0x7D, 0x7D, 0x7D),
    panel: CssColor::rgb(0xD7, 0xDA, 0xE1),
    text_dark: CssColor::rgb(0x16, 0x16, 0x16),
    text_blue: CssColor::rgb(0x1A, 0x46, 0xB0),
    grid: LineStyle {
        color: CssColor::rgb(0xE7, 0xE7, 0xE7),
        width_px: 1,
    },
    accent_green: CssColor::rgb(0x20, 0xD8, 0x3A),
    white: CssColor::rgb(0xEB, 0xEB, 0xEB),
    black: CssColor::rgb(0x10, 0x10, 0x10),
    primary_bars: [
        CssColor::rgb(0xEB, 0xEB, 0xEB),
        CssColor::rgb(0xEB, 0xEB, 0x12),
        CssColor::rgb(0x0F, 0xEC, 0xED),
        CssColor::rgb(0x10, 0xEC, 0x26),
        CssColor::rgb(0xCD, 0x2E, 0xCD),
        CssColor::rgb(0xEB, 0x10, 0x34),
        CssColor::rgb(0x3E, 0x11, 0xBC),
        CssColor::rgb(0x10, 0x10, 0x10),
    ],
    grayscale: ColorRampStyle {
        start: CssColor::rgb(0x10, 0x10, 0x10),
        middle: CssColor::rgb(0x7E, 0x7E, 0x7E),
        end: CssColor::rgb(0xE2, 0xE2, 0xE2),
    },
    red_ramp: ColorRampStyle {
        start: CssColor::rgb(0xE4, 0xD5, 0xCE),
        middle: CssColor::rgb(0xF3, 0x10, 0x18),
        end: CssColor::rgb(0x10, 0x10, 0x10),
    },
    green_ramp: ColorRampStyle {
        start: CssColor::rgb(0xCE, 0xE7, 0xD2),
        middle: CssColor::rgb(0x19, 0xF0, 0x19),
        end: CssColor::rgb(0x0B, 0x20, 0x0B),
    },
    blue_ramp: ColorRampStyle {
        start: CssColor::rgb(0xC2, 0xC4, 0xDD),
        middle: CssColor::rgb(0x2F, 0x2F, 0xDC),
        end: CssColor::rgb(0x0D, 0x0D, 0x22),
    },
};

impl CssColor {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 0xFF }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn to_hex_rgb(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    pub fn to_css_rgba(self) -> String {
        if self.a == 0xFF {
            self.to_hex_rgb()
        } else {
            let alpha = (self.a as f32) / 255.0;
            format!("rgba({}, {}, {}, {:.3})", self.r, self.g, self.b, alpha)
        }
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        let mix = |from: u8, to: u8| -> u8 {
            ((from as f32) + ((to as f32) - (from as f32)) * t.clamp(0.0, 1.0)).round() as u8
        };

        Self {
            r: mix(self.r, other.r),
            g: mix(self.g, other.g),
            b: mix(self.b, other.b),
            a: mix(self.a, other.a),
        }
    }
}

impl TileCellStyle {
    pub fn label(self) -> String {
        format!("R{}C{}", self.row, self.column)
    }
}

impl TileDebugFrameStyle {
    pub const fn cell_count(&self) -> usize {
        self.columns as usize * self.rows as usize
    }

    pub fn cell(&self, row: u8, column: u8) -> Option<TileCellStyle> {
        let fill = self
            .palette
            .get(row as usize)
            .and_then(|columns| columns.get(column as usize))
            .copied()?;
        Some(TileCellStyle { row, column, fill })
    }

    pub fn to_css_custom_properties(&self, prefix: &str) -> String {
        let mut css = String::new();
        css.push_str(":root {\n");
        css.push_str(&format!(
            "  --{}-canvas-width: {}px;\n",
            prefix, self.canvas_width_px
        ));
        css.push_str(&format!(
            "  --{}-canvas-height: {}px;\n",
            prefix, self.canvas_height_px
        ));
        css.push_str(&format!(
            "  --{}-cell-width: {}px;\n",
            prefix, self.cell_width_px
        ));
        css.push_str(&format!(
            "  --{}-cell-height: {}px;\n",
            prefix, self.cell_height_px
        ));
        css.push_str(&format!(
            "  --{}-divider-color: {};\n",
            prefix,
            self.divider.color.to_css_rgba()
        ));
        css.push_str(&format!(
            "  --{}-guide-color: {};\n",
            prefix,
            self.center_guides.color.to_css_rgba()
        ));
        css.push_str(&format!(
            "  --{}-label-color: {};\n",
            prefix,
            self.cell_label.color.to_css_rgba()
        ));

        for row in 0..self.rows as usize {
            for column in 0..self.columns as usize {
                css.push_str(&format!(
                    "  --{}-r{}-c{}: {};\n",
                    prefix,
                    row,
                    column,
                    self.palette[row][column].to_hex_rgb()
                ));
            }
        }

        css.push('}');
        css.push('\n');
        css
    }
}

impl ColorRampStyle {
    pub fn sample(self, position: f32) -> CssColor {
        let position = position.clamp(0.0, 1.0);
        if position <= 0.5 {
            self.start.lerp(self.middle, position * 2.0)
        } else {
            self.middle.lerp(self.end, (position - 0.5) * 2.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BUROSCH_AVEC_TERMINAL_PATTERN, TRUEOS_TILE_DEBUG_FRAME_0};

    #[test]
    fn frame_0_has_expected_grid_geometry() {
        assert_eq!(TRUEOS_TILE_DEBUG_FRAME_0.columns, 12);
        assert_eq!(TRUEOS_TILE_DEBUG_FRAME_0.rows, 6);
        assert_eq!(TRUEOS_TILE_DEBUG_FRAME_0.cell_width_px, 160);
        assert_eq!(TRUEOS_TILE_DEBUG_FRAME_0.cell_height_px, 180);
        assert_eq!(TRUEOS_TILE_DEBUG_FRAME_0.cell_count(), 72);
    }

    #[test]
    fn frame_0_exposes_sampled_cell_colors() {
        let center_left = TRUEOS_TILE_DEBUG_FRAME_0.cell(5, 5).unwrap();
        let center_right = TRUEOS_TILE_DEBUG_FRAME_0.cell(5, 6).unwrap();
        assert_eq!(center_left.label(), "R5C5");
        assert_eq!(center_left.fill.to_hex_rgb(), "#4C657E");
        assert_eq!(center_right.fill.to_hex_rgb(), "#3A54D0");
    }

    #[test]
    fn css_variable_export_contains_palette_tokens() {
        let css = TRUEOS_TILE_DEBUG_FRAME_0.to_css_custom_properties("trueos-tile");
        assert!(css.contains("--trueos-tile-r0-c0: #DE3329;"));
        assert!(css.contains("--trueos-tile-r5-c11: #98775A;"));
    }

    #[test]
    fn burosch_primary_bars_keep_reference_order() {
        assert_eq!(
            BUROSCH_AVEC_TERMINAL_PATTERN.primary_bars[0].to_hex_rgb(),
            "#EBEBEB"
        );
        assert_eq!(
            BUROSCH_AVEC_TERMINAL_PATTERN.primary_bars[7].to_hex_rgb(),
            "#101010"
        );
    }
}
