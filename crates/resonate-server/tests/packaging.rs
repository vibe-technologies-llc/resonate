use std::{fs, path::PathBuf};

const UNIT: &str = "server/resonate-server.service";
const HAND_OVER: &str = "server/hand-over-credentials.sh";
const INSTALLED_HAND_OVER: &str = "/usr/lib/resonate-server/hand-over-credentials";
const ARCH_HOOK: &str = "server/resonate-server.hook";
const ARCH_RECIPES: [&str; 2] = ["server/PKGBUILD", "release/server/PKGBUILD"];
const SPEC: &str = "server/resonate-server.spec";
const DEB_SCRIPT: &str = "deb/build.sh";
const DEB_MAINTAINER_SCRIPTS: [&str; 3] = ["postinst", "prerm", "postrm"];

fn packaging_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging")
        .join(name)
}

fn packaging(name: &str) -> String {
    let path = packaging_path(name);

    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn unit_value<'a>(unit: &'a str, key: &str) -> Vec<&'a str> {
    unit.lines()
        .filter_map(|line| line.strip_prefix(key)?.strip_prefix('='))
        .collect()
}

#[test]
fn the_unit_hands_the_pair_over_before_it_starts_and_again_on_each_reload() {
    let unit = packaging(UNIT);
    let hand_over = packaging(HAND_OVER);

    let starts = unit_value(&unit, "ExecStart");
    let before = unit_value(&unit, "ExecStartPre");
    let reloads = unit_value(&unit, "ExecReload");

    assert_eq!(unit_value(&unit, "RuntimeDirectory"), ["resonate-server"]);
    assert_eq!(unit_value(&unit, "RuntimeDirectoryMode"), ["0700"]);
    assert!(
        hand_over.contains(":-/run/resonate-server}"),
        "the hand-over writes somewhere other than the unit's runtime directory"
    );
    assert!(
        unit_value(&unit, "LoadCredential").is_empty(),
        "a credential is a snapshot a reload never refreshes"
    );

    assert_eq!(before, [format!("+{INSTALLED_HAND_OVER}")]);
    assert_eq!(reloads, [format!("+{INSTALLED_HAND_OVER} $MAINPID")]);
    assert_eq!(starts.len(), 1);
    assert!(starts[0].contains("--certificate ${RUNTIME_DIRECTORY}/certificate"));
    assert!(starts[0].contains("--key ${RUNTIME_DIRECTORY}/key"));
}

#[test]
fn every_server_recipe_installs_the_hand_over_where_the_unit_runs_it() {
    let recipes = [ARCH_RECIPES[0], ARCH_RECIPES[1], SPEC, DEB_SCRIPT];

    for recipe in recipes {
        let text = packaging(recipe);

        assert!(
            text.contains("packaging/server/hand-over-credentials.sh"),
            "{recipe} does not install the hand-over"
        );
        assert!(
            text.contains("/lib/resonate-server/hand-over-credentials"),
            "{recipe} installs the hand-over somewhere the unit does not run it from"
        );
    }

    assert!(packaging(SPEC).contains("%{_prefix}/lib/resonate-server/hand-over-credentials"));
}

#[test]
fn an_rpm_or_arch_upgrade_restarts_a_running_server_and_an_install_starts_none() {
    let spec = packaging(SPEC);
    let hook = packaging(ARCH_HOOK);

    assert!(spec.contains("%systemd_postun_with_restart resonate-server.service"));
    assert!(spec.contains("%systemd_post resonate-server.service"));

    let operations: Vec<&str> = hook
        .lines()
        .filter_map(|line| line.strip_prefix("Operation = "))
        .collect();

    assert_eq!(operations, ["Upgrade"]);
    assert!(hook.contains("Target = usr/bin/resonate-server"));
    assert!(hook.contains("When = PostTransaction"));
    assert!(hook.contains(
        "Exec = /usr/share/libalpm/scripts/systemd-hook restart resonate-server.service"
    ));

    for recipe in ARCH_RECIPES {
        assert!(
            packaging(recipe).contains("usr/share/libalpm/hooks/70-resonate-server-restart.hook"),
            "{recipe} does not install the restart hook after systemd's daemon-reload"
        );
    }
}

