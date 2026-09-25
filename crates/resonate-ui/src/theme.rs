use std::sync::{Arc, LazyLock};

use gpui::{Font, FontFeatures, FontStyle, FontWeight, Pixels, Rgba, SharedString, Size, px, rgba};
use parking_lot::RwLock;
use resonate_core::{Accent, Appearance, Theme};

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

    fn ramped() -> Self {
        let lit = |hue: f32| hsl(hue, RAMPED_ACCENT_SATURATION, RAMPED_ACCENT_LIGHTNESS);

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
}

const MAUVE_HUE: f32 = 0.762;
const BLUE_HUE: f32 = 0.578;
const TEAL_HUE: f32 = 0.487;
const GREEN_HUE: f32 = 0.395;
const AMBER_HUE: f32 = 0.108;
const PEACH_HUE: f32 = 0.055;
const RED_HUE: f32 = 0.985;

const RAMPED_ACCENT_SATURATION: f32 = 0.70;
const RAMPED_ACCENT_LIGHTNESS: f32 = 0.62;

const GROUND: f32 = 0.082;
const BELOW_THE_GROUND: f32 = 0.056;
const LIFTED: f32 = 0.112;
const UNDER_THE_POINTER: f32 = 0.152;
const HAIRLINE: f32 = 0.135;
const DRAWN_EDGE: f32 = 0.225;
const BODY_INK: f32 = 0.935;
const SECOND_INK: f32 = 0.660;
const THIRD_INK: f32 = 0.455;
const PITCH: f32 = 0.040;
const PAPER: f32 = 0.968;
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
    let surface = |lightness: f32| hsl(recipe.hue, recipe.chroma, lightness);
    let ink = |lightness: f32| hsl(recipe.hue, recipe.chroma * INK_CHROMA, lightness);
    let pitch = surface(PITCH);

    Flavour {
        native: recipe.native,
        background: surface(GROUND),
        surface: surface(BELOW_THE_GROUND),
        raised: surface(LIFTED),
        hover: surface(UNDER_THE_POINTER),
        border: surface(HAIRLINE),
        outline: surface(DRAWN_EDGE),
        text: ink(BODY_INK),
        muted: ink(SECOND_INK),
        faint: ink(THIRD_INK),
        pitch,
        paper: ink(PAPER),
        scrim: (pitch << 8) | SCRIM_ALPHA,
        alarm: hsl(recipe.alarm, 0.62, 0.55),
        accents: Accents::ramped(),
    }
}

static RESONATE: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Mauve,
    background: 0x0c0c0e,
    surface: 0x121215,
    raised: 0x1a1a1f,
    hover: 0x222228,
    border: 0x222227,
    outline: 0x31313a,
    text: 0xf1efe9,
    muted: 0x9f9d97,
    faint: 0x64625d,
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
    })
});

static GRAPHITE: LazyLock<Flavour> = LazyLock::new(|| {
    ramp(&Recipe {
        native: Accent::Blue,
        hue: 0.0,
        chroma: 0.0,
        alarm: RED_HUE,
    })
});

static AMOLED: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Mauve,
    background: 0x000000,
    surface: 0x000000,
    raised: 0x121212,
    hover: 0x1d1d1d,
    border: 0x1a1a1a,
    outline: 0x2e2e2e,
    text: 0xf2f2f2,
    muted: 0xa3a3a3,
    faint: 0x686868,
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
    })
});

static ROSE_PINE: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Mauve,
    background: 0x1f1d2e,
    surface: 0x191724,
    raised: 0x26233a,
    hover: 0x403d52,
    border: 0x2a2739,
    outline: 0x4c4864,
    text: 0xe0def4,
    muted: 0xa8a3c4,
    faint: 0x807b9c,
    pitch: 0x16141f,
    paper: 0xfaf4ed,
    scrim: 0x191724ea,
    alarm: 0xeb6f92,
    accents: Accents {
        mauve: 0xc4a7e7,
        blue: 0x6fb3ce,
        teal: 0x9ccfd8,
        green: 0x81cf9e,
        amber: 0xf6c177,
        peach: 0xebbcba,
        red: 0xeb6f92,
    },
});

static ROSE_PINE_MOON: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Mauve,
    background: 0x2a273f,
    surface: 0x232136,
    raised: 0x393552,
    hover: 0x44415a,
    border: 0x36324d,
    outline: 0x56526e,
    text: 0xe0def4,
    muted: 0xaba6c8,
    faint: 0x8983a3,
    pitch: 0x1d1b2e,
    paper: 0xfaf4ed,
    scrim: 0x232136ea,
    alarm: 0xeb6f92,
    accents: Accents {
        mauve: 0xc4a7e7,
        blue: 0x69b5d8,
        teal: 0x9ccfd8,
        green: 0x81cf9e,
        amber: 0xf6c177,
        peach: 0xea9a97,
        red: 0xeb6f92,
    },
});

