use std::{
    io::{self, BufRead},
    thread,
    time::Duration,
};

use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
    protocol::wl_registry,
};
use wayland_protocols_plasma::fake_input::client::org_kde_kwin_fake_input::OrgKdeKwinFakeInput;

const LEFT_BUTTON: u32 = 0x110;
const FAKE_INPUT_VERSIONS: std::ops::RangeInclusive<u32> = 1..=5;
const STEP_EVERY: Duration = Duration::from_millis(20);

struct State;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<OrgKdeKwinFakeInput, ()> for State {
    fn event(
        _: &mut Self,
        _: &OrgKdeKwinFakeInput,
        _: <OrgKdeKwinFakeInput as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

fn number(word: &str) -> f64 {
    word.parse().expect("a number")
}

fn main() {
    let connection = Connection::connect_to_env().expect("a compositor to connect to");
    let (globals, mut queue) = registry_queue_init::<State>(&connection).expect("the globals");
    let fake: OrgKdeKwinFakeInput = globals
        .bind(&queue.handle(), FAKE_INPUT_VERSIONS, ())
        .expect("a compositor offering org_kde_kwin_fake_input");
    fake.authenticate("resonate-drive".into(), "a drag onto the window".into());
    let mut state = State;
    queue.roundtrip(&mut state).expect("the authentication");
    println!("fakein: ready");

    for line in io::stdin().lock().lines() {
        let line = line.expect("a line");
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["move", x, y] => fake.pointer_motion_absolute(number(x), number(y)),
            ["down"] => fake.button(LEFT_BUTTON, 1),
            ["up"] => fake.button(LEFT_BUTTON, 0),
            ["sleep", seconds] => thread::sleep(Duration::from_secs_f64(number(seconds))),
            ["drag", x0, y0, x1, y1, steps] => {
                let (from, to) = ((number(x0), number(y0)), (number(x1), number(y1)));
                let steps = number(steps) as u32;
                for step in 1..=steps {
                    let share = f64::from(step) / f64::from(steps);
                    fake.pointer_motion_absolute(
                        from.0 + (to.0 - from.0) * share,
                        from.1 + (to.1 - from.1) * share,
                    );
                    connection.flush().expect("the flush");
                    thread::sleep(STEP_EVERY);
                }
            }
            ["quit"] => break,
            _ => {}
        }
        connection.flush().expect("the flush");
        queue.roundtrip(&mut state).expect("the roundtrip");
        println!("fakein: done {line}");
    }
}
