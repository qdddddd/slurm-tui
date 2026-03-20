use ratatui::style::Color;

#[derive(Clone, Copy)]
pub struct Palette {
    pub fg: Color,
    pub gray: Color,
    pub dim: Color,
    pub red: Color,
    pub green: Color,
    pub yellow: Color,
    pub blue: Color,
    pub purple: Color,
    pub aqua: Color,
    pub orange: Color,
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            fg: Color::Rgb(0xeb, 0xdb, 0xb2),
            gray: Color::Rgb(0x92, 0x83, 0x74),
            dim: Color::Rgb(0x66, 0x5c, 0x54),
            red: Color::Rgb(0xfb, 0x49, 0x34),
            green: Color::Rgb(0xb8, 0xbb, 0x26),
            yellow: Color::Rgb(0xfa, 0xbd, 0x2f),
            blue: Color::Rgb(0x83, 0xa5, 0x98),
            purple: Color::Rgb(0xd3, 0x86, 0x9b),
            aqua: Color::Rgb(0x8e, 0xc0, 0x7c),
            orange: Color::Rgb(0xfe, 0x80, 0x19),
        }
    }

    pub fn light() -> Self {
        Self {
            fg: Color::Rgb(0x3c, 0x37, 0x35),
            gray: Color::Rgb(0x92, 0x83, 0x73),
            dim: Color::Rgb(0x7c, 0x6f, 0x64),
            red: Color::Rgb(0xcc, 0x23, 0x1c),
            green: Color::Rgb(0x98, 0x97, 0x19),
            yellow: Color::Rgb(0xd7, 0x99, 0x20),
            blue: Color::Rgb(0x45, 0x85, 0x88),
            purple: Color::Rgb(0xb1, 0x62, 0x86),
            aqua: Color::Rgb(0x68, 0x9d, 0x69),
            orange: Color::Rgb(0xd6, 0x5d, 0x0e),
        }
    }
}