static CATPPUCCIN_MOCHA: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Mauve,
    background: 0x1e1e2e,
    surface: 0x181825,
    raised: 0x313244,
    hover: 0x45475a,
    border: 0x313244,
    outline: 0x585b70,
    text: 0xcdd6f4,
    muted: 0x9399b2,
    faint: 0x6c7086,
    pitch: 0x11111b,
    paper: 0xeff1f5,
    scrim: 0x11111bea,
    alarm: 0xf38ba8,
    accents: Accents {
        mauve: 0xcba6f7,
        blue: 0x89b4fa,
        teal: 0x94e2d5,
        green: 0xa6e3a1,
        amber: 0xf9e2af,
        peach: 0xfab387,
        red: 0xf38ba8,
    },
});

static CATPPUCCIN_MACCHIATO: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Mauve,
    background: 0x24273a,
    surface: 0x1e2030,
    raised: 0x363a4f,
    hover: 0x494d64,
    border: 0x363a4f,
    outline: 0x5b6078,
    text: 0xcad3f5,
    muted: 0x939ab7,
    faint: 0x6e738d,
    pitch: 0x181926,
    paper: 0xeff1f5,
    scrim: 0x181926ea,
    alarm: 0xed8796,
    accents: Accents {
        mauve: 0xc6a0f6,
        blue: 0x8aadf4,
        teal: 0x8bd5ca,
        green: 0xa6da95,
        amber: 0xeed49f,
        peach: 0xf5a97f,
        red: 0xed8796,
    },
});

static CATPPUCCIN_FRAPPE: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Mauve,
    background: 0x303446,
    surface: 0x292c3c,
    raised: 0x414559,
    hover: 0x51576d,
    border: 0x414559,
    outline: 0x626880,
    text: 0xc6d0f5,
    muted: 0x949cbb,
    faint: 0x737994,
    pitch: 0x232634,
    paper: 0xeff1f5,
    scrim: 0x232634ea,
    alarm: 0xe78284,
    accents: Accents {
        mauve: 0xca9ee6,
        blue: 0x8caaee,
        teal: 0x81c8be,
        green: 0xa6d189,
        amber: 0xe5c890,
        peach: 0xef9f76,
        red: 0xe78284,
    },
});

static NORD: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Blue,
    background: 0x2e3440,
    surface: 0x29303c,
    raised: 0x3b4252,
    hover: 0x434c5e,
    border: 0x3b4252,
    outline: 0x4c566a,
    text: 0xeceff4,
    muted: 0xa7b1c2,
    faint: 0x77829a,
    pitch: 0x242933,
    paper: 0xeceff4,
    scrim: 0x242933ea,
    alarm: 0xbf616a,
    accents: Accents {
        mauve: 0xb48ead,
        blue: 0x81a1c1,
        teal: 0x8fbcbb,
        green: 0xa3be8c,
        amber: 0xebcb8b,
        peach: 0xd08770,
        red: 0xc87d86,
    },
});

static GRUVBOX_DARK: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Amber,
    background: 0x282828,
    surface: 0x1d2021,
    raised: 0x3c3836,
    hover: 0x504945,
    border: 0x3c3836,
    outline: 0x665c54,
    text: 0xebdbb2,
    muted: 0xbdae93,
    faint: 0x928374,
    pitch: 0x16181a,
    paper: 0xfbf1c7,
    scrim: 0x1d2021ea,
    alarm: 0xcc241d,
    accents: Accents {
        mauve: 0xd3869b,
        blue: 0x83a598,
        teal: 0x8ec07c,
        green: 0xb8bb26,
        amber: 0xfabd2f,
        peach: 0xfe8019,
        red: 0xfb4934,
    },
});

static TOKYO_NIGHT: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Blue,
    background: 0x1a1b26,
    surface: 0x16161e,
    raised: 0x24283b,
    hover: 0x292e42,
    border: 0x232433,
    outline: 0x3b4261,
    text: 0xc0caf5,
    muted: 0xa9b1d6,
    faint: 0x737aa2,
    pitch: 0x101014,
    paper: 0xd5d6db,
    scrim: 0x16161eea,
    alarm: 0xf7768e,
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
    outline: 0x4d5066,
    text: 0xf8f8f2,
    muted: 0xa9abc2,
    faint: 0x6272a4,
    pitch: 0x191a21,
    paper: 0xf8f8f2,
    scrim: 0x191a21ea,
    alarm: 0xff5555,
    accents: Accents {
        mauve: 0xff79c6,
        blue: 0xbd93f9,
        teal: 0x8be9fd,
        green: 0x50fa7b,
        amber: 0xf1fa8c,
        peach: 0xffb86c,
        red: 0xff5555,
    },
});

