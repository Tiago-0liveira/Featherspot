#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Color {
    const fn hex(value: u32) -> Self {
        Self {
            red: ((value >> 16) & 0xff) as u8,
            green: ((value >> 8) & 0xff) as u8,
            blue: (value & 0xff) as u8,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Theme {
    PastelLight,
    InkDark,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ThemeTokens {
    pub canvas: Color,
    pub surface: Color,
    pub surface_raised: Color,
    pub text: Color,
    pub text_muted: Color,
    pub accent_peach: Color,
    pub accent_lavender: Color,
    pub accent_sage: Color,
    pub focus: Color,
    pub danger: Color,
    pub control_height: u16,
    pub radius_small: u16,
    pub radius_medium: u16,
    pub motion_fast_ms: u16,
    pub motion_normal_ms: u16,
}

impl ThemeTokens {
    pub const fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::PastelLight => Self {
                canvas: Color::hex(0x00ff_f8ed),
                surface: Color::hex(0x00ff_fdf8),
                surface_raised: Color::hex(0x00ff_ffff),
                text: Color::hex(0x0020_2331),
                text_muted: Color::hex(0x005e_6170),
                accent_peach: Color::hex(0x00f4_b89f),
                accent_lavender: Color::hex(0x00c7_b8ea),
                accent_sage: Color::hex(0x00ae_cbb3),
                focus: Color::hex(0x0065_54c0),
                danger: Color::hex(0x00a5_2f3e),
                control_height: 36,
                radius_small: 6,
                radius_medium: 12,
                motion_fast_ms: 120,
                motion_normal_ms: 180,
            },
            Theme::InkDark => Self {
                canvas: Color::hex(0x000d_1320),
                surface: Color::hex(0x0015_1d2b),
                surface_raised: Color::hex(0x001c_2636),
                text: Color::hex(0x00f7_f0e7),
                text_muted: Color::hex(0x00b8_bac2),
                accent_peach: Color::hex(0x00e7_a58e),
                accent_lavender: Color::hex(0x00b8_a8dd),
                accent_sage: Color::hex(0x009a_b9a0),
                focus: Color::hex(0x00d0_c3ff),
                danger: Color::hex(0x00ff_9ca8),
                control_height: 36,
                radius_small: 6,
                radius_medium: 12,
                motion_fast_ms: 120,
                motion_normal_ms: 180,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themes_share_layout_and_motion_tokens() {
        let light = ThemeTokens::for_theme(Theme::PastelLight);
        let dark = ThemeTokens::for_theme(Theme::InkDark);
        assert_eq!(light.control_height, dark.control_height);
        assert_eq!(light.motion_normal_ms, dark.motion_normal_ms);
        assert_ne!(light.canvas, dark.canvas);
    }
}
