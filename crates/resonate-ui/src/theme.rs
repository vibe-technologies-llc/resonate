use std::sync::{Arc, LazyLock};

use gpui::{Font, FontFeatures, FontStyle, FontWeight, Pixels, Rgba, SharedString, Size, px, rgba};
use parking_lot::RwLock;
use resonate_core::{Accent, Appearance, DesktopLook, Ground, Theme};

use crate::motion;

struct Accents {
    mauve: u32,
    blue: u32,
    teal: u32,
    green: u32,
    amber: u32,
    peach: u32,
    red: u32,
}

impl Accents {
    const fn pick(&self, accent: Accent) -> u32 {
        match accent {
            Accent::Mauve => self.mauve,
            Accent::Blue => self.blue,
            Accent::Teal => self.teal,
            Accent::Green => self.green,
            Accent::Amber => self.amber,
            Accent::Peach => self.peach,
            Accent::Red => self.red,
        }
    }

    fn ramped(lit_at: LitAt) -> Self {
        let lit = |hue: f32| hsl(hue, lit_at.saturation, lit_at.lightness);

        Self {
            mauve: lit(MAUVE_HUE),
            blue: lit(BLUE_HUE),
            teal: lit(TEAL_HUE),
            green: lit(GREEN_HUE),
            amber: lit(AMBER_HUE),
            peach: lit(PEACH_HUE),
            red: lit(RED_HUE),
        }
    }
}

struct Flavour {
    native: Accent,
    background: u32,
    surface: u32,
    raised: u32,
    hover: u32,
    border: u32,
    outline: u32,
    text: u32,
    muted: u32,
    faint: u32,
    pitch: u32,
    paper: u32,
    scrim: u32,
    alarm: u32,
    accents: Accents,
}

impl Flavour {
    fn accent(&self, chosen: Option<Accent>) -> u32 {
        self.accents.pick(chosen.unwrap_or(self.native))
    }
}

struct Recipe {
    native: Accent,
    hue: f32,
    chroma: f32,
    alarm: f32,
    accents: LitAt,
    ladder: &'static Ladder,
}

struct Ladder {
    ground: f32,
    below: f32,
    lifted: f32,
    under_the_pointer: f32,
    hairline: f32,
    drawn_edge: f32,
    body_ink: f32,
    second_ink: f32,
    third_ink: f32,
    pitch: f32,
    paper: f32,
    scrim: f32,
}

#[derive(Clone, Copy)]
struct LitAt {
    saturation: f32,
    lightness: f32,
}

const MAUVE_HUE: f32 = 0.762;
const BLUE_HUE: f32 = 0.578;
const TEAL_HUE: f32 = 0.487;
const GREEN_HUE: f32 = 0.395;
const AMBER_HUE: f32 = 0.108;
const PEACH_HUE: f32 = 0.055;
const RED_HUE: f32 = 0.985;

const LIT_ON_NAVY: LitAt = LitAt {
    saturation: 0.60,
    lightness: 0.68,
};
const LIT_ON_GREY: LitAt = LitAt {
    saturation: 0.38,
    lightness: 0.64,
};
const LIT_ON_PLUM: LitAt = LitAt {
    saturation: 0.78,
    lightness: 0.68,
};
const LIT_ON_PAPER: LitAt = LitAt {
    saturation: 0.65,
    lightness: 0.26,
};
const LIT_ON_WHITE: LitAt = LitAt {
    saturation: 0.80,
    lightness: 0.16,
};

static AFTER_DARK: Ladder = Ladder {
    ground: 0.082,
    below: 0.056,
    lifted: 0.112,
    under_the_pointer: 0.152,
    hairline: 0.135,
    drawn_edge: 0.44,
    body_ink: 0.935,
    second_ink: 0.660,
    third_ink: 0.525,
    pitch: 0.040,
    paper: 0.968,
    scrim: 0.040,
};
static BY_DAYLIGHT: Ladder = Ladder {
    ground: 0.965,
    below: 0.940,
    lifted: 0.990,
    under_the_pointer: 0.915,
    hairline: 0.880,
    drawn_edge: 0.480,
    body_ink: 0.100,
    second_ink: 0.340,
    third_ink: 0.400,
    pitch: 0.040,
    paper: 0.968,
    scrim: 0.985,
};
static IN_FULL_LIGHT: Ladder = Ladder {
    ground: 1.0,
    below: 1.0,
    lifted: 0.970,
    under_the_pointer: 0.930,
    hairline: 0.400,
    drawn_edge: 0.340,
    body_ink: 0.0,
    second_ink: 0.100,
    third_ink: 0.180,
    pitch: 0.0,
    paper: 1.0,
    scrim: 1.0,
};
const LIGHT_SHADOW_SHARE: f32 = 0.4;
const SCRIM_ALPHA: u32 = 0xea;
const INK_CHROMA: f32 = 0.34;

fn hsl(hue: f32, saturation: f32, lightness: f32) -> u32 {
    let chroma = (1.0 - (2.0f32.mul_add(lightness, -1.0)).abs()) * saturation;
    let sector = hue.rem_euclid(1.0) * 6.0;
    let second = chroma * (1.0 - (sector % 2.0 - 1.0).abs());
    let (red, green, blue) = match sector as u32 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let floor = lightness - chroma / 2.0;
    let channel = |value: f32| ((value + floor).clamp(0.0, 1.0) * 255.0).round() as u32;

    (channel(red) << 16) | (channel(green) << 8) | channel(blue)
}

fn ramp(recipe: &Recipe) -> Flavour {
    let ladder = recipe.ladder;
    let surface = |lightness: f32| hsl(recipe.hue, recipe.chroma, lightness);
    let ink = |lightness: f32| hsl(recipe.hue, recipe.chroma * INK_CHROMA, lightness);

    Flavour {
        native: recipe.native,
        background: surface(ladder.ground),
        surface: surface(ladder.below),
        raised: surface(ladder.lifted),
        hover: surface(ladder.under_the_pointer),
        border: surface(ladder.hairline),
        outline: surface(ladder.drawn_edge),
        text: ink(ladder.body_ink),
        muted: ink(ladder.second_ink),
        faint: ink(ladder.third_ink),
        pitch: surface(ladder.pitch),
        paper: ink(ladder.paper),
        scrim: (surface(ladder.scrim) << 8) | SCRIM_ALPHA,
        alarm: hsl(recipe.alarm, 0.62, 0.55),
        accents: Accents::ramped(recipe.accents),
    }
}

static RESONATE: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Mauve,
    background: 0x0c0c0e,
    surface: 0x121215,
    raised: 0x1a1a1f,
    hover: 0x222228,
    border: 0x222227,
    outline: 0x656577,
    text: 0xf1efe9,
    muted: 0x9f9d97,
    faint: 0x85827b,
    pitch: 0x0b0a10,
    paper: 0xf6f4ee,
    scrim: 0x08080aea,
    alarm: 0xb4373a,
    accents: Accents {
        mauve: 0xb98ef0,
        blue: 0x7cb6ea,
        teal: 0x5ecfc0,
        green: 0x5ccb92,
        amber: 0xe4a960,
        peach: 0xe8875f,
        red: 0xe36f6f,
    },
});

static MIDNIGHT: LazyLock<Flavour> = LazyLock::new(|| {
    ramp(&Recipe {
        native: Accent::Blue,
        hue: 0.598,
        chroma: 0.17,
        alarm: RED_HUE,
        accents: LIT_ON_NAVY,
        ladder: &AFTER_DARK,
    })
});

static GRAPHITE: LazyLock<Flavour> = LazyLock::new(|| {
    ramp(&Recipe {
        native: Accent::Blue,
        hue: 0.0,
        chroma: 0.0,
        alarm: RED_HUE,
        accents: LIT_ON_GREY,
        ladder: &AFTER_DARK,
    })
});

static AMOLED: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Mauve,
    background: 0x000000,
    surface: 0x000000,
    raised: 0x121212,
    hover: 0x1d1d1d,
    border: 0x1a1a1a,
    outline: 0x616161,
    text: 0xf2f2f2,
    muted: 0xa3a3a3,
    faint: 0x7d7d7d,
    pitch: 0x000000,
    paper: 0xf7f7f7,
    scrim: 0x000000ea,
    alarm: 0xc2383c,
    accents: Accents {
        mauve: 0xb98ef0,
        blue: 0x7cb6ea,
        teal: 0x5ecfc0,
        green: 0x5ccb92,
        amber: 0xe4a960,
        peach: 0xe8875f,
        red: 0xe36f6f,
    },
});

