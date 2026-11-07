use std::{
    env, fs,
    io::ErrorKind,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{self, Command},
    sync::atomic::{AtomicU32, Ordering},
    thread,
    time::{Duration, Instant},
};

use resonate_ytmusic::{Cookies, Tool};

const EXECUTABLE: u32 = 0o755;
const FORKS_HOLDING_THE_SCRIPT_OPEN_END_WITHIN: Duration = Duration::from_secs(5);
const A_CALL_ENDS_WITH: &str = "--end-of-call--";
const ASKED_IF_IT_RUNS: &str = "--does-it-run";

static MADE: AtomicU32 = AtomicU32::new(0);

fn runs_once_no_fork_holds_it_open(program: &Path) {
    let began = Instant::now();
    loop {
        match Command::new(program).arg(ASKED_IF_IT_RUNS).status() {
            Err(error) if error.kind() == ErrorKind::ExecutableFileBusy => {
                assert!(began.elapsed() < FORKS_HOLDING_THE_SCRIPT_OPEN_END_WITHIN);
                thread::yield_now();
            }
            answered => {
                answered.expect("the fake tool runs");
                return;
            }
        }
    }
}

pub struct Fake {
    folder: PathBuf,
}

impl Fake {
    pub fn answering(body: &str) -> Self {
        let folder = env::temp_dir().join(format!(
            "resonate-ytmusic-{}-{}",
            process::id(),
            MADE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&folder).expect("a folder for the fake tool");
        let program = folder.join("yt-dlp");
        let script = format!(
            "#!/bin/sh\n[ \"$1\" = {ASKED_IF_IT_RUNS} ] && exit 0\nhere=$(dirname \"$0\")\nprintf '%s\\n' \"$@\" {A_CALL_ENDS_WITH} >> \"$here/asked\"\n{body}\n"
        );
        fs::write(&program, script).expect("the fake tool");
        fs::set_permissions(&program, fs::Permissions::from_mode(EXECUTABLE))
            .expect("the fake tool made executable");
        runs_once_no_fork_holds_it_open(&program);
        Self { folder }
    }

    pub fn listing(formats: &[(&str, u64)]) -> Self {
        let listed: Vec<String> = formats
            .iter()
            .map(|(id, size)| format!(r#"{{"format_id":"{id}","filesize":{size}}}"#))
            .collect();
        Self::answering(&format!(
            "echo '{{\"id\":\"x\",\"formats\":[{}]}}'",
            listed.join(",")
        ))
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    pub fn tool(&self, cookies: Cookies) -> Tool {
        Tool::new(Some(self.folder.join("yt-dlp")), cookies)
    }

    pub fn calls(&self) -> Vec<Vec<String>> {
        let asked = fs::read_to_string(self.folder.join("asked")).unwrap_or_default();
        let mut calls = Vec::new();
        let mut call = Vec::new();
        for argument in asked.lines() {
            if argument == A_CALL_ENDS_WITH {
                calls.push(std::mem::take(&mut call));
            } else {
                call.push(argument.to_owned());
            }
        }
        calls
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.folder);
    }
}
