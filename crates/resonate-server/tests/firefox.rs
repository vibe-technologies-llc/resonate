use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

const FIREFOX: &str = "firefox";
const STARTS_WITHIN: Duration = Duration::from_secs(60);
const LOOKED_AT_EVERY: Duration = Duration::from_millis(100);
const QUITS_WITHIN: Duration = Duration::from_secs(20);
const PORT_FILE: &str = "MarionetteActivePort";
const SYSTEM_ACCESS: &str = "MOZ_REMOTE_ALLOW_SYSTEM_ACCESS";
const A_COMMAND: u8 = 0;
const AN_ANSWER: u64 = 1;
const PREFERENCES: [(&str, &str); 4] = [
    ("marionette.port", "0"),
    ("browser.chrome.site_icons", "false"),
    ("browser.shell.checkDefaultBrowser", "false"),
    ("browser.startup.homepage_override.mstone", "\"ignore\""),
];
const POLICY_REPORTED_AS: &str = "Content-Security-Policy";
const VIOLATIONS: &str = "return Services.console.getMessageArray().map((message) => message.message || String(message)).filter((said) => said.includes(arguments[0]));";

struct Started(Child);

impl Drop for Started {
    fn drop(&mut self) {
        let deadline = Instant::now() + QUITS_WITHIN;
        while Instant::now() < deadline {
            if matches!(self.0.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(LOOKED_AT_EVERY);
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub struct Firefox {
    _started: Started,
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    asked: u64,
}

impl Firefox {
    pub fn installed() -> bool {
        Command::new(FIREFOX)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    pub fn headless(profile: &Path) -> Self {
        fs::create_dir_all(profile).expect("a profile folder");
        let preferences = PREFERENCES
            .iter()
            .map(|(name, value)| format!("user_pref(\"{name}\", {value});\n"))
            .collect::<String>();
        fs::write(profile.join("user.js"), preferences).expect("the preferences written");

        let started = Started(
            Command::new(FIREFOX)
                .args(["--headless", "--marionette", "--no-remote", "--profile"])
                .arg(profile)
                .env(SYSTEM_ACCESS, "1")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("firefox started"),
        );
        let port = port_written_in(profile);
        let writer = TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
            .expect("marionette reached");
        let reader = BufReader::new(writer.try_clone().expect("a second handle"));
        let mut firefox = Self {
            _started: started,
            reader,
            writer,
            asked: 0,
        };

        firefox.heard();
        firefox.command("WebDriver:NewSession", &json!({}));
        firefox
    }

    pub fn open(&mut self, url: &str) {
        self.command("WebDriver:Navigate", &json!({ "url": url }));
    }

    pub fn page_says(&mut self, script: &str) -> Value {
        self.command(
            "WebDriver:ExecuteScript",
            &json!({ "script": script, "args": [] }),
        )
    }

    pub fn until(&mut self, what: &str, script: &str, done: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + STARTS_WITHIN;
        loop {
            let said = self.page_says(script);
            if done(&said) {
                return said;
            }
            assert!(Instant::now() < deadline, "{what} never happened: {said}");
            thread::sleep(LOOKED_AT_EVERY);
        }
    }

    pub fn policy_violations(&mut self) -> Vec<String> {
        self.command("Marionette:SetContext", &json!({ "value": "chrome" }));
        let violations = self.command(
            "WebDriver:ExecuteScript",
            &json!({ "script": VIOLATIONS, "args": [POLICY_REPORTED_AS] }),
        );
        self.command("Marionette:SetContext", &json!({ "value": "content" }));

        violations
            .as_array()
            .expect("the console's messages")
            .iter()
            .filter_map(|said| said.as_str().map(str::to_owned))
            .collect()
    }

    fn command(&mut self, name: &str, parameters: &Value) -> Value {
        self.asked += 1;
        let body = json!([A_COMMAND, self.asked, name, parameters]).to_string();
        write!(self.writer, "{}:{body}", body.len()).expect("asked marionette");

        let answer = self.heard();
        assert_eq!(answer[0], AN_ANSWER, "{answer}");
        assert_eq!(answer[1], self.asked, "{answer}");
        assert!(answer[2].is_null(), "{name} failed: {}", answer[2]);
        answer[3]
            .get("value")
            .cloned()
            .unwrap_or_else(|| answer[3].clone())
    }

    fn heard(&mut self) -> Value {
        let mut length = Vec::new();
        self.reader
            .read_until(b':', &mut length)
            .expect("marionette answered");
        length.pop();
        let length = std::str::from_utf8(&length)
            .ok()
            .and_then(|length| length.parse::<usize>().ok())
            .expect("a length");
        let mut body = vec![0; length];
        self.reader.read_exact(&mut body).expect("the whole answer");

        serde_json::from_slice(&body).expect("an answer in JSON")
    }
}

impl Drop for Firefox {
    fn drop(&mut self) {
        self.asked += 1;
        let body = json!([A_COMMAND, self.asked, "Marionette:Quit", {}]).to_string();
        let _ = write!(self.writer, "{}:{body}", body.len());
    }
}

fn port_written_in(profile: &Path) -> u16 {
    let deadline = Instant::now() + STARTS_WITHIN;
    loop {
        let written = fs::read_to_string(profile.join(PORT_FILE)).unwrap_or_default();
        if let Ok(port) = written.trim().parse() {
            return port;
        }
        assert!(Instant::now() < deadline, "firefox never opened marionette");
        thread::sleep(LOOKED_AT_EVERY);
    }
}