static PLUM: LazyLock<Flavour> = LazyLock::new(|| {
    ramp(&Recipe {
        native: Accent::Mauve,
        hue: 0.783,
        chroma: 0.15,
        alarm: RED_HUE,
        accents: LIT_ON_PLUM,
        ladder: &AFTER_DARK,
    })
});

static ROSE_PINE: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Mauve,
    background: 0x1f1d2e,
    surface: 0x191724,
    raised: 0x26233a,
    hover: 0x403d52,
    border: 0x2a2739,
    outline: 0x706a93,
    text: 0xe0def4,
    muted: 0xa8a3c4,
    faint: 0x8d89a6,
    pitch: 0x16141f,
    paper: 0xfaf4ed,
    scrim: 0x191724ea,
    alarm: 0xe5406f,
    accents: Accents {
        mauve: 0xc4a7e7,
        blue: 0x71b4cf,
        teal: 0x9ccfd8,
        green: 0x81cf9e,
        amber: 0xf6c177,
        peach: 0xebbcba,
        red: 0xef8da8,
    },
});

static ROSE_PINE_MOON: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Mauve,
    background: 0x2a273f,
    surface: 0x232136,
    raised: 0x393552,
    hover: 0x44415a,
    border: 0x36324d,
    outline: 0x837e9f,
    text: 0xe0def4,
    muted: 0xaba6c8,
    faint: 0xa39eb7,
    pitch: 0x1d1b2e,
    paper: 0xfaf4ed,
    scrim: 0x232136ea,
    alarm: 0xe5406f,
    accents: Accents {
        mauve: 0xc5a8e7,
        blue: 0x77bcdc,
        teal: 0x9ccfd8,
        green: 0x81cf9e,
        amber: 0xf6c177,
        peach: 0xeba09d,
        red: 0xf199b2,
    },
});

static CATPPUCCIN_MOCHA: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Mauve,
    background: 0x1e1e2e,
    surface: 0x181825,
    raised: 0x313244,
    hover: 0x45475a,
    border: 0x313244,
    outline: 0x777b94,
    text: 0xcdd6f4,
    muted: 0xa6adc8,
    faint: 0x949ab2,
    pitch: 0x11111b,
    paper: 0xeff1f5,
    scrim: 0x11111bea,
    alarm: 0xe8245b,
    accents: Accents {
        mauve: 0xcca7f7,
        blue: 0x8fb8fa,
        teal: 0x94e2d5,
        green: 0xa6e3a1,
        amber: 0xf9e2af,
        peach: 0xfab387,
        red: 0xf59db5,
    },
});

static CATPPUCCIN_MACCHIATO: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Mauve,
    background: 0x24273a,
    surface: 0x1e2030,
    raised: 0x363a4f,
    hover: 0x494d64,
    border: 0x363a4f,
    outline: 0x7e849d,
    text: 0xcad3f5,
    muted: 0xa5adcb,
    faint: 0x9da4be,
    pitch: 0x181926,
    paper: 0xeff1f5,
    scrim: 0x181926ea,
    alarm: 0xe0314b,
    accents: Accents {
        mauve: 0xd1b2f8,
        blue: 0xa5c0f7,
        teal: 0x8bd5ca,
        green: 0xa6da95,
        amber: 0xeed49f,
        peach: 0xf6af88,
        red: 0xf3acb7,
    },
});

static CATPPUCCIN_FRAPPE: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Mauve,
    background: 0x303446,
    surface: 0x292c3c,
    raised: 0x414559,
    hover: 0x51576d,
    border: 0x414559,
    outline: 0x8b91a6,
    text: 0xc6d0f5,
    muted: 0xb5bfe2,
    faint: 0xacb3ca,
    pitch: 0x232634,
    paper: 0xeff1f5,
    scrim: 0x232634ea,
    alarm: 0xd9383b,
    accents: Accents {
        mauve: 0xdfc4f0,
        blue: 0xbdcef5,
        teal: 0xa4d7d0,
        green: 0xb3d89a,
        amber: 0xe6ca93,
        peach: 0xf5c3a9,
        red: 0xf3c1c2,
    },
});

static NORD: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Blue,
    background: 0x2e3440,
    surface: 0x29303c,
    raised: 0x3b4252,
    hover: 0x434c5e,
    border: 0x3b4252,
    outline: 0x818da4,
    text: 0xeceff4,
    muted: 0xa7b1c2,
    faint: 0xa6aebd,
    pitch: 0x242933,
    paper: 0xeceff4,
    scrim: 0x242933ea,
    alarm: 0xbf616a,
    accents: Accents {
        mauve: 0xd2bbcd,
        blue: 0xafc4d8,
        teal: 0xa4c9c8,
        green: 0xb2c99f,
        amber: 0xebcb8b,
        peach: 0xe3b8aa,
        red: 0xe1b7bc,
    },
});

static GRUVBOX_DARK: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Amber,
    background: 0x282828,
    surface: 0x1d2021,
    raised: 0x3c3836,
    hover: 0x504945,
    border: 0x3c3836,
    outline: 0x8d8075,
    text: 0xebdbb2,
    muted: 0xbdae93,
    faint: 0xaea08c,
    pitch: 0x16181a,
    paper: 0xfbf1c7,
    scrim: 0x1d2021ea,
    alarm: 0xcc241d,
    accents: Accents {
        mauve: 0xe1abba,
        blue: 0xa6bfb5,
        teal: 0x98c688,
        green: 0xbdc027,
        amber: 0xfabd2f,
        peach: 0xfea55c,
        red: 0xfda196,
    },
});

static TOKYO_NIGHT: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Blue,
    background: 0x1a1b26,
    surface: 0x16161e,
    raised: 0x24283b,
    hover: 0x292e42,
    border: 0x232433,
    outline: 0x646fa1,
    text: 0xc0caf5,
    muted: 0xa9b1d6,
    faint: 0x878eaf,
    pitch: 0x101014,
    paper: 0xd5d6db,
    scrim: 0x16161eea,
    alarm: 0xe10d35,
    accents: Accents {
        mauve: 0xbb9af7,
        blue: 0x7aa2f7,
        teal: 0x2ac3de,
        green: 0x9ece6a,
        amber: 0xe0af68,
        peach: 0xff9e64,
        red: 0xf7768e,
    },
});

static DRACULA: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Blue,
    background: 0x282a36,
    surface: 0x21222c,
    raised: 0x343746,
    hover: 0x44475a,
    border: 0x343746,
    outline: 0x7b7f9b,
    text: 0xf8f8f2,
    muted: 0xa9abc2,
    faint: 0x95a0c2,
    pitch: 0x191a21,
    paper: 0xf8f8f2,
    scrim: 0x191a21ea,
    alarm: 0xff5151,
    accents: Accents {
        mauve: 0xff93d1,
        blue: 0xc9a7fa,
        teal: 0x8be9fd,
        green: 0x50fa7b,
        amber: 0xf1fa8c,
        peach: 0xffb86c,
        red: 0xff9a9a,
    },
});

static ONE_DARK: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Blue,
    background: 0x282c34,
    surface: 0x21252b,
    raised: 0x2c313a,
    hover: 0x3e4451,
    border: 0x2c313a,
    outline: 0x6f7992,
    text: 0xd7dae0,
    muted: 0xabb2bf,
    faint: 0x9198a5,
    pitch: 0x181a1f,
    paper: 0xd7dae0,
    scrim: 0x181a1fea,
    alarm: 0xd84853,
    accents: Accents {
        mauve: 0xd59ce6,
        blue: 0x70b7f1,
        teal: 0x66bdc8,
        green: 0x98c379,
        amber: 0xe5c07b,
        peach: 0xd7a77a,
        red: 0xea9ba1,
    },
});

static SOLARIZED_DARK: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Blue,
    background: 0x002b36,
    surface: 0x00212b,
    raised: 0x073642,
    hover: 0x0a4251,
    border: 0x073642,
    outline: 0x657e86,
    text: 0xeee8d5,
    muted: 0x93a1a1,
    faint: 0x8b9b9d,
    pitch: 0x000000,
    paper: 0xfdf6e3,
    scrim: 0x001b22ea,
    alarm: 0xdc322f,
    accents: Accents {
        mauve: 0xa0a4d9,
        blue: 0x66b0e3,
        teal: 0x31bcb1,
        green: 0x9bb300,
        amber: 0xd19e00,
        peach: 0xee8d65,
        red: 0xeb8c8b,
    },
});

