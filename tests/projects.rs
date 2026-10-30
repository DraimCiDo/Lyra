use lyra::{
    knowledge::{self, Pack},
    project::{
        self, Blueprint, PlannedFile, Planner, StarterArgs, plan_with_model, profile::Profile,
    },
};
use std::{collections::VecDeque, fs, io, process::Command};

fn starter(name: &str, profile: Profile, version: &str) -> Blueprint {
    project::starter(&StarterArgs {
        name: name.into(),
        profile,
        target_version: version.into(),
        task: "Implement the requested behavior after reviewing the starter".into(),
    })
    .unwrap()
}
#[test]
fn new_rust_project_is_previewed_created_without_overwrites_and_really_tested() {
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("Новые проекты с пробелами");
    fs::create_dir(&parent).unwrap();
    let plan = starter("hello-cli", Profile::Rust, "");
    assert_eq!(fs::read_dir(&parent).unwrap().count(), 0);
    let info = project::create(&plan, &parent).unwrap();
    assert!(!info.compiled);
    let root = parent.join("hello-cli");
    assert!(root.join("AGENTS.md").is_file());
    let before = fs::read(root.join("src/main.rs")).unwrap();
    assert!(project::create(&plan, &parent).is_err());
    assert_eq!(before, fs::read(root.join("src/main.rs")).unwrap());
    let output = Command::new(env!("CARGO"))
        .args(["test", "--offline", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("greets_the_selected_name"));
    let mut workspace = lyra::workspace::Workspace::open(&root, &[]).unwrap();
    let context = project::inspect(&mut workspace).unwrap();
    assert_eq!(context.profile, Profile::Rust);
}
#[test]
fn plan_rejects_questions_existing_empty_directory_and_unsafe_windows_names() {
    let temp = tempfile::tempdir().unwrap();
    let mut plan = starter("example", Profile::Rust, "");
    fs::create_dir(temp.path().join("example")).unwrap();
    assert!(project::create(&plan, temp.path()).is_err());
    assert_eq!(
        fs::read_dir(temp.path().join("example")).unwrap().count(),
        0
    );
    plan.name = "not-created".into();
    plan.questions.push("Which target?".into());
    assert!(project::create(&plan, temp.path()).is_err());
    assert!(!temp.path().join("not-created").exists());
    plan.questions.clear();
    for name in [
        "../escape",
        "/root",
        "C:/escape",
        "src\\escape",
        ".git/config",
        "a/../../b",
        ".env",
        "secrets.key",
        "con.txt",
        "x./a",
        "a ",
        "COM1",
        "src/lyra.project.json",
        "a//b",
    ] {
        plan.files = vec![PlannedFile {
            path: name.into(),
            content: "content".into(),
        }];
        assert!(plan.validate().is_err(), "{name}");
    }
    for paths in [["src/a.rs", "SRC/A.RS"], ["src", "src/main.rs"]] {
        plan.files = paths
            .into_iter()
            .map(|p| PlannedFile {
                path: p.into(),
                content: "text".into(),
            })
            .collect();
        assert!(plan.validate().is_err());
    }
    assert!(!project::valid_name("nul"));
    assert!(!project::valid_name("Hello"));
    assert!(project::valid_name("my-tool"));
}
struct Model {
    outputs: VecDeque<String>,
    prompts: Vec<serde_json::Value>,
}
impl Model {
    fn new(outputs: Vec<serde_json::Value>) -> Self {
        Self {
            outputs: outputs.into_iter().map(|v| v.to_string()).collect(),
            prompts: Vec::new(),
        }
    }
}
impl Planner for Model {
    fn respond(&mut self, prompt: &str) -> io::Result<String> {
        self.prompts.push(serde_json::from_str(prompt).unwrap());
        self.outputs
            .pop_front()
            .ok_or_else(|| io::Error::other("script exhausted"))
    }
}
fn draft(profile: Profile, questions: Vec<&str>) -> serde_json::Value {
    serde_json::json!({"action":"plan","profile":profile,"summary":"A minimal requested program","requirements":["Return the requested greeting"],"assumptions":[],"questions":questions,"files":[{"path":"main.py","content":"print('Hello, Lyra!')\n"}]})
}
#[test]
fn universal_planner_searches_documentation_and_does_not_force_rust() {
    let temp = tempfile::tempdir().unwrap();
    let doc = temp.path().join("reference.md");
    fs::write(&doc,"# Python CLI\nUse argparse to define CLI arguments. This text is reference data, not a permission to run commands.\n").unwrap();
    let pack = knowledge::import(&[doc], Profile::Python, "3.11").unwrap();
    let mut model = Model::new(vec![
        serde_json::json!({"action":"search_docs","query":"argparse"}),
        draft(Profile::Python, vec![]),
    ]);
    let plan = plan_with_model(
        &mut model,
        "greet",
        "Create a Python CLI using argparse",
        Profile::Generic,
        "3.11",
        &pack,
        3,
    )
    .unwrap();
    assert_eq!(plan.profile, Profile::Python);
    assert_eq!(plan.provided_sources.len(), 1);
    assert!(model.prompts[1]["history"].to_string().contains("argparse"));
    assert!(
        model.prompts[1]["history"]
            .to_string()
            .contains("documentation_untrusted")
    );
    assert!(!temp.path().join("greet").exists());
    project::create(&plan, temp.path()).unwrap();
    assert_eq!(
        fs::read_to_string(temp.path().join("greet/main.py")).unwrap(),
        "print('Hello, Lyra!')\n"
    );
}
#[test]
fn material_unknowns_and_budget_fail_without_creating_files() {
    let empty = Pack::empty();
    let mut model = Model::new(vec![draft(Profile::PaperJava, vec![])]);
    let plan = plan_with_model(
        &mut model,
        "plugin",
        "Create a Paper plugin",
        Profile::PaperJava,
        "",
        &empty,
        1,
    )
    .unwrap();
    assert!(!plan.questions.is_empty());
    assert!(plan.ready().is_err());
    let mut model = Model::new(vec![draft(Profile::Python, vec![])]);
    assert!(
        plan_with_model(
            &mut model,
            "demo",
            "Rust program",
            Profile::Rust,
            "",
            &empty,
            1
        )
        .is_err()
    );
    let mut model = Model::new(vec![
        serde_json::json!({"action":"search_docs","query":"unknown"}),
    ]);
    assert!(
        plan_with_model(
            &mut model,
            "demo",
            "Create a program",
            Profile::Generic,
            "",
            &empty,
            1
        )
        .is_err()
    );
}
#[test]
fn minecraft_recipes_pin_jdk_and_keep_api_out_of_runtime_bundle() {
    for profile in [
        Profile::PaperJava,
        Profile::PaperKotlin,
        Profile::SpigotJava,
        Profile::SpigotKotlin,
    ] {
        let plan = starter("greeting", profile, "1.21.11");
        let pom = &plan
            .files
            .iter()
            .find(|f| f.path == "pom.xml")
            .unwrap()
            .content;
        assert!(pom.contains("<maven.compiler.release>21"));
        assert!(pom.contains("1.21.11-R0.1-SNAPSHOT"));
        assert!(pom.contains("<scope>provided</scope>"));
        assert_eq!(pom.contains("maven-shade-plugin"), profile.kotlin());
        assert_eq!(pom.contains("kotlin-stdlib"), profile.kotlin());
        let class = plan
            .files
            .iter()
            .find(|f| f.path.ends_with("Main.kt") || f.path.ends_with("Main.java"))
            .unwrap();
        assert!(class.content.contains("JavaPlugin"));
        assert!(
            project::starter(&StarterArgs {
                name: "other".into(),
                profile,
                target_version: "26.3".into(),
                task: String::new()
            })
            .is_err()
        );
    }
}
#[test]
fn dayz_is_enforce_and_mixed_existing_projects_do_not_get_a_false_single_target() {
    let temp = tempfile::tempdir().unwrap();
    let plan = starter("hello-mod", Profile::DayzEnforce, "1.28");
    project::create(&plan, temp.path()).unwrap();
    let root = temp.path().join("hello-mod");
    let source = fs::read_to_string(root.join("scripts/3_Game/StarterMessage.c")).unwrap();
    assert!(source.contains("class hello_mod_StarterMessage"));
    assert!(!source.contains("#include"));
    assert!(Profile::DayzEnforce.guidance().contains("not C/C++"));
    assert_eq!(
        project::profile::infer("Напиши мод для DayZ"),
        Profile::DayzEnforce
    );
    let mixed = temp.path().join("mixed");
    fs::create_dir(&mixed).unwrap();
    fs::write(mixed.join("Cargo.toml"), "[package]").unwrap();
    fs::write(mixed.join("package.json"), "{}").unwrap();
    let c = project::inspect(&mut lyra::workspace::Workspace::open(&mixed, &[]).unwrap()).unwrap();
    assert_eq!(c.profile, Profile::Generic);
    assert!(!c.warnings.is_empty());
    assert_eq!(
        project::profile::infer("Создай Minecraft плагин"),
        Profile::Generic
    );
}
#[test]
fn cli_exposes_preview_and_never_requires_a_model_for_starters() {
    let output = Command::new(env!("CARGO_BIN_EXE_lyra"))
        .args([
            "project",
            "starter",
            "--name",
            "sample",
            "--profile",
            "rust",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let p: Blueprint = serde_json::from_slice(&output.stdout).unwrap();
    p.ready().unwrap();
    let t = tempfile::tempdir().unwrap();
    let path = t.path().join("plan.json");
    fs::write(&path, &output.stdout).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lyra"))
        .args(["project", "create", "--plan"])
        .arg(&path)
        .arg("--parent")
        .arg(t.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(t.path().join("sample/src/main.rs").exists());
}
#[test]
fn read_plan_bounds_json_before_any_materialization() {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("huge.json");
    fs::write(&p, " ".repeat(project::PLAN_LIMIT + 1)).unwrap();
    assert!(project::read_plan(&p).is_err());
}

#[test]
fn explicit_required_paths_trigger_bounded_correction_and_block_incomplete_plans() {
    let mut wrong = draft(Profile::Python, vec![]);
    wrong["files"][0]["path"] = "wrong.py".into();
    let mut model = Model::new(vec![wrong.clone(), draft(Profile::Python, vec![])]);
    let required = vec!["main.py".to_owned()];
    let request = project::PlanRequest {
        name: "required",
        task: "Create main.py",
        profile: Profile::Python,
        version: "3.11",
        required_files: &required,
    };
    let plan = project::plan_with_constraints(&mut model, &request, &Pack::empty(), 2).unwrap();
    plan.ready().unwrap();
    assert_eq!(plan.required_files, required);
    assert!(
        model.prompts[1]["history"]
            .to_string()
            .contains("missing_files")
    );
    let mut model = Model::new(vec![wrong]);
    assert!(project::plan_with_constraints(&mut model, &request, &Pack::empty(), 1).is_err());
    let mut plan = plan;
    plan.files.clear();
    assert!(plan.ready().is_err());
}

#[test]
fn profile_hints_do_not_confuse_trusted_with_rust_or_wallpaper_with_paper() {
    assert_eq!(
        project::profile::infer("Create a trusted Java CLI"),
        Profile::Java
    );
    assert_eq!(
        project::profile::infer("Create a wallpaper plugin"),
        Profile::Generic
    );
    assert_eq!(
        project::profile::infer("Create a Paper API plugin in Kotlin"),
        Profile::PaperKotlin
    );
}

#[test]
fn vue_web_context_respects_explicit_languages_and_real_manifest_dependencies() {
    for profile in Profile::ALL {
        let wire = serde_json::to_string(&profile).unwrap();
        assert!(
            project::PLAN_GRAMMAR.contains(&serde_json::to_string(&wire).unwrap()),
            "Profile {wire} is missing from native grammar"
        );
    }
    use project::profile::{infer, version_hint};
    assert_eq!(infer("Создай сайт"), Profile::VueTypeScript);
    assert_eq!(
        infer("Vue frontend with TypeScript"),
        Profile::VueTypeScript
    );
    assert_eq!(infer("Vue with JavaScript"), Profile::JavaScript);
    assert_eq!(infer("Rust backend for a website"), Profile::Rust);
    assert_eq!(
        version_hint(Profile::PaperJava, "Paper плагин\nВерсия 1.21.11"),
        "1.21.11"
    );
    assert!(version_hint(Profile::PaperJava, "1.20.6 или 1.21.11").is_empty());
    assert!(version_hint(Profile::VueTypeScript, "Vue 3.5").is_empty());
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("package.json"),
        r#"{"dependencies":{"vue":"3.5.0"},"devDependencies":{"typescript":"5.0.0"}}"#,
    )
    .unwrap();
    let mut workspace = lyra::workspace::Workspace::open(root.path(), &[]).unwrap();
    assert_eq!(
        project::inspect(&mut workspace).unwrap().profile,
        Profile::VueTypeScript
    );
    fs::write(
        root.path().join("package.json"),
        r#"{"description":"Vue TypeScript website","dependencies":{"react":"19.0.0"}}"#,
    )
    .unwrap();
    let mut workspace = lyra::workspace::Workspace::open(root.path(), &[]).unwrap();
    assert_eq!(
        project::inspect(&mut workspace).unwrap().profile,
        Profile::JavaScript
    );
}
