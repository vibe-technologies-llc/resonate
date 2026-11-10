#![cfg(target_os = "linux")]

use std::{
    fs, thread,
    time::{Duration, Instant},
};

use resonate_providers::Opened;
use resonate_ytmusic::{Cookies, Tier, VideoId, interrupt_the_tools};

#[allow(dead_code)]
mod support;

use support::Fake;

const A_KILL_LANDS_WITHIN: Duration = Duration::from_secs(5);
const LOOKED_AT_EVERY: Duration = Duration::from_millis(10);
const A_PROCESS_ENDED_IS_MARKED: &str = " Z ";

fn has_ended(pid: &str) -> bool {
    fs::read_to_string(format!("/proc/{pid}/stat"))
        .map_or(true, |stat| stat.contains(A_PROCESS_ENDED_IS_MARKED))
}

fn pid_written_by(fake: &Fake) -> String {
    let file = fake.folder().join("pid");
    let began = Instant::now();
    loop {
        let pid = fs::read_to_string(&file).unwrap_or_default();
        if pid.ends_with('\n') {
            return pid.trim().to_owned();
        }
        assert!(
            began.elapsed() < A_KILL_LANDS_WITHIN,
            "the tool never began"
        );
        thread::sleep(LOOKED_AT_EVERY);
    }
}

#[test]
fn a_tool_interrupted_is_dead_and_the_next_one_still_starts() {
    let fake = Fake::answering(
        "if [ -e \"$here/pid\" ]; then exit 1; fi\necho $$ > \"$here/pid\"\nprintf 'bytes'\nexec sleep 300",
    );
    let tool = fake.tool(Cookies::None);
    let song = VideoId::read("abc").expect("a video id");

    let opened = tool.download(&song, Tier::Opus774).expect("a download");
    let pid = pid_written_by(&fake);
    assert!(!has_ended(&pid));

    interrupt_the_tools();

    let began = Instant::now();
    while !has_ended(&pid) {
        assert!(began.elapsed() < A_KILL_LANDS_WITHIN, "the tool lives on");
        thread::sleep(LOOKED_AT_EVERY);
    }
    assert!(matches!(opened, Opened::Reading { .. }));

    let _ = tool.formats(&song);

    assert_eq!(fake.calls().len(), 2, "a tool starts after an interruption");
}