static HIGH_CONTRAST: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Amber,
    background: 0x000000,
    surface: 0x000000,
    raised: 0x141414,
    hover: 0x2b2b2b,
    border: 0x808080,
    outline: 0xa8a8a8,
    text: 0xffffff,
    muted: 0xe0e0e0,
    faint: 0xbdbdbd,
    pitch: 0x000000,
    paper: 0xffffff,
    scrim: 0x000000f5,
    alarm: 0xa80000,
    accents: Accents {
        mauve: 0xc9a9ff,
        blue: 0x7fc4ff,
        teal: 0x4fe8d6,
        green: 0x5cf09a,
        amber: 0xffc94d,
        peach: 0xff9f70,
        red: 0xff9b9b,
    },
});

static RESONATE_LIGHT: LazyLock<Flavour> = LazyLock::new(|| {
    ramp(&Recipe {
        native: Accent::Mauve,
        hue: 0.10,
        chroma: 0.10,
        alarm: RED_HUE,
        accents: LIT_ON_PAPER,
        ladder: &BY_DAYLIGHT,
    })
});

static HIGH_CONTRAST_LIGHT: LazyLock<Flavour> = LazyLock::new(|| {
    ramp(&Recipe {
        native: Accent::Blue,
        hue: 0.0,
        chroma: 0.0,
        alarm: RED_HUE,
        accents: LIT_ON_WHITE,
        ladder: &IN_FULL_LIGHT,
    })
});

fn flavour(theme: Theme) -> &'static Flavour {
    match theme {
        Theme::Resonate => &RESONATE,
        Theme::Midnight => &MIDNIGHT,
        Theme::Graphite => &GRAPHITE,
        Theme::Amoled => &AMOLED,
        Theme::Plum => &PLUM,
        Theme::RosePine => &ROSE_PINE,
        Theme::RosePineMoon => &ROSE_PINE_MOON,
        Theme::CatppuccinMocha => &CATPPUCCIN_MOCHA,
        Theme::CatppuccinMacchiato => &CATPPUCCIN_MACCHIATO,
        Theme::CatppuccinFrappe => &CATPPUCCIN_FRAPPE,
        Theme::Nord => &NORD,
        Theme::GruvboxDark => &GRUVBOX_DARK,
        Theme::TokyoNight => &TOKYO_NIGHT,
        Theme::Dracula => &DRACULA,
        Theme::OneDark => &ONE_DARK,
        Theme::SolarizedDark => &SOLARIZED_DARK,
        Theme::HighContrast => &HIGH_CONTRAST,
        Theme::Light => &RESONATE_LIGHT,
        Theme::HighContrastLight => &HIGH_CONTRAST_LIGHT,
    }
}

struct Worn {
    chosen: Appearance,
    look: DesktopLook,
    appearance: Appearance,
    flavour: &'static Flavour,
    accent: u32,
    scale: f32,
}

impl Worn {
    fn of(chosen: Appearance, look: DesktopLook) -> Self {
        let appearance = chosen.seen_by(look);
        let flavour = flavour(appearance.theme);

        Self {
            chosen,
            look,
            appearance,
            flavour,
            accent: flavour.accent(appearance.accent),
            scale: appearance.text_size.scale(),
        }
    }
}

static WORN: LazyLock<RwLock<Worn>> =
    LazyLock::new(|| RwLock::new(Worn::of(Appearance::DEFAULT, DesktopLook::default())));

pub fn wear(chosen: Appearance) {
    let mut worn = WORN.write();
    *worn = Worn::of(chosen, worn.look);
}

pub fn hear_the_desktop(look: DesktopLook) {
    let mut worn = WORN.write();
    *worn = Worn::of(worn.chosen, look);
}

pub fn worn() -> Appearance {
    WORN.read().appearance
}

pub fn chosen() -> Appearance {
    WORN.read().chosen
}

pub(crate) fn ground() -> Ground {
    WORN.read().appearance.theme.ground()
}

pub(crate) fn shadow(strength: f32) -> gpui::Hsla {
    let share = match ground() {
        Ground::Dark => 1.0,
        Ground::Light => LIGHT_SHADOW_SHARE,
    };

    gpui::hsla(0.0, 0.0, 0.0, strength * share)
}

pub(crate) struct Sample {
    pub(crate) background: u32,
    pub(crate) surface: u32,
    pub(crate) raised: u32,
    pub(crate) accent: u32,
}

pub(crate) fn sample(appearance: Appearance) -> Sample {
    let flavour = flavour(appearance.theme);

    Sample {
        background: flavour.background,
        surface: flavour.surface,
        raised: flavour.raised,
        accent: flavour.accent(appearance.accent),
    }
}

pub(crate) fn background() -> u32 {
    WORN.read().flavour.background
}

pub(crate) fn surface() -> u32 {
    WORN.read().flavour.surface
}

pub(crate) fn raised() -> u32 {
    WORN.read().flavour.raised
}

pub(crate) fn hover() -> u32 {
    WORN.read().flavour.hover
}

pub(crate) fn border() -> u32 {
    WORN.read().flavour.border
}

pub(crate) fn outline() -> u32 {
    WORN.read().flavour.outline
}

pub(crate) fn text() -> u32 {
    WORN.read().flavour.text
}

pub(crate) fn muted() -> u32 {
    WORN.read().flavour.muted
}

pub(crate) fn faint() -> u32 {
    WORN.read().flavour.faint
}

pub(crate) fn accent() -> u32 {
    WORN.read().accent
}

pub(crate) fn hue(accent: Accent) -> u32 {
    WORN.read().flavour.accents.pick(accent)
}

pub(crate) fn beside_the_accent(nth: usize) -> u32 {
    let worn = WORN.read();
    let others: Vec<u32> = Accent::ALL
        .into_iter()
        .map(|other| worn.flavour.accents.pick(other))
        .filter(|other| *other != worn.accent)
        .collect();
    others
        .get(nth % others.len().max(1))
        .copied()
        .unwrap_or(worn.accent)
}

pub(crate) fn ink_over(colour: u32) -> u32 {
    let worn = WORN.read();

    reads_better_on(colour, worn.flavour.pitch, worn.flavour.paper)
}

pub(crate) fn accent_ink() -> u32 {
    ink_over(accent())
}

pub(crate) fn soft_edge() -> u32 {
    soft_edge_of(WORN.read().flavour)
}

fn soft_edge_of(flavour: &Flavour) -> u32 {
    motion::blended(
        flavour.border,
        flavour.outline,
        SOFT_EDGE_TOWARD_THE_OUTLINE,
    )
}

pub(crate) fn switch_knob_off() -> u32 {
    ink_over(outline())
}

const SOFT_EDGE_TOWARD_THE_OUTLINE: f32 = 0.25;

pub(crate) const FOCUS_RING_WIDTH: f32 = 2.0;

pub(crate) fn focus_ring() -> u32 {
    let worn = WORN.read();

    ring_of(worn.flavour, worn.accent)
}

pub(crate) fn focus_ring_over(fill: u32) -> u32 {
    let worn = WORN.read();
    let ring = ring_of(worn.flavour, worn.accent);

    [
        ring,
        worn.flavour.text,
        worn.flavour.pitch,
        worn.flavour.paper,
    ]
    .into_iter()
    .find(|candidate| contrast(*candidate, fill) >= RING_STANDS_APART)
    .unwrap_or(worn.flavour.text)
}

fn ring_of(flavour: &Flavour, accent: u32) -> u32 {
    let panes = [
        flavour.background,
        flavour.surface,
        flavour.raised,
        flavour.hover,
    ];

    [accent, flavour.text]
        .into_iter()
        .find(|candidate| {
            panes
                .iter()
                .all(|pane| contrast(*candidate, *pane) >= RING_STANDS_APART)
        })
        .unwrap_or(flavour.text)
}

pub(crate) fn selection() -> u32 {
    (accent() << 8) | u32::from(SELECTION_ALPHA)
}

pub(crate) fn scrim() -> u32 {
    WORN.read().flavour.scrim
}

pub(crate) fn bit_perfect() -> u32 {
    WORN.read().flavour.accents.green
}

pub(crate) fn lossless() -> u32 {
    bit_perfect()
}

pub(crate) fn done() -> u32 {
    bit_perfect()
}

pub(crate) fn repacked() -> u32 {
    WORN.read().flavour.accents.blue
}

pub(crate) fn dithered() -> u32 {
    WORN.read().flavour.accents.amber
}

pub(crate) fn converted() -> u32 {
    muted()
}

pub(crate) fn lossy() -> u32 {
    muted()
}

