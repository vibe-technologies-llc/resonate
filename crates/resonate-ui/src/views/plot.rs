use gpui::{Background, Bounds, Pixels, Point, Window, fill, point, px};

const HALF: f32 = 0.5;

pub(crate) fn stroke(
    window: &mut Window,
    points: &[Point<Pixels>],
    width: f32,
    colour: impl Into<Background>,
) {
    let colour = colour.into();
    let half = px(width * HALF);
    for pair in points.windows(2) {
        let [from, to] = pair else {
            continue;
        };
        let (left, right) = widened(from.x.min(to.x), from.x.max(to.x), px(width));
        window.paint_quad(fill(
            Bounds::from_corners(
                point(left, from.y.min(to.y) - half),
                point(right, from.y.max(to.y) + half),
            ),
            colour,
        ));
    }
}

pub(crate) fn wash(
    window: &mut Window,
    points: &[Point<Pixels>],
    level: Pixels,
    colour: impl Into<Background>,
) {
    let colour = colour.into();
    for pair in points.windows(2) {
        let [from, to] = pair else {
            continue;
        };
        let reached = (from.y + to.y) * HALF;
        if reached == level {
            continue;
        }
        window.paint_quad(fill(
            Bounds::from_corners(
                point(from.x.min(to.x), reached.min(level)),
                point(from.x.max(to.x), reached.max(level)),
            ),
            colour,
        ));
    }
}

pub(crate) fn between(
    window: &mut Window,
    upper: &[Point<Pixels>],
    lower: &[Point<Pixels>],
    colour: impl Into<Background>,
) {
    let colour = colour.into();
    for (over, under) in upper.windows(2).zip(lower.windows(2)) {
        let ([over_from, over_to], [under_from, under_to]) = (over, under) else {
            continue;
        };
        let top = over_from.y.min(over_to.y);
        let bottom = under_from.y.max(under_to.y);
        let (left, right) = widened(over_from.x, over_to.x, px(1.0));
        window.paint_quad(fill(
            Bounds::from_corners(point(left, top.min(bottom)), point(right, top.max(bottom))),
            colour,
        ));
    }
}

fn widened(left: Pixels, right: Pixels, at_least: Pixels) -> (Pixels, Pixels) {
    let short = at_least - (right - left);
    if short <= px(0.0) {
        return (left, right);
    }
    let each = short * HALF;
    (left - each, right + each)
}
