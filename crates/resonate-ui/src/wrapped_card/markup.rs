use super::{CARD_HEIGHT, CARD_MARGIN, CARD_WIDTH, CardColours};

pub(super) struct Said {
    pub(super) family: String,
    pub(super) eyebrow: String,
    pub(super) listened: String,
    pub(super) of_music: String,
    pub(super) across: String,
    pub(super) rows: String,
    pub(super) foot: String,
}

pub(super) fn row(y: u32, label: &str, name: &str, colours: &CardColours) -> String {
    format!(
        concat!(
            r##"<text x="{margin}" y="{y}" font-size="28" font-weight="600" "##,
            r##"fill="#{muted:06x}" letter-spacing="2">{label}</text>"##,
            r##"<text x="{margin}" y="{name_y}" font-size="50" font-weight="600" "##,
            r##"fill="#{text:06x}">{name}</text>"##,
        ),
        margin = CARD_MARGIN,
        y = y,
        name_y = y + 62,
        muted = colours.muted,
        text = colours.text,
        label = label,
        name = name,
    )
}

pub(super) fn card(said: &Said, colours: &CardColours) -> String {
    format!(
        concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" "##,
            r##"viewBox="0 0 {width} {height}" font-family="{family}, sans-serif">"##,
            r##"<rect width="{width}" height="{height}" fill="#{ground:06x}"/>"##,
            r##"<rect width="{width}" height="18" fill="#{accent:06x}"/>"##,
            r##"<text x="{margin}" y="170" font-size="32" font-weight="700" "##,
            r##"fill="#{accent:06x}" letter-spacing="3">{eyebrow}</text>"##,
            r##"<text x="{margin}" y="320" font-size="118" font-weight="800" "##,
            r##"fill="#{text:06x}">{listened}</text>"##,
            r##"<text x="{margin}" y="392" font-size="52" font-weight="600" "##,
            r##"fill="#{text:06x}">{of_music}</text>"##,
            r##"<text x="{margin}" y="462" font-size="32" fill="#{muted:06x}">{across}</text>"##,
            r##"<rect x="{margin}" y="508" width="{rule}" height="2" fill="#{raised:06x}"/>"##,
            "{rows}",
            r##"<text x="{margin}" y="1210" font-size="30" fill="#{muted:06x}">{foot}</text>"##,
            r##"<text x="{margin}" y="1278" font-size="34" font-weight="700" "##,
            r##"fill="#{accent:06x}">Resonate</text>"##,
            "</svg>"
        ),
        width = CARD_WIDTH,
        height = CARD_HEIGHT,
        margin = CARD_MARGIN,
        rule = CARD_WIDTH - 2 * CARD_MARGIN,
        family = said.family,
        ground = colours.ground,
        accent = colours.accent,
        raised = colours.raised,
        text = colours.text,
        muted = colours.muted,
        eyebrow = said.eyebrow,
        listened = said.listened,
        of_music = said.of_music,
        across = said.across,
        rows = said.rows,
        foot = said.foot,
    )
}