pub(crate) fn suspect() -> u32 {
    WORN.read().flavour.accents.peach
}

pub(crate) fn failure() -> u32 {
    WORN.read().flavour.accents.red
}

pub(crate) fn close_hover() -> u32 {
    WORN.read().flavour.alarm
}

fn reads_better_on(over: u32, pitch: u32, paper: u32) -> u32 {
    if contrast(pitch, over) >= contrast(paper, over) {
        pitch
    } else {
        paper
    }
}

pub(crate) fn luminance(colour: u32) -> f32 {
    let channel = |shift: u32| {
        let raw = ((colour >> shift) & 0xff) as f32 / 255.0;
        if raw <= 0.03928 {
            raw / 12.92
        } else {
            ((raw + 0.055) / 1.055).powf(2.4)
        }
    };

    0.0722f32.mul_add(
        channel(0),
        0.2126f32.mul_add(channel(16), 0.7152 * channel(8)),
    )
}

pub(crate) fn lifted(colour: u32, toward_white: f32) -> u32 {
    let channel = |shift: u32| {
        let value = ((colour >> shift) & 0xff) as f32;
        (255.0 - value).mul_add(toward_white, value).round() as u32
    };

    (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

pub(crate) fn contrast(over: u32, under: u32) -> f32 {
    let (over, under) = (luminance(over), luminance(under));
    let (lit, dim) = if over > under {
        (over, under)
    } else {
        (under, over)
    };

    (lit + 0.05) / (dim + 0.05)
}

const SELECTION_ALPHA: u8 = 0x24;
const RING_STANDS_APART: f32 = 3.0;

pub const UNMARKED: u32 = 0x00000000;

const TEXT_XS: f32 = 11.0;
const TEXT_SM: f32 = 12.5;
const TEXT_BASE: f32 = 13.5;
const TEXT_LG: f32 = 16.0;
const TEXT_XL: f32 = 21.0;
const TEXT_TITLE: f32 = 26.0;
const TEXT_LYRIC: f32 = 36.0;

pub const WINDOW_MIN_WIDTH: f32 = 720.0;
pub const WINDOW_MIN_HEIGHT: f32 = 520.0;

const HEADER_HEIGHT: f32 = 52.0;
const SIDEBAR_WIDTH: f32 = 216.0;
const SEARCH_WIDTH: f32 = 560.0;
const TRANSPORT_HEIGHT: f32 = 88.0;
const TRANSPORT_CENTRE: f32 = 460.0;
const TRANSPORT_STEP: Control = Control::of(36.0, 18.0);
const TRANSPORT_PLAY: Control = Control::of(40.0, 20.0);
const TOGGLE: Control = Control::of(32.0, 16.0);
const VOLUME_READING: f32 = 34.0;
const SLEEP_READING: f32 = 34.0;
const TYPE_AHEAD_LIFT: f32 = 22.0;
const PANE_ICON: f32 = 16.0;
const SEARCH_ICON: f32 = 15.0;
const ROW_MARKER_ICON: f32 = 11.0;
const COLUMN_MARK: f32 = 10.0;
const ROW_CONTROL: Control = Control::of(26.0, 14.0);
const PICKER_WIDTH: f32 = 400.0;
const LISTEN_WIDTH: f32 = 460.0;

const ORDER_WIDTH: f32 = 264.0;

const WRAPPED_WIDTH: f32 = 640.0;
const WRAPPED_STAGE: f32 = 380.0;
const LISTEN_COVER: f32 = 112.0;
const PAIRING_TILE: f32 = 176.0;
const STANDING_MARK: f32 = 40.0;
const STANDING_MARK_ICON: f32 = 20.0;
const DEVICE_MARK: f32 = 32.0;
const DEVICE_MARK_ICON: f32 = 16.0;
const SCANNED_PAPER: u32 = 0xffffff;
const SCANNED_INK: u32 = 0x000000;
const CONFIRM_WIDTH: f32 = 420.0;
const DOWNLOADS_HEIGHT: f32 = 420.0;
const DOWNLOADS_WIDTH: f32 = 440.0;
const CLOCK_WIDTH: f32 = 44.0;
const NOW_PLAYING_COVER: f32 = 56.0;
const GRID_COVER: f32 = 164.0;
const SHELF_COVER: f32 = 104.0;
const GRID_GAP: f32 = 20.0;
const GRID_ROW: f32 = 230.0;
const SCOPE_COVER: f32 = 224.0;
const BUTTON_HEIGHT: f32 = 28.0;
const PILL_HEIGHT: f32 = 26.0;
const WAY_BACK_HEIGHT: f32 = 26.0;
const COLUMN_HEADER_HEIGHT: f32 = 28.0;
const SIDEBAR_ROW: f32 = 32.0;
const ROW_HEIGHT: f32 = 36.0;
const TALL_ROW_HEIGHT: f32 = 48.0;
const ROW_NUMBER: f32 = 30.0;
const ROW_ARTIST: f32 = 200.0;
const ROW_FORMAT: f32 = 196.0;
const ROW_PLAYS: f32 = 96.0;
const ROW_LENGTH: f32 = 48.0;
const ROW_COVER: f32 = 26.0;
const SEARCH_HEIGHT: f32 = 32.0;
const FIELD_LABEL: f32 = 96.0;
const HEADING_NAME: f32 = 220.0;
const CARET_HEIGHT: f32 = 16.0;
const CLEAR: Control = Control::of(24.0, 12.0);
const FILTER_CLEAR: Control = Control::of(24.0, 14.0);
const HINT: Control = Control::of(24.0, 14.0);
const CHECK_MARK: f32 = 14.0;
const HINT_WIDTH: f32 = 300.0;
const MENU_WIDTH: f32 = 236.0;
const RECORD_WIDTH: f32 = 320.0;
const MENU_ROW: f32 = 28.0;
const MENU_ICON: f32 = 13.0;
const GHOST_WIDTH: f32 = 260.0;
const ROOT_ICON: f32 = 14.0;
const SETTINGS_COLUMN: f32 = 720.0;
const CURVE_HEIGHT_AT_LEAST: f32 = 160.0;
const CURVE_HEIGHT_AT_MOST: f32 = 320.0;
const INSPECTOR_COLUMN: f32 = 760.0;
const SETTINGS_RAIL: f32 = 188.0;
const LYRIC_COLUMN: f32 = 720.0;
const LYRIC_GUTTER: f32 = 32.0;
const LYRIC_END_RULE: f32 = 36.0;
const LYRIC_PAD: f32 = 16.0;
const LYRIC_BREAK: f32 = 18.0;
const LYRIC_EDGE: f32 = 88.0;
const LYRIC_DOT: f32 = 7.0;
const LYRIC_DOT_GAP: f32 = 10.0;
const LYRIC_BREATH: f32 = 28.0;
const VOLUME_WIDTH: f32 = 88.0;
const AVATAR: f32 = 34.0;
const STAGE_WIDTH: f32 = 200.0;
const STAT_TILE: f32 = 150.0;
const CHART_HEIGHT: f32 = 148.0;
const CHART_AXIS: f32 = 56.0;
const CHART_BAR_WIDEST: f32 = 48.0;
const RHYTHM_HEIGHT: f32 = 96.0;
const RHYTHM_BAR_WIDEST: f32 = 40.0;
const STAT_MARK: f32 = 32.0;
const STAT_MARK_ICON: f32 = 16.0;
const STATISTICS_TWO_COLUMNS: f32 = 760.0;
const NUMBER_ONE_ART: f32 = 72.0;
const METER_HEIGHT: f32 = 6.0;
const LEGEND_MARK: f32 = 9.0;
const SUGGESTION_CARD: f32 = 252.0;
const EMPTY_ICON: f32 = 34.0;
const THEME_SWATCH: f32 = 128.0;
const THEME_SWATCH_STRIP: f32 = 28.0;
const THEME_SWATCH_SIDEBAR: f32 = 26.0;
const THEME_SWATCH_RAISED: f32 = 18.0;
const THEME_SWATCH_ACCENT: f32 = 8.0;
const ACCENT_SWATCH: f32 = 26.0;
const SWITCH_TRACK: f32 = 32.0;
const SWITCH_HEIGHT: f32 = 18.0;
const SWITCH_KNOB: f32 = 14.0;

pub const PICKER_ROWS: usize = 9;
pub const CARET_WIDTH: f32 = 1.5;
pub const SCAN_BAR: f32 = 4.0;
pub const RAIL_HEIGHT: f32 = 24.0;
pub const RAIL_TRACK: f32 = 4.0;
pub const CHART_BAR: f32 = 3.0;
pub const CHART_BAR_GAP: f32 = 2.0;
pub const CHART_BASELINE: f32 = 2.0;
pub const RAIL_THUMB: f32 = 12.0;
pub const RAIL_BUBBLE_WIDTH: f32 = 64.0;
pub const RAIL_BUBBLE_GAP: f32 = 8.0;
pub const WINDOW_CONTROL: f32 = 31.0;
pub const WINDOW_MARK: f32 = 14.0;
pub const WINDOW_MARK_OVERLAP: f32 = 3.0;
pub const RESIZE_BORDER: f32 = 10.0;
pub const CORNER_RADIUS: f32 = 12.0;
pub const FRAME_BORDER: f32 = 1.0;
pub const MAGNIFIED_COVER_SHARE: f32 = 0.7;
pub const MAGNIFIED_COVER_MAX: f32 = 720.0;

fn scaled(base: f32) -> f32 {
    base * WORN.read().scale
}

const MARK_MARGIN_AT_LEAST: f32 = 3.0;
const TARGET_AT_LEAST: f32 = 24.0;

const _: () = assert!(
    RAIL_HEIGHT >= TARGET_AT_LEAST,
    "a rail's hit area is smaller than a pointer target"
);

#[derive(Clone, Copy)]
struct Control {
    hit: f32,
    mark: f32,
}

impl Control {
    const fn of(hit: f32, mark: f32) -> Self {
        assert!(
            mark > 0.0 && mark + 2.0 * MARK_MARGIN_AT_LEAST <= hit,
            "a control's mark does not sit inside its box"
        );
        assert!(
            hit >= TARGET_AT_LEAST,
            "a control's box is smaller than a pointer target"
        );
        Self { hit, mark }
    }
}

macro_rules! measures {
    ($($measure:ident => $base:expr),+ $(,)?) => {
        $(
            pub fn $measure() -> f32 {
                scaled($base)
            }
        )+

        #[cfg(test)]
        fn every_measure() -> Vec<(&'static str, f32)> {
            vec![$((stringify!($measure), $measure())),+]
        }
    };
}

measures! {
    text_xs => TEXT_XS,
    text_sm => TEXT_SM,
    text_base => TEXT_BASE,
    text_lg => TEXT_LG,
    text_xl => TEXT_XL,
    text_title => TEXT_TITLE,
    text_lyric => TEXT_LYRIC,
    header_height => HEADER_HEIGHT,
    sidebar_width => SIDEBAR_WIDTH,
    search_width => SEARCH_WIDTH,
    transport_height => TRANSPORT_HEIGHT,
    transport_centre => TRANSPORT_CENTRE,
    transport_step => TRANSPORT_STEP.hit,
    transport_step_icon => TRANSPORT_STEP.mark,
    transport_play => TRANSPORT_PLAY.hit,
    transport_play_icon => TRANSPORT_PLAY.mark,
    toggle_control => TOGGLE.hit,
    toggle_icon => TOGGLE.mark,
    volume_reading => VOLUME_READING,
    sleep_reading => SLEEP_READING,
    type_ahead_lift => TYPE_AHEAD_LIFT,
    pane_icon => PANE_ICON,
    search_icon => SEARCH_ICON,
    row_marker_icon => ROW_MARKER_ICON,
    column_mark => COLUMN_MARK,
    row_control => ROW_CONTROL.hit,
    row_control_icon => ROW_CONTROL.mark,
    picker_width => PICKER_WIDTH,
    listen_width => LISTEN_WIDTH,
    order_width => ORDER_WIDTH,
    wrapped_width => WRAPPED_WIDTH,
    wrapped_stage => WRAPPED_STAGE,
    listen_cover => LISTEN_COVER,
    pairing_tile => PAIRING_TILE,
    standing_mark => STANDING_MARK,
    standing_mark_icon => STANDING_MARK_ICON,
    device_mark => DEVICE_MARK,
    device_mark_icon => DEVICE_MARK_ICON,
    confirm_width => CONFIRM_WIDTH,
    downloads_height => DOWNLOADS_HEIGHT,
    downloads_width => DOWNLOADS_WIDTH,
    clock_width => CLOCK_WIDTH,
    now_playing_cover => NOW_PLAYING_COVER,
    grid_cover => GRID_COVER,
    shelf_cover => SHELF_COVER,
    grid_gap => GRID_GAP,
    grid_row => GRID_ROW,
    scope_cover => SCOPE_COVER,
    button_height => BUTTON_HEIGHT,
    pill_height => PILL_HEIGHT,
    way_back_height => WAY_BACK_HEIGHT,
    column_header_height => COLUMN_HEADER_HEIGHT,
    sidebar_row => SIDEBAR_ROW,
    row_height => ROW_HEIGHT,
    tall_row_height => TALL_ROW_HEIGHT,
    row_number => ROW_NUMBER,
    row_artist => ROW_ARTIST,
    row_format => ROW_FORMAT,
    row_plays => ROW_PLAYS,
    row_length => ROW_LENGTH,
    row_cover => ROW_COVER,
    search_height => SEARCH_HEIGHT,
    field_label => FIELD_LABEL,
    heading_name => HEADING_NAME,
    caret_height => CARET_HEIGHT,
    clear_control => CLEAR.hit,
    clear_mark => CLEAR.mark,
    filter_clear_control => FILTER_CLEAR.hit,
    filter_clear_mark => FILTER_CLEAR.mark,
    hint_control => HINT.hit,
    hint_icon => HINT.mark,
    check_mark => CHECK_MARK,
    hint_width => HINT_WIDTH,
    menu_width => MENU_WIDTH,
    record_width => RECORD_WIDTH,
    menu_row => MENU_ROW,
    menu_icon => MENU_ICON,
    ghost_width => GHOST_WIDTH,
    root_icon => ROOT_ICON,
    settings_column => SETTINGS_COLUMN,
    curve_height_at_least => CURVE_HEIGHT_AT_LEAST,
    curve_height_at_most => CURVE_HEIGHT_AT_MOST,
    inspector_column => INSPECTOR_COLUMN,
    settings_rail => SETTINGS_RAIL,
    lyric_column => LYRIC_COLUMN,
    lyric_gutter => LYRIC_GUTTER,
    lyric_end_rule => LYRIC_END_RULE,
    lyric_pad => LYRIC_PAD,
    lyric_break => LYRIC_BREAK,
    lyric_edge => LYRIC_EDGE,
    lyric_dot => LYRIC_DOT,
    lyric_dot_gap => LYRIC_DOT_GAP,
    lyric_breath => LYRIC_BREATH,
    volume_width => VOLUME_WIDTH,
    avatar => AVATAR,
    stage_width => STAGE_WIDTH,
    stat_tile => STAT_TILE,
    chart_height => CHART_HEIGHT,
    chart_axis => CHART_AXIS,
    chart_bar_widest => CHART_BAR_WIDEST,
    rhythm_height => RHYTHM_HEIGHT,
    rhythm_bar_widest => RHYTHM_BAR_WIDEST,
    stat_mark => STAT_MARK,
    stat_mark_icon => STAT_MARK_ICON,
    statistics_two_columns => STATISTICS_TWO_COLUMNS,
    number_one_art => NUMBER_ONE_ART,
    meter_height => METER_HEIGHT,
    legend_mark => LEGEND_MARK,
    suggestion_card => SUGGESTION_CARD,
    empty_icon => EMPTY_ICON,
    theme_swatch => THEME_SWATCH,
    theme_swatch_strip => THEME_SWATCH_STRIP,
    theme_swatch_sidebar => THEME_SWATCH_SIDEBAR,
    theme_swatch_raised => THEME_SWATCH_RAISED,
    theme_swatch_accent => THEME_SWATCH_ACCENT,
    accent_swatch => ACCENT_SWATCH,
    switch_track => SWITCH_TRACK,
    switch_height => SWITCH_HEIGHT,
    switch_knob => SWITCH_KNOB,
}

pub fn width(value: f32) -> Pixels {
    px(value)
}

pub(crate) const fn scanned_paper() -> u32 {
    SCANNED_PAPER
}

pub(crate) const fn scanned_ink() -> u32 {
    SCANNED_INK
}

pub fn tinted(colour: u32, alpha: u8) -> Rgba {
    rgba((colour << 8) | u32::from(alpha))
}

static TABULAR_FIGURES: LazyLock<FontFeatures> =
    LazyLock::new(|| FontFeatures(Arc::new(vec![("tnum".to_owned(), 1)])));

pub fn tabular() -> FontFeatures {
    TABULAR_FIGURES.clone()
}

pub fn mono(weight: FontWeight) -> Font {
    Font {
        family: mono_face(),
        features: tabular(),
        fallbacks: None,
        weight,
        style: FontStyle::Normal,
    }
}

pub fn ui(weight: FontWeight) -> Font {
    Font {
        family: ui_face(),
        features: tabular(),
        fallbacks: None,
        weight,
        style: FontStyle::Normal,
    }
}

pub fn ui_face() -> SharedString {
    crate::fonts::body()
}

pub fn mono_face() -> SharedString {
    crate::fonts::monospace()
}

pub fn magnified_cover(viewport: Size<Pixels>) -> Pixels {
    let shorter = f32::from(viewport.width).min(f32::from(viewport.height));
    px((shorter * MAGNIFIED_COVER_SHARE).min(MAGNIFIED_COVER_MAX))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use resonate_core::{Following, TextSize};

    use super::*;

    const READS_AS_BODY: f32 = 4.5;
    const READS_AT_REST: f32 = 7.0;
    const STANDS_APART: f32 = 3.0;
    const WASH_ALPHA: u8 = 0x1c;
    const SEVEN_TO_ONE: f32 = 7.0;
    const FOUR_AND_A_HALF: f32 = 4.5;

    static WEARING: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    fn mixed(over: u32, alpha: u8, under: u32) -> u32 {
        let share = f32::from(alpha) / 255.0;
        let channel = |shift: u32| {
            let top = ((over >> shift) & 0xff) as f32;
            let bottom = ((under >> shift) & 0xff) as f32;
            top.mul_add(share, bottom * (1.0 - share)).round() as u32
        };

        (channel(16) << 16) | (channel(8) << 8) | channel(0)
    }

    fn panes_of(flavour: &Flavour) -> [(&'static str, u32); 4] {
        [
            ("background", flavour.background),
            ("surface", flavour.surface),
            ("raised", flavour.raised),
            ("hover", flavour.hover),
        ]
    }

    fn resting_panes_of(flavour: &Flavour) -> [(&'static str, u32); 3] {
        let [background, surface, raised, _] = panes_of(flavour);

        [background, surface, raised]
    }

    fn colours_of(flavour: &Flavour) -> [(&'static str, u32); 7] {
        [
            ("mauve", flavour.accents.mauve),
            ("blue", flavour.accents.blue),
            ("teal", flavour.accents.teal),
            ("green", flavour.accents.green),
            ("amber", flavour.accents.amber),
            ("peach", flavour.accents.peach),
            ("red", flavour.accents.red),
        ]
    }

    fn nothing_failed(failures: Vec<String>) {
        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    }

    fn every_appearance() -> impl Iterator<Item = Appearance> {
        Theme::ALL.into_iter().flat_map(|theme| {
            [None]
                .into_iter()
                .chain(Accent::ALL.map(Some))
                .map(move |accent| Appearance {
                    theme,
                    accent,
                    ..Appearance::DEFAULT
                })
        })
    }

    #[test]
    fn every_theme_is_dark_or_light_enough_to_read_its_own_text_on() {
        for theme in Theme::ALL {
            let flavour = flavour(theme);
            for pane in [flavour.background, flavour.surface] {
                let read = contrast(flavour.text, pane);
                assert!(
                    read >= READS_AT_REST,
                    "{theme}: text on {pane:06x} reads at {read:.1}"
                );
            }
            for lifted in [flavour.raised, flavour.hover] {
                let read = contrast(flavour.text, lifted);
                assert!(
                    read >= READS_AS_BODY,
                    "{theme}: text on {lifted:06x} reads at {read:.1}"
                );
            }

            for under in [flavour.background, flavour.surface, flavour.raised] {
                let read = contrast(flavour.muted, under);
                assert!(
                    read >= READS_AS_BODY,
                    "{theme}: muted text on {under:06x} reads at {read:.2}"
                );
            }
            for pane in [flavour.background, flavour.surface] {
                let read = contrast(flavour.faint, pane);
                assert!(
                    read >= READS_AS_BODY,
                    "{theme}: faint text on {pane:06x} reads at {read:.2}"
                );
            }
        }
    }

    #[test]
    fn every_theme_steps_its_inks_down_from_text_through_muted_to_faint() {
        for theme in Theme::ALL {
            let flavour = flavour(theme);
            let on_the_ground = |ink: u32| contrast(ink, flavour.background);

            assert!(
                on_the_ground(flavour.text) > on_the_ground(flavour.muted),
                "{theme} draws muted text as bright as its body text"
            );
            assert!(
                on_the_ground(flavour.muted) > on_the_ground(flavour.faint),
                "{theme} draws faint text as bright as its muted text"
            );
        }
    }

    #[test]
    fn every_theme_lifts_its_surfaces_away_from_its_ground() {
        for theme in Theme::ALL {
            let flavour = flavour(theme);
            let apart = |over: u32, under: u32| (luminance(over) - luminance(under)).abs();

            assert!(
                apart(flavour.raised, flavour.background) > 0.0,
                "{theme} draws what is raised at the ground's own lightness"
            );
            assert!(
                apart(flavour.hover, flavour.raised) > 0.0,
                "{theme} draws a hovered control at the raised lightness"
            );
            assert!(
                apart(flavour.outline, flavour.border) > 0.0,
                "{theme} draws its outline at its border's lightness"
            );
        }
    }

    #[test]
    fn every_accent_carries_its_own_ink_and_stands_off_the_panes() {
        for appearance in every_appearance() {
            let flavour = flavour(appearance.theme);
            let accent = flavour.accent(appearance.accent);
            let named = appearance.accent.unwrap_or(flavour.native);
            let theme = appearance.theme;
            let ink = reads_better_on(accent, flavour.pitch, flavour.paper);

            let written = contrast(accent, ink);
            assert!(
                written >= READS_AS_BODY,
                "{theme}/{named}: what is written on the accent reads at {written:.1}"
            );

            let stands = contrast(accent, flavour.background);
            assert!(
                stands >= STANDS_APART,
                "{theme}/{named}: the accent reads at {stands:.1} against the panes"
            );
        }
    }

    #[test]
    fn every_theme_draws_each_verdict_of_a_study_in_a_colour_of_its_own() {
        for theme in Theme::ALL {
            let flavour = flavour(theme);
            let drawn = [
                flavour.accents.green,
                flavour.accents.peach,
                flavour.accents.red,
                flavour.muted,
                flavour.faint,
            ];

            for (at, colour) in drawn.iter().enumerate() {
                assert!(
                    !drawn[..at].contains(colour),
                    "{theme} draws two verdicts in {colour:06x}"
                );
            }
        }
    }

    #[test]
    fn what_is_written_on_a_colour_is_whichever_of_the_two_inks_reads_better() {
        for theme in Theme::ALL {
            let flavour = flavour(theme);

            assert_eq!(
                reads_better_on(flavour.paper, flavour.pitch, flavour.paper),
                flavour.pitch,
                "{theme} writes on its own paper in paper"
            );
            assert_eq!(
                reads_better_on(flavour.pitch, flavour.pitch, flavour.paper),
                flavour.paper,
                "{theme} writes on its own pitch in pitch"
            );
        }
    }

    #[test]
    fn no_theme_gives_two_accents_the_same_colour() {
        for theme in Theme::ALL {
            let accents = &flavour(theme).accents;
            for accent in Accent::ALL {
                let sharing = Accent::ALL
                    .into_iter()
                    .filter(|other| accents.pick(*other) == accents.pick(accent))
                    .count();
                assert_eq!(sharing, 1, "{theme} draws {accent} as another accent");
            }
        }
    }

    #[test]
    fn a_palette_draws_the_accent_it_was_built_around_where_none_is_asked_for() {
        for theme in Theme::ALL {
            let flavour = flavour(theme);
            let dressed = Appearance {
                theme,
                accent: None,
                ..Appearance::DEFAULT
            };

            assert_eq!(
                sample(dressed).accent,
                flavour.accents.pick(flavour.native),
                "{theme} draws something other than its own accent when none is named"
            );
        }
    }

    #[test]
    fn a_scrim_is_opaque_enough_to_hide_what_is_under_it() {
        for theme in Theme::ALL {
            assert!(
                (flavour(theme).scrim & 0xff) >= 0xd0,
                "{theme}'s scrim lets the panes through"
            );
        }
    }

    #[test]
    fn each_ramped_palette_lights_the_accents_its_own_way() {
        let rows = [&*MIDNIGHT, &*GRAPHITE, &*PLUM]
            .map(|flavour| Accent::ALL.map(|accent| flavour.accent(Some(accent))));
        assert_ne!(rows[0], rows[1]);
        assert_ne!(rows[1], rows[2]);
        assert_ne!(rows[0], rows[2]);
    }

    #[test]
    fn a_ramped_palette_reads_back_the_lightness_it_was_asked_for() {
        let grey = hsl(0.0, 0.0, 0.5);
        assert_eq!(grey, 0x808080);

        assert_eq!(hsl(0.0, 0.0, 0.0), 0x000000);
        assert_eq!(hsl(0.0, 0.0, 1.0), 0xffffff);
        assert_eq!(hsl(0.0, 1.0, 0.5), 0xff0000);
        assert_eq!(hsl(1.0 / 3.0, 1.0, 0.5), 0x00ff00);
        assert_eq!(hsl(2.0 / 3.0, 1.0, 0.5), 0x0000ff);
    }

    #[test]
    fn what_is_worn_is_what_was_asked_for() {
        let _worn = WEARING.lock();

        for appearance in every_appearance() {
            wear(appearance);

            assert_eq!(worn(), appearance);
            assert_eq!(accent(), sample(appearance).accent);
            assert_eq!(background(), flavour(appearance.theme).background);
            assert_eq!(selection() >> 8, accent());
            assert_eq!(selection() & 0xff, u32::from(SELECTION_ALPHA));
        }

        wear(Appearance::DEFAULT);
        assert_eq!(worn(), Appearance::DEFAULT);
    }

    #[test]
    fn a_larger_text_size_draws_every_measure_larger() {
        let _worn = WEARING.lock();

        let read = |size: TextSize| {
            wear(Appearance {
                text_size: size,
                ..Appearance::DEFAULT
            });
            every_measure()
        };

        let sizes = TextSize::ALL.map(read);

        for pair in sizes.windows(2) {
            for (smaller, larger) in pair[0].iter().zip(&pair[1]) {
                assert!(
                    smaller.1 < larger.1,
                    "{} does not grow from {} to {}",
                    smaller.0,
                    smaller.1,
                    larger.1
                );
            }
        }

        let medium = read(TextSize::Medium);
        let largest = read(TextSize::Largest);
        for (middling, doubled) in medium.iter().zip(&largest) {
            assert_eq!(
                middling.1 * 2.0,
                doubled.1,
                "{} is not twice its middling measure at the largest size",
                middling.0
            );
        }
        assert_eq!(medium[2].1, TEXT_BASE);

        wear(Appearance::DEFAULT);
    }

    #[test]
    fn a_switch_knob_thrown_off_stands_off_its_track() {
        let mut failures = Vec::new();

        for theme in Theme::ALL {
            let flavour = flavour(theme);
            let knob = reads_better_on(flavour.outline, flavour.pitch, flavour.paper);
            let read = contrast(knob, flavour.outline);
            if read < RING_STANDS_APART {
                failures.push(format!(
                    "{theme}: the off knob reads at {read:.2} on its track"
                ));
            }
        }

        nothing_failed(failures);
    }

    #[test]
    fn every_drawn_edge_stands_off_the_panes_it_is_drawn_on() {
        let mut failures = Vec::new();

        for theme in Theme::ALL {
            let flavour = flavour(theme);
            for (name, pane) in resting_panes_of(flavour) {
                let read = contrast(flavour.outline, pane);
                if read < STANDS_APART {
                    failures.push(format!("{theme}: outline on {name} reads at {read:.2}"));
                }
            }
        }

        nothing_failed(failures);
    }

    #[test]
    fn a_soft_edge_is_quiet_beside_the_outline_and_strong_in_high_contrast() {
        let mut failures = Vec::new();

        for theme in Theme::ALL {
            let flavour = flavour(theme);
            let soft = soft_edge_of(flavour);
            let strong = matches!(theme, Theme::HighContrast | Theme::HighContrastLight);
            for (name, pane) in resting_panes_of(flavour) {
                let read = contrast(soft, pane);
                if strong && read < FOUR_AND_A_HALF {
                    failures.push(format!(
                        "{theme}: the soft edge on {name} reads at {read:.2}"
                    ));
                }
                if !strong && read >= STANDS_APART {
                    failures.push(format!(
                        "{theme}: the soft edge on {name} glows at {read:.2}"
                    ));
                }
            }
            if contrast(soft, flavour.raised) <= 1.0 {
                failures.push(format!("{theme}: the soft edge is lost on what floats"));
            }
        }

        nothing_failed(failures);
    }

    #[test]
    fn every_focus_ring_stands_off_the_panes_it_rings() {
        let mut failures = Vec::new();

        for appearance in every_appearance() {
            let flavour = flavour(appearance.theme);
            let ring = ring_of(flavour, flavour.accent(appearance.accent));
            let theme = appearance.theme;
            let named = appearance.accent.unwrap_or(flavour.native);

            for (name, pane) in panes_of(flavour) {
                let read = contrast(ring, pane);
                if read < STANDS_APART {
                    failures.push(format!(
                        "{theme}/{named}: the focus ring on {name} reads at {read:.2}"
                    ));
                }
            }
        }

        nothing_failed(failures);
    }

    #[test]
    fn every_accent_is_text_that_reads_on_the_panes() {
        let mut failures = Vec::new();

        for theme in Theme::ALL {
            let flavour = flavour(theme);
            for (hue, colour) in colours_of(flavour) {
                for (name, pane) in panes_of(flavour) {
                    let read = contrast(colour, pane);
                    if read < READS_AS_BODY {
                        failures.push(format!("{theme}: {hue} text on {name} reads at {read:.2}"));
                    }
                }
            }
        }

        nothing_failed(failures);
    }

    #[test]
    fn every_colour_reads_as_text_on_its_own_wash() {
        let mut failures = Vec::new();

        for theme in Theme::ALL {
            let flavour = flavour(theme);
            for (hue, colour) in colours_of(flavour) {
                for (name, pane) in resting_panes_of(flavour) {
                    let washed = mixed(colour, WASH_ALPHA, pane);
                    let read = contrast(colour, washed);
                    if read < READS_AS_BODY {
                        failures.push(format!(
                            "{theme}: {hue} text on its wash over {name} reads at {read:.2}"
                        ));
                    }
                }
            }
        }

        nothing_failed(failures);
    }

    #[test]
    fn the_inks_read_on_every_pane_they_are_drawn_on() {
        let mut failures = Vec::new();

        for theme in Theme::ALL {
            let flavour = flavour(theme);
            for (name, pane) in resting_panes_of(flavour) {
                for (ink, colour) in [("muted", flavour.muted), ("faint", flavour.faint)] {
                    let read = contrast(colour, pane);
                    if read < READS_AS_BODY {
                        failures.push(format!("{theme}: {ink} on {name} reads at {read:.2}"));
                    }
                }
            }
        }

        nothing_failed(failures);
    }

    #[test]
    fn text_reads_on_the_selection_wash() {
        let mut failures = Vec::new();

        for appearance in every_appearance() {
            let flavour = flavour(appearance.theme);
            let accent = flavour.accent(appearance.accent);
            let theme = appearance.theme;
            let named = appearance.accent.unwrap_or(flavour.native);

            for (name, pane) in resting_panes_of(flavour) {
                let selected = mixed(accent, SELECTION_ALPHA, pane);
                let read = contrast(flavour.text, selected);
                if read < READS_AS_BODY {
                    failures.push(format!(
                        "{theme}/{named}: text on the selection over {name} reads at {read:.2}"
                    ));
                }
            }
        }

        nothing_failed(failures);
    }

    #[test]
    fn the_mark_on_a_closing_window_stands_off_the_alarm() {
        for theme in Theme::ALL {
            let flavour = flavour(theme);
            let read = contrast(flavour.text, flavour.alarm);

            assert!(
                read >= STANDS_APART,
                "{theme}: the close mark on the alarm reads at {read:.2}"
            );
        }
    }

    #[test]
    fn both_high_contrasts_clear_every_bar_at_the_stricter_ratio() {
        let mut failures = Vec::new();

        for theme in [Theme::HighContrast, Theme::HighContrastLight] {
            failures.extend(
                high_contrast_failures(flavour(theme))
                    .into_iter()
                    .map(|failure| format!("{theme}: {failure}")),
            );
        }

        nothing_failed(failures);
    }

    fn high_contrast_failures(flavour: &Flavour) -> Vec<String> {
        let mut failures = Vec::new();

        for (name, pane) in panes_of(flavour) {
            for (ink, colour) in [
                ("text", flavour.text),
                ("muted", flavour.muted),
                ("faint", flavour.faint),
            ] {
                let read = contrast(colour, pane);
                if read < SEVEN_TO_ONE {
                    failures.push(format!("{ink} on {name} reads at {read:.2}"));
                }
            }
            for (hue, colour) in colours_of(flavour) {
                let read = contrast(colour, pane);
                if read < SEVEN_TO_ONE {
                    failures.push(format!("{hue} on {name} reads at {read:.2}"));
                }
                if name == "hover" {
                    continue;
                }
                let washed = mixed(colour, WASH_ALPHA, pane);
                let read = contrast(colour, washed);
                if read < SEVEN_TO_ONE {
                    failures.push(format!("{hue} on its wash over {name} reads at {read:.2}"));
                }
            }
            for (edge, colour) in [("border", flavour.border), ("outline", flavour.outline)] {
                if name == "hover" {
                    continue;
                }
                let read = contrast(colour, pane);
                if read < FOUR_AND_A_HALF {
                    failures.push(format!("{edge} on {name} reads at {read:.2}"));
                }
            }
            let ring = contrast(ring_of(flavour, flavour.accent(None)), pane);
            if ring < SEVEN_TO_ONE {
                failures.push(format!("the focus ring on {name} reads at {ring:.2}"));
            }
        }

        failures
    }

    #[test]
    fn a_light_theme_has_a_ground_lighter_than_its_text_and_a_dark_one_darker() {
        for theme in Theme::ALL {
            let flavour = flavour(theme);
            let ground = luminance(flavour.background);
            let text = luminance(flavour.text);

            match theme.ground() {
                Ground::Light => assert!(ground > text, "{theme} is light under lighter text"),
                Ground::Dark => assert!(ground < text, "{theme} is dark under darker text"),
            }
        }
    }

    #[test]
    fn the_light_themes_raise_what_is_raised_toward_the_ground_they_stand_on() {
        for theme in Theme::ALL
            .into_iter()
            .filter(|theme| theme.ground() == Ground::Light)
        {
            let flavour = flavour(theme);

            assert!(
                luminance(flavour.hover) < luminance(flavour.raised),
                "{theme} lets the pointer lighten a raised control"
            );
        }
    }

    #[test]
    fn a_scrim_stands_on_the_ground_of_its_palette() {
        for theme in Theme::ALL {
            let flavour = flavour(theme);
            let scrim = flavour.scrim >> 8;

            match theme.ground() {
                Ground::Light => assert!(
                    luminance(scrim) > luminance(flavour.text),
                    "{theme}'s scrim is darker than its text"
                ),
                Ground::Dark => assert!(
                    luminance(scrim) < luminance(flavour.text),
                    "{theme}'s scrim is lighter than its text"
                ),
            }
        }
    }

    #[test]
    fn a_shadow_is_softer_on_a_light_ground_than_on_a_dark_one() {
        let _worn = WEARING.lock();

        wear(Appearance::DEFAULT);
        let on_dark = shadow(0.5).a;
        wear(Appearance {
            theme: Theme::Light,
            ..Appearance::DEFAULT
        });
        let on_light = shadow(0.5).a;

        assert_eq!(on_dark, 0.5);
        assert!(on_light < on_dark);

        wear(Appearance::DEFAULT);
    }

    #[test]
    fn the_desktop_light_wears_the_counterpart_and_its_scale_the_nearest_size() {
        let _worn = WEARING.lock();

        wear(Appearance::DEFAULT);
        hear_the_desktop(DesktopLook {
            ground: Some(Ground::Light),
            text_scale: Some(1.5),
        });

        assert_eq!(worn().theme, Theme::Light);
        assert_eq!(worn().text_size, TextSize::Larger);
        assert_eq!(chosen(), Appearance::DEFAULT);
        assert_eq!(background(), flavour(Theme::Light).background);
        assert_eq!(text_base(), TEXT_BASE * TextSize::Larger.scale());

        hear_the_desktop(DesktopLook::default());
        assert_eq!(worn(), Appearance::DEFAULT);
    }

    #[test]
    fn the_desktop_is_not_heard_where_the_listener_wears_a_palette_and_size_of_their_own() {
        let _worn = WEARING.lock();

        wear(Appearance {
            theme: Theme::Nord,
            text_size: TextSize::Small,
            follows: Following {
                scheme: false,
                text: false,
            },
            ..Appearance::DEFAULT
        });
        hear_the_desktop(DesktopLook {
            ground: Some(Ground::Light),
            text_scale: Some(2.0),
        });

        assert_eq!(worn().theme, Theme::Nord);
        assert_eq!(worn().text_size, TextSize::Small);

        hear_the_desktop(DesktopLook::default());
        wear(Appearance::DEFAULT);
    }

    #[test]
    fn a_focus_ring_over_a_fill_is_whichever_ink_stands_off_it() {
        let _worn = WEARING.lock();

        for appearance in every_appearance() {
            wear(appearance);
            let fill = accent();
            let ring = focus_ring_over(fill);

            assert!(
                contrast(ring, fill) >= STANDS_APART,
                "{}: a ring over the accent reads at {:.2}",
                appearance.theme,
                contrast(ring, fill)
            );
        }

        wear(Appearance::DEFAULT);
    }

    const HEIGHT_CALLS: [&str; 4] = [".h(px(", ".min_h(px(", ".max_h(px(", ".size(px("];
    const HAIRLINES: [f32; 2] = [0.0, 1.0];
    const FIXED_BY_DESIGN: [(&str, &str); 3] = [
        ("dropping.rs", ".h(px(4.0))"),
        ("kit.rs", ".size(px(7.0))"),
        ("root.rs", ".size(px(22.0))"),
    ];

    fn view_sources(folder: &Path, read: &mut Vec<(String, String)>) {
        let mut entries: Vec<PathBuf> = fs::read_dir(folder)
            .expect("the views folder is readable")
            .map(|entry| entry.expect("a readable directory entry").path())
            .collect();
        entries.sort();

        for path in entries {
            if path.is_dir() {
                view_sources(&path, read);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let name = path
                    .file_name()
                    .expect("a file has a name")
                    .to_string_lossy()
                    .into_owned();
                read.push((
                    name,
                    fs::read_to_string(&path).expect("a readable source file"),
                ));
            }
        }
    }

    fn literal_heights(source: &str) -> Vec<String> {
        let mut found = Vec::new();

        for line in source.lines() {
            let line = line.trim();
            for call in HEIGHT_CALLS {
                let mut rest = line;
                while let Some(at) = rest.find(call) {
                    let after = &rest[at + call.len()..];
                    let digits: String = after
                        .chars()
                        .take_while(|letter| letter.is_ascii_digit() || *letter == '.')
                        .collect();
                    let drawn = after.starts_with(|letter: char| letter.is_ascii_digit());
                    let hairline = digits
                        .parse::<f32>()
                        .is_ok_and(|value| HAIRLINES.contains(&value));
                    if drawn && !hairline {
                        found.push(format!("{call}{digits}))"));
                    }
                    rest = after;
                }
            }
        }

        found
    }

    #[test]
    fn a_view_draws_no_height_in_a_bare_number_of_pixels_unless_it_is_fixed_by_design() {
        let mut read = Vec::new();
        view_sources(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("src")
                .join("views")
                .as_path(),
            &mut read,
        );
        assert!(
            read.len() > 10,
            "the walk found {} view files, so it is not reading the views at all",
            read.len()
        );

        let mut unlisted = Vec::new();
        let mut used: Vec<(&str, String)> = Vec::new();
        for (name, source) in &read {
            for height in literal_heights(source) {
                if FIXED_BY_DESIGN
                    .iter()
                    .any(|(file, literal)| file == name && *literal == height)
                {
                    used.push((name, height));
                } else {
                    unlisted.push(format!("{name}: {height}"));
                }
            }
        }

        assert!(
            unlisted.is_empty(),
            "a pixel height that does not grow with the text size: {unlisted:?}"
        );
        for (file, literal) in FIXED_BY_DESIGN {
            assert!(
                used.iter()
                    .any(|(name, height)| *name == file && height == literal),
                "{file}: {literal} is allowed but no longer drawn, so it leaves the list"
            );
        }
    }

    #[test]
    fn nothing_is_drawn_where_the_palette_is_unmarked() {
        assert_eq!(tinted(UNMARKED, 0x00).a, 0.0);
    }
}