static ONE_DARK: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Blue,
    background: 0x282c34,
    surface: 0x21252b,
    raised: 0x2c313a,
    hover: 0x3e4451,
    border: 0x2c313a,
    outline: 0x4b5263,
    text: 0xd7dae0,
    muted: 0xabb2bf,
    faint: 0x5c6370,
    pitch: 0x181a1f,
    paper: 0xd7dae0,
    scrim: 0x181a1fea,
    alarm: 0xe06c75,
    accents: Accents {
        mauve: 0xc678dd,
        blue: 0x61afef,
        teal: 0x56b6c2,
        green: 0x98c379,
        amber: 0xe5c07b,
        peach: 0xd19a66,
        red: 0xe06c75,
    },
});

static SOLARIZED_DARK: LazyLock<Flavour> = LazyLock::new(|| Flavour {
    native: Accent::Blue,
    background: 0x002b36,
    surface: 0x00212b,
    raised: 0x073642,
    hover: 0x0a4251,
    border: 0x073642,
    outline: 0x586e75,
    text: 0xeee8d5,
    muted: 0x93a1a1,
    faint: 0x657b83,
    pitch: 0x000000,
    paper: 0xfdf6e3,
    scrim: 0x001b22ea,
    alarm: 0xdc322f,
    accents: Accents {
        mauve: 0x6c71c4,
        blue: 0x268bd2,
        teal: 0x2aa198,
        green: 0x859900,
        amber: 0xb58900,
        peach: 0xcb4b16,
        red: 0xdc322f,
    },
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
    }
}

struct Worn {
    appearance: Appearance,
    flavour: &'static Flavour,
    accent: u32,
    scale: f32,
}

impl Worn {
    fn of(appearance: Appearance) -> Self {
        let flavour = flavour(appearance.theme);

        Self {
            appearance,
            flavour,
            accent: flavour.accent(appearance.accent),
            scale: appearance.text_size.scale(),
        }
    }
}

static WORN: LazyLock<RwLock<Worn>> = LazyLock::new(|| RwLock::new(Worn::of(Appearance::DEFAULT)));

pub fn wear(appearance: Appearance) {
    *WORN.write() = Worn::of(appearance);
}

