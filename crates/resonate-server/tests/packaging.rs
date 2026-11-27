use std::{fs, path::PathBuf};

const UNIT: &str = "server/resonate-server.service";
const HAND_OVER: &str = "server/hand-over-credentials.sh";
const INSTALLED_HAND_OVER: &str = "/usr/lib/resonate-server/hand-over-credentials";
const HANDED_TO: &str = "/run/resonate-server-credentials";
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

fn opens_a_section(line: &str) -> bool {
    line.starts_with('%') && !line.starts_with("%{")
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
    let after = unit_value(&unit, "ExecStopPost");

    assert_eq!(unit_value(&unit, "RuntimeDirectory"), ["resonate-server"]);
    assert!(
        hand_over.contains("runtime=${RESONATE_SERVER_RUNTIME_DIRECTORY:-/run/resonate-server}")
    );
    assert!(
        hand_over.contains(&format!(
            "to=${{RESONATE_SERVER_CREDENTIALS_TO:-{HANDED_TO}}}"
        )),
        "the hand-over writes somewhere the server does not read from"
    );
    assert!(
        unit_value(&unit, "LoadCredential").is_empty(),
        "a credential is a snapshot a reload never refreshes"
    );

    assert_eq!(before, [format!("+{INSTALLED_HAND_OVER}")]);
    assert_eq!(reloads, [format!("+{INSTALLED_HAND_OVER} $MAINPID")]);
    assert_eq!(after, [format!("+{INSTALLED_HAND_OVER} withdraw")]);
    assert_eq!(starts.len(), 1);
    assert!(starts[0].contains(&format!("--certificate {HANDED_TO}/certificate")));
    assert!(starts[0].contains(&format!("--key {HANDED_TO}/key")));
    assert!(
        !starts[0].contains("RUNTIME_DIRECTORY"),
        "the server reads its pair from a folder it may write"
    );
}