#[test]
fn the_deb_installs_each_maintainer_script_it_keeps() {
    let build = packaging(DEB_SCRIPT);

    assert!(build.contains("for script in postinst prerm postrm; do"));
    assert!(build.contains("\"packaging/deb/server/$script.sh\" \"$stage/DEBIAN/$script\""));
    assert!(build.contains("init-system-helpers"));

    for script in DEB_MAINTAINER_SCRIPTS {
        packaging(&format!("deb/server/{script}.sh"));
    }
}

#[cfg(target_os = "linux")]
mod run {
    use std::{
        env, fs,
        os::unix::{
            fs::{MetadataExt, PermissionsExt},
            process::ExitStatusExt,
        },
        path::{Path, PathBuf},
        process::{Command, Output, Stdio},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{DEB_MAINTAINER_SCRIPTS, HAND_OVER, packaging_path};

    const READ_BY_ITS_OWNER_ALONE: u32 = 0o400;
    const PERMISSION_BITS: u32 = 0o7777;
    const HANGUP: i32 = 1;

    fn scratch(name: &str) -> PathBuf {
        let folder = env::temp_dir().join(format!(
            "resonate-server-packaging-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));

        fs::create_dir_all(&folder).expect("a scratch folder");
        folder
    }

    fn handed_over(from: &Path, to: &Path, signalled: Option<u32>) -> Output {
        let mut command = Command::new("sh");

        command
            .arg(packaging_path(HAND_OVER))
            .env("RESONATE_SERVER_CREDENTIALS_FROM", from)
            .env("RESONATE_SERVER_CREDENTIALS_TO", to);
        if let Some(pid) = signalled {
            command.arg(pid.to_string());
        }
        command.output().expect("the hand-over ran")
    }

    fn own_uid(folder: &Path) -> u32 {
        fs::metadata(folder).expect("the folder").uid()
    }

    #[test]
    fn the_hand_over_copies_the_pair_for_the_runtime_directorys_owner_alone() {
        let folder = scratch("hand-over");
        let from = folder.join("etc");
        let to = folder.join("run");

        fs::create_dir_all(&from).expect("a source");
        fs::create_dir_all(&to).expect("a runtime directory");
        fs::write(from.join("fullchain.pem"), "first chain").expect("written");
        fs::write(from.join("privkey.pem"), "first key").expect("written");

        let output = handed_over(&from, &to, None);

        assert!(output.status.success(), "{output:?}");

        for (copy, holds) in [("certificate", "first chain"), ("key", "first key")] {
            let path = to.join(copy);
            let metadata = fs::metadata(&path).expect("handed over");

            assert_eq!(fs::read_to_string(&path).expect("read"), holds);
            assert_eq!(
                metadata.permissions().mode() & PERMISSION_BITS,
                READ_BY_ITS_OWNER_ALONE
            );
            assert_eq!(metadata.uid(), own_uid(&to));
        }

        assert_eq!(
            fs::read_dir(&to).expect("listed").count(),
            2,
            "the hand-over left a staged copy behind"
        );
    }

    #[test]
    fn a_reload_hangs_up_on_the_server_only_once_the_pair_is_handed_over() {
        let folder = scratch("hangup");
        let from = folder.join("etc");
        let to = folder.join("run");

        fs::create_dir_all(&from).expect("a source");
        fs::create_dir_all(&to).expect("a runtime directory");
        fs::write(from.join("fullchain.pem"), "first chain").expect("written");
        fs::write(from.join("privkey.pem"), "first key").expect("written");

        assert!(handed_over(&from, &to, None).status.success());

        let mut kept = Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .spawn()
            .expect("a stand-in server");

        fs::remove_file(from.join("fullchain.pem")).expect("the renewal lost");

        let refused = handed_over(&from, &to, Some(kept.id()));
        let kept_running = kept.try_wait().expect("asked").is_none();
        let previous = fs::read_to_string(to.join("certificate")).expect("the earlier copy");

        let _ = kept.kill();
        let _ = kept.wait();

        assert!(!refused.status.success());
        assert!(kept_running, "a failed hand-over still hung up");
        assert_eq!(previous, "first chain");

        fs::write(from.join("fullchain.pem"), "renewed chain").expect("written");
        fs::write(from.join("privkey.pem"), "renewed key").expect("written");

        let mut hung_up = Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .spawn()
            .expect("a stand-in server");

        let output = handed_over(&from, &to, Some(hung_up.id()));
        let ended = hung_up.wait().expect("it ended");

        assert!(output.status.success(), "{output:?}");
        assert_eq!(ended.signal(), Some(HANGUP));
        assert_eq!(
            fs::read_to_string(to.join("certificate")).expect("read"),
            "renewed chain"
        );
        assert_eq!(
            fs::read_to_string(to.join("key")).expect("read"),
            "renewed key"
        );
    }

    fn stubbed(folder: &Path, log: &Path) -> PathBuf {
        let bin = folder.join("bin");

        fs::create_dir_all(&bin).expect("a stub folder");
        for program in ["systemctl", "deb-systemd-invoke"] {
            let stub = bin.join(program);

            fs::write(
                &stub,
                format!("#!/bin/sh\necho \"{program} $*\" >> '{}'\n", log.display()),
            )
            .expect("a stub");
            fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).expect("executable");
        }
        bin
    }

    fn maintained(script: &str, arguments: &[&str]) -> Vec<String> {
        let folder = scratch(&format!("deb-{script}"));
        let log = folder.join("asked");

        let bin = stubbed(&folder, &log);
        let path = format!("{}:{}", bin.display(), env::var("PATH").unwrap_or_default());

        let status = Command::new("sh")
            .arg(packaging_path(&format!("deb/server/{script}.sh")))
            .args(arguments)
            .env("PATH", path)
            .status()
            .expect("the maintainer script ran");

        assert!(status.success(), "{script} {arguments:?} failed");
        fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn under_systemd() -> bool {
        PathBuf::from("/run/systemd/system").is_dir()
    }

    #[test]
    fn a_debian_upgrade_restarts_a_running_server_and_a_first_install_starts_none() {
        if !under_systemd() {
            eprintln!("skipped: the maintainer scripts act only under systemd");
            return;
        }

        let installed = maintained("postinst", &["configure"]);
        let upgraded = maintained("postinst", &["configure", "0.16.0-1"]);
        let aborted = maintained("postinst", &["abort-upgrade", "0.17.0-1"]);
        let upgrading = maintained("prerm", &["upgrade", "0.18.0-1"]);
        let removed = maintained("prerm", &["remove"]);
        let purged = maintained("postrm", &["purge"]);

        assert_eq!(installed, ["systemctl --system daemon-reload"]);
        assert_eq!(
            upgraded,
            [
                "systemctl --system daemon-reload",
                "deb-systemd-invoke try-restart resonate-server.service"
            ]
        );
        assert!(aborted.is_empty(), "{aborted:?}");
        assert!(
            upgrading.is_empty(),
            "an upgrade stopped the server: {upgrading:?}"
        );
        assert_eq!(removed, ["deb-systemd-invoke stop resonate-server.service"]);
        assert_eq!(purged, ["systemctl --system daemon-reload"]);

        for script in DEB_MAINTAINER_SCRIPTS {
            let text = super::packaging(&format!("deb/server/{script}.sh"));

            for refused in ["enable", "disable", " start "] {
                assert!(
                    !text.contains(refused),
                    "{script} would {refused} the unit, changing what the operator chose"
                );
            }
        }
    }
}