pub fn worn() -> Appearance {
    WORN.read().appearance
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

pub(crate) fn ink_over(colour: u32) -> u32 {
    let worn = WORN.read();

    reads_better_on(colour, worn.flavour.pitch, worn.flavour.paper)
}

pub(crate) fn accent_ink() -> u32 {
    ink_over(accent())
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

const SELECTION_ALPHA: u8 = 0x40;

pub const UNMARKED: u32 = 0x00000000;

const TEXT_XS: f32 = 11.0;
const TEXT_SM: f32 = 12.5;
const TEXT_BASE: f32 = 13.5;
const TEXT_LG: f32 = 16.0;
const TEXT_XL: f32 = 21.0;
const TEXT_TITLE: f32 = 26.0;
const TEXT_LYRIC: f32 = 26.0;
const TEXT_LYRIC_LEAD: f32 = 36.0;

pub const WINDOW_MIN_WIDTH: f32 = 720.0;
pub const WINDOW_MIN_HEIGHT: f32 = 520.0;

const HEADER_HEIGHT: f32 = 52.0;
const SIDEBAR_WIDTH: f32 = 216.0;
const SEARCH_WIDTH: f32 = 560.0;
const TRANSPORT_HEIGHT: f32 = 104.0;
const TRANSPORT_CENTRE: f32 = 460.0;
const TRANSPORT_STEP: Control = Control::of(36.0, 18.0);
const TRANSPORT_PLAY: Control = Control::of(44.0, 20.0);
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
const LISTEN_COVER: f32 = 112.0;
const CLOCK_WIDTH: f32 = 44.0;
const NOW_PLAYING_COVER: f32 = 64.0;
const GRID_COVER: f32 = 164.0;
const SHELF_COVER: f32 = 104.0;
const GRID_GAP: f32 = 20.0;
const GRID_ROW: f32 = 230.0;
const SCOPE_COVER: f32 = 148.0;
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
const CLEAR: Control = Control::of(20.0, 12.0);
const FILTER_CLEAR: Control = Control::of(20.0, 14.0);
const HINT: Control = Control::of(20.0, 14.0);
const CHECK_MARK: f32 = 14.0;
const HINT_WIDTH: f32 = 300.0;
const MENU_WIDTH: f32 = 236.0;
const MENU_ROW: f32 = 28.0;
const MENU_ICON: f32 = 13.0;
const GHOST_WIDTH: f32 = 260.0;
const ROOT_ICON: f32 = 14.0;
const SETTINGS_COLUMN: f32 = 680.0;
const SETTINGS_RAIL: f32 = 188.0;
const LYRIC_COLUMN: f32 = 720.0;
const LYRIC_GUTTER: f32 = 32.0;
const LYRIC_REACH: f32 = 180.0;
const LYRIC_PAD: f32 = 16.0;
const LYRIC_BREAK: f32 = 18.0;
const LYRIC_EDGE: f32 = 88.0;
const LYRIC_DOT: f32 = 7.0;
const LYRIC_DOT_GAP: f32 = 10.0;
const VOLUME_WIDTH: f32 = 88.0;
const AVATAR: f32 = 34.0;
const STAGE_WIDTH: f32 = 200.0;
const STAT_TILE: f32 = 150.0;
const CHART_HEIGHT: f32 = 132.0;
const SUGGESTION_CARD: f32 = 268.0;
const EMPTY_ICON: f32 = 28.0;
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
pub const RAIL_HEIGHT: f32 = 16.0;
pub const RAIL_TRACK: f32 = 4.0;
pub const CHART_BAR: f32 = 3.0;
pub const CHART_BAR_GAP: f32 = 2.0;
pub const CHART_BASELINE: f32 = 2.0;
pub const RAIL_THUMB: f32 = 12.0;
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
    text_lyric_lead => TEXT_LYRIC_LEAD,
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
    listen_cover => LISTEN_COVER,
    clock_width => CLOCK_WIDTH,
    now_playing_cover => NOW_PLAYING_COVER,
    grid_cover => GRID_COVER,
    shelf_cover => SHELF_COVER,
    grid_gap => GRID_GAP,
    grid_row => GRID_ROW,
    scope_cover => SCOPE_COVER,
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
    menu_row => MENU_ROW,
    menu_icon => MENU_ICON,
    ghost_width => GHOST_WIDTH,
    root_icon => ROOT_ICON,
    settings_column => SETTINGS_COLUMN,
    settings_rail => SETTINGS_RAIL,
    lyric_column => LYRIC_COLUMN,
    lyric_gutter => LYRIC_GUTTER,
    lyric_reach => LYRIC_REACH,
    lyric_pad => LYRIC_PAD,
    lyric_break => LYRIC_BREAK,
    lyric_edge => LYRIC_EDGE,
    lyric_dot => LYRIC_DOT,
    lyric_dot_gap => LYRIC_DOT_GAP,
    volume_width => VOLUME_WIDTH,
    avatar => AVATAR,
    stage_width => STAGE_WIDTH,
    stat_tile => STAT_TILE,
    chart_height => CHART_HEIGHT,
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
    use resonate_core::TextSize;

    use super::*;

    const READS_AS_BODY: f32 = 4.5;
    const READS_AT_REST: f32 = 7.0;
    const STANDS_APART: f32 = 3.0;

    fn every_appearance() -> impl Iterator<Item = Appearance> {
        Theme::ALL.into_iter().flat_map(|theme| {
            [None]
                .into_iter()
                .chain(Accent::ALL.map(Some))
                .map(move |accent| Appearance {
                    theme,
                    accent,
                    text_size: TextSize::Medium,
                })
        })
    }

    #[test]
    fn every_theme_is_dark_enough_to_read_its_own_text_on() {
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

            let read = contrast(flavour.muted, flavour.background);
            assert!(
                read >= READS_AS_BODY,
                "{theme}: muted text reads at {read:.1}"
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
                text_size: TextSize::Medium,
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
        let read = |size: TextSize| {
            wear(Appearance {
                text_size: size,
                ..Appearance::DEFAULT
            });
            (text_base(), row_height(), sidebar_width(), settings_rail())
        };

        let small = read(TextSize::Small);
        let medium = read(TextSize::Medium);
        let large = read(TextSize::Large);

        assert!(small.0 < medium.0 && medium.0 < large.0);
        assert!(small.1 < medium.1 && medium.1 < large.1);
        assert!(small.2 < medium.2 && medium.2 < large.2);
        assert!(small.3 < medium.3 && medium.3 < large.3);
        assert_eq!(medium.0, TEXT_BASE);

        wear(Appearance::DEFAULT);
    }

    #[test]
    fn nothing_is_drawn_where_the_palette_is_unmarked() {
        assert_eq!(tinted(UNMARKED, 0x00).a, 0.0);
    }
}