#[test]
fn the_hand_over_narrows_every_file_it_makes_before_writing_any() {
    let hand_over = packaging(HAND_OVER);

    let narrowed = hand_over.find("umask 077").expect("a umask");
    let first_write = hand_over.find("install ").expect("an install");

    assert!(narrowed < first_write);
    assert!(hand_over.contains("install -d -m 0750 -g \"$group\" \"$to\""));
    assert!(
        !hand_over.contains("-o "),
        "the hand-over gives a file away from root"
    );
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
fn every_line_the_spec_runs_to_install_installs_a_file() {
    let spec = packaging(SPEC);

    let installing: Vec<&str> = spec
        .lines()
        .skip_while(|line| *line != "%install")
        .skip(1)
        .take_while(|line| !opens_a_section(line))
        .filter(|line| !line.is_empty())
        .collect();

    assert!(!installing.is_empty());
    for line in installing {
        assert!(
            line.starts_with("install "),
            "the spec's %install runs {line:?}, which installs nothing"
        );
    }
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
        process::{Child, Command, Output, Stdio},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{DEB_MAINTAINER_SCRIPTS, HAND_OVER, packaging_path};

    const REQUIRED_BY: &str = "RESONATE_SERVER_SYSTEMD_TESTS";
    const READ_BY_OWNER_AND_GROUP: u32 = 0o440;
    const ENTERED_BY_OWNER_AND_GROUP: u32 = 0o750;
    const PERMISSION_BITS: u32 = 0o7777;
    const HANGUP: i32 = 1;

    struct Folders {
        from: PathBuf,
        runtime: PathBuf,
        to: PathBuf,
    }

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

    fn laid_out(name: &str, chain: &str, key: &str) -> Folders {
        let folder = scratch(name);
        let folders = Folders {
            from: folder.join("etc"),
            runtime: folder.join("run"),
            to: folder.join("credentials"),
        };

        fs::create_dir_all(&folders.from).expect("a source");
        fs::create_dir_all(&folders.runtime).expect("a runtime directory");
        renewed(&folders, chain, key);
        folders
    }

    fn renewed(folders: &Folders, chain: &str, key: &str) {
        fs::write(folders.from.join("fullchain.pem"), chain).expect("written");
        fs::write(folders.from.join("privkey.pem"), key).expect("written");
    }

    fn handed_over(folders: &Folders, argument: Option<String>) -> Output {
        let mut command = Command::new("sh");

        command
            .arg(packaging_path(HAND_OVER))
            .args(argument)
            .env("RESONATE_SERVER_CREDENTIALS_FROM", &folders.from)
            .env("RESONATE_SERVER_RUNTIME_DIRECTORY", &folders.runtime)
            .env("RESONATE_SERVER_CREDENTIALS_TO", &folders.to);
        command.output().expect("the hand-over ran")
    }

    fn a_stand_in_server() -> Child {
        Command::new("sleep")
            .arg("30")
            .stdin(Stdio::null())
            .spawn()
            .expect("a stand-in server")
    }

    #[test]
    fn the_hand_over_writes_a_folder_of_its_own_the_servers_group_reads_and_cannot_write() {
        let folders = laid_out("hand-over", "first chain", "first key");

        let output = handed_over(&folders, None);
        let runtime = fs::metadata(&folders.runtime).expect("the runtime directory");
        let handed = fs::symlink_metadata(&folders.to).expect("the hand-over's folder");
        let runner = fs::metadata(scratch("runner"))
            .expect("a folder of the runner's")
            .uid();

        assert!(output.status.success(), "{output:?}");
        assert!(handed.is_dir());
        assert_eq!(
            handed.permissions().mode() & PERMISSION_BITS,
            ENTERED_BY_OWNER_AND_GROUP
        );
        assert_eq!(handed.gid(), runtime.gid());
        assert_eq!(handed.uid(), runner);

        for (copy, holds) in [("certificate", "first chain"), ("key", "first key")] {
            let path = folders.to.join(copy);
            let metadata = fs::symlink_metadata(&path).expect("handed over");

            assert!(metadata.is_file());
            assert_eq!(fs::read_to_string(&path).expect("read"), holds);
            assert_eq!(
                metadata.permissions().mode() & PERMISSION_BITS,
                READ_BY_OWNER_AND_GROUP
            );
            assert_eq!(metadata.gid(), runtime.gid());
            assert_eq!(metadata.uid(), runner);
        }

        assert_eq!(
            fs::read_dir(&folders.to).expect("listed").count(),
            2,
            "the hand-over left a staged copy behind"
        );
        assert_eq!(
            fs::read_dir(&folders.runtime).expect("listed").count(),
            0,
            "the hand-over wrote into the server's own folder"
        );
    }

    #[test]
    fn a_stopped_server_has_its_handed_over_pair_withdrawn() {
        let folders = laid_out("withdraw", "first chain", "first key");

        let handed = handed_over(&folders, None);
        let withdrawn = handed_over(&folders, Some("withdraw".to_owned()));

        assert!(handed.status.success(), "{handed:?}");
        assert!(withdrawn.status.success(), "{withdrawn:?}");
        assert!(!folders.to.exists());
    }

    #[test]
    fn a_reload_hangs_up_on_the_server_only_once_the_pair_is_handed_over() {
        let folders = laid_out("hangup", "first chain", "first key");

        assert!(handed_over(&folders, None).status.success());

        let mut kept = a_stand_in_server();

        fs::remove_file(folders.from.join("fullchain.pem")).expect("the renewal lost");

        let refused = handed_over(&folders, Some(kept.id().to_string()));
        let kept_running = kept.try_wait().expect("asked").is_none();
        let previous =
            fs::read_to_string(folders.to.join("certificate")).expect("the earlier copy");

        let _ = kept.kill();
        let _ = kept.wait();

        assert!(!refused.status.success());
        assert!(kept_running, "a failed hand-over still hung up");
        assert_eq!(previous, "first chain");

        renewed(&folders, "renewed chain", "renewed key");

        let mut hung_up = a_stand_in_server();

        let output = handed_over(&folders, Some(hung_up.id().to_string()));
        let ended = hung_up.wait().expect("it ended");

        assert!(output.status.success(), "{output:?}");
        assert_eq!(ended.signal(), Some(HANGUP));
        assert_eq!(
            fs::read_to_string(folders.to.join("certificate")).expect("read"),
            "renewed chain"
        );
        assert_eq!(
            fs::read_to_string(folders.to.join("key")).expect("read"),
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
            passed_over!("the maintainer scripts act only under systemd");
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
