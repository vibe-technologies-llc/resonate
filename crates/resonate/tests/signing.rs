use std::{fs, path::PathBuf};

const WORKFLOWS: [(&str, usize); 3] = [
    ("release.yml", 3),
    ("android-apk.yml", 1),
    ("macos-dmg.yml", 1),
];
const SIGNING_ENVIRONMENT: &str = "'release-signing'";
const SECRET: &str = "secrets.";
const BUILDING: [&str; 6] = [
    "cargo ",
    "rustup ",
    "gradlew",
    "rust-cache",
    "-Stage Programs",
    "\" bundle",
];
const SIGNING: [&str; 3] = ["windows/sign.ps1 -Files", "\" image", "apksigner\" sign"];
const KEYSTORE_DECODED: &str = "base64 -d";
const JOB_INDENT: &str = "  ";
const JOB_FIELD_INDENT: &str = "    ";
const STEP_OPENS: &str = "      - ";

struct Job<'a> {
    name: &'a str,
    header: Vec<&'a str>,
    steps: Vec<String>,
}

impl Job<'_> {
    fn holds_a_secret(&self) -> bool {
        self.steps.iter().any(|step| step.contains(SECRET))
    }

    fn environment(&self) -> Option<&str> {
        self.header.iter().find_map(|line| {
            line.strip_prefix(JOB_FIELD_INDENT)?
                .strip_prefix("environment: ")
        })
    }
}

fn workflow(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.github/workflows")
        .join(name);

    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn script(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packaging")
        .join(name);

    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn jobs(text: &str) -> Vec<Job<'_>> {
    let mut jobs: Vec<Job<'_>> = Vec::new();
    let mut inside = false;

    for line in text.lines() {
        if line == "jobs:" {
            inside = true;
            continue;
        }
        if !inside || line.trim().is_empty() {
            continue;
        }

        let opens_a_job = line.starts_with(JOB_INDENT)
            && !line[JOB_INDENT.len()..].starts_with(' ')
            && line.ends_with(':');

        if opens_a_job {
            jobs.push(Job {
                name: line.trim().trim_end_matches(':'),
                header: Vec::new(),
                steps: Vec::new(),
            });
            continue;
        }

        let job = jobs.last_mut().expect("a line under a job");

        if line.starts_with(STEP_OPENS) {
            job.steps.push(String::new());
        }
        match job.steps.last_mut() {
            Some(step) => {
                step.push_str(line);
                step.push('\n');
            }
            None => job.header.push(line),
        }
    }

    jobs
}

#[test]
fn no_job_that_builds_holds_a_signing_secret_and_every_job_that_signs_names_the_environment() {
    for (name, signing_jobs) in WORKFLOWS {
        let text = workflow(name);
        let jobs = jobs(&text);

        let signers: Vec<&Job<'_>> = jobs.iter().filter(|job| job.holds_a_secret()).collect();

        assert_eq!(
            signers.len(),
            signing_jobs,
            "{name} signs in other jobs than the ones it is known to"
        );

        for job in &jobs {
            let builds = job
                .steps
                .iter()
                .any(|step| BUILDING.iter().any(|building| step.contains(building)));

            if job.holds_a_secret() {
                assert!(
                    !builds,
                    "{name}'s {} builds on the runner it signs on",
                    job.name
                );
                assert!(
                    job.environment()
                        .is_some_and(|environment| environment.contains(SIGNING_ENVIRONMENT)),
                    "{name}'s {} signs outside the signing environment",
                    job.name
                );
            } else {
                assert!(
                    job.environment().is_none(),
                    "{name}'s {} names an environment it holds no secret of",
                    job.name
                );
            }

            for step in &job.steps {
                if step.contains(SECRET) {
                    assert!(
                        SIGNING.iter().any(|signing| step.contains(signing)),
                        "{name}'s {} hands a secret to a step that does not sign:\n{step}",
                        job.name
                    );
                }
                if step.contains(KEYSTORE_DECODED) {
                    assert!(
                        step.contains("apksigner\" sign"),
                        "{name}'s {} writes the keystore out where nothing signs with it",
                        job.name
                    );
                }
            }
        }
    }
}

#[test]
fn a_release_is_signed_only_from_its_own_tag_and_a_branch_is_built_unsigned() {
    let release = workflow("release.yml");

    assert!(
        release.contains("\"$GITHUB_REF\" != \"refs/tags/$RELEASE_TAG\""),
        "a release dispatched from a branch would be signed outside the tag the environment admits"
    );
    assert!(release.contains(
        "environment: ${{ needs.release.outputs.tag != '' && 'release-signing' || '' }}"
    ));

    for name in ["android-apk.yml", "macos-dmg.yml"] {
        assert!(
            workflow(name).contains(
                "environment: ${{ startsWith(github.ref, 'refs/tags/v') && 'release-signing' || '' }}"
            ),
            "{name} would ask for the signing environment from a branch"
        );
    }
}

#[test]
fn each_build_script_builds_in_one_stage_and_packs_and_signs_in_another() {
    let windows = script("windows/build.ps1");
    let macos = script("macos/build.sh");

    let programs = windows
        .split("\"Programs\" {")
        .nth(1)
        .and_then(|rest| rest.split("\"Package\" {").next())
        .expect("build.ps1's Programs stage");
    let package = windows
        .split("\"Package\" {")
        .nth(1)
        .and_then(|rest| rest.split("\"Whole\" {").next())
        .expect("build.ps1's Package stage");
    let imaged = macos
        .split("if [ \"$stage\" = image ]; then")
        .nth(1)
        .and_then(|rest| rest.split("fi\n").next())
        .expect("build.sh's image stage");
    let bundled = macos
        .split("if [ \"$stage\" = bundle ]; then")
        .nth(1)
        .and_then(|rest| rest.split("fi\n").next())
        .expect("build.sh's bundle stage");

    assert!(programs.contains("Build-Programs") && !programs.contains("$signing"));
    assert!(!package.contains("Build-Programs") && !package.contains("$signing"));
    assert!(imaged.contains("packed") && imaged.contains("exit 0") && !imaged.contains("cargo"));
    assert!(
        macos.find("if [ \"$stage\" = image ]; then") < macos.find("cargo build"),
        "build.sh's image stage would build before it signs"
    );
    assert!(bundled.contains("tar -c") && bundled.contains("exit 0"));
    assert!(
        macos.rfind("cargo build") < macos.find("if [ \"$stage\" = bundle ]; then"),
        "build.sh's bundle stage would leave before the programs are built"
    );
}
