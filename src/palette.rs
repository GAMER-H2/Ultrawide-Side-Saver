//! Colour palettes. Each is three linear-ish RGB stops that the shader blends
//! between. They are chosen to stay low-saturation and low-luminance so the bars
//! read as ambient light rather than as content.

pub struct Palette {
    pub name: &'static str,
    pub stops: [[f32; 3]; 3],
}

pub const PALETTES: &[Palette] = &[
    Palette {
        name: "aurora",
        stops: [
            [0.05, 0.10, 0.20],
            [0.10, 0.45, 0.42],
            [0.35, 0.25, 0.60],
        ],
    },
    Palette {
        name: "ember",
        stops: [
            [0.12, 0.04, 0.03],
            [0.55, 0.18, 0.06],
            [0.70, 0.42, 0.12],
        ],
    },
    Palette {
        name: "ocean",
        stops: [
            [0.02, 0.06, 0.14],
            [0.05, 0.28, 0.45],
            [0.20, 0.55, 0.60],
        ],
    },
    Palette {
        name: "mono",
        stops: [
            [0.06, 0.06, 0.07],
            [0.28, 0.29, 0.32],
            [0.55, 0.56, 0.60],
        ],
    },
    Palette {
        name: "forest",
        stops: [
            [0.03, 0.09, 0.05],
            [0.12, 0.34, 0.16],
            [0.42, 0.50, 0.22],
        ],
    },
];

pub fn lookup(name: &str) -> Option<&'static Palette> {
    PALETTES.iter().find(|p| p.name.eq_ignore_ascii_case(name))
}

pub fn names() -> Vec<&'static str> {
    PALETTES.iter().map(|p| p.name).collect()
}
