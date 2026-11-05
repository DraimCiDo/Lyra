use lyra::{
    universal::{
        Technology, WorkPlan,
        artifacts::{FileProposal, Input, Store},
        environment::Registry,
        execution::extract,
    },
    workspace::Workspace,
};
use std::{fs, io};
#[test]
fn open_technologies_and_custom_environment_are_not_language_enums() {
    let plan = WorkPlan {
        schema_version: 1,
        goal: "A polyglot task".into(),
        technologies: vec![
            Technology {
                name: "Go".into(),
                purpose: "service".into(),
                rationale: "deployment".into(),
            },
            Technology {
                name: "Julia".into(),
                purpose: "analysis".into(),
                rationale: "libraries".into(),
            },
        ],
        ..Default::default()
    };
    plan.validate().unwrap();
    let registry:Registry=serde_json::from_value(serde_json::json!({"schema_version":1,"environments":[{"id":"custom-julia","description":"User configured runtime","image":format!("runtime@sha256:{}","a".repeat(64)),"program":"julia","script_extension":"jl","capabilities":["julia","analysis"]}]})).unwrap();
    registry.validate().unwrap();
    assert!(registry.get("custom-julia").is_ok());
    assert!(registry.get("unregistered").is_err());
}
#[test]
fn blob_integrity_and_file_conflicts_prevent_overwrites() {
    let repo = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let store = Store::open(cache.path()).unwrap();
    let original = b"PK\0original document";
    fs::write(repo.path().join("input.docx"), original).unwrap();
    let file = FileProposal {
        path: "input.docx".into(),
        original: Some(store.put(original).unwrap()),
        replacement: store.put(b"PK\0replacement").unwrap(),
        inputs: vec![],
    };
    fs::write(repo.path().join("input.docx"), b"user edits").unwrap();
    assert!(
        lyra::universal::artifacts::publish(
            repo.path(),
            &store,
            std::slice::from_ref(&file),
            || Ok(())
        )
        .is_err()
    );
    assert_eq!(
        fs::read(repo.path().join("input.docx")).unwrap(),
        b"user edits"
    );
    fs::write(repo.path().join("input.docx"), original).unwrap();
    lyra::universal::artifacts::publish(
        repo.path(),
        &store,
        std::slice::from_ref(&file),
        || Ok(()),
    )
    .unwrap();
    assert_eq!(
        fs::read(repo.path().join("input.docx")).unwrap(),
        b"PK\0replacement"
    );
    fs::write(cache.path().join(&file.replacement.sha256), b"tampered").unwrap();
    assert!(store.get(&file.replacement).is_err());
}
#[test]
fn publication_rolls_back_binary_and_text_after_a_failed_final_guard() {
    let repo = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let store = Store::open(cache.path()).unwrap();
    fs::write(repo.path().join("old.bin"), b"old\0").unwrap();
    let files = vec![
        FileProposal {
            path: "old.bin".into(),
            original: Some(store.put(b"old\0").unwrap()),
            replacement: store.put(b"new\0").unwrap(),
            inputs: vec![],
        },
        FileProposal {
            path: "sub/new.txt".into(),
            original: None,
            replacement: store.put(b"new text").unwrap(),
            inputs: vec![],
        },
    ];
    let calls = std::cell::Cell::new(0);
    let result = lyra::universal::artifacts::publish(repo.path(), &store, &files, || {
        calls.set(calls.get() + 1);
        if calls.get() > 1 {
            Err(io::Error::other("injected final check failure"))
        } else {
            Ok(())
        }
    });
    assert!(result.is_err());
    assert_eq!(fs::read(repo.path().join("old.bin")).unwrap(), b"old\0");
    assert!(!repo.path().join("sub/new.txt").exists());
    assert!(!repo.path().join(".lyra/artifact-journal.json").exists());
}
#[test]
fn dependency_drift_blocks_publication_before_any_output() {
    let repo = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let store = Store::open(cache.path()).unwrap();
    fs::write(repo.path().join("source.csv"), b"old").unwrap();
    let file = FileProposal {
        path: "result.xlsx".into(),
        original: None,
        replacement: store.put(b"result\0").unwrap(),
        inputs: vec![Input {
            path: "source.csv".into(),
            blob: store.put(b"old").unwrap(),
        }],
    };
    fs::write(repo.path().join("source.csv"), b"changed").unwrap();
    assert!(lyra::universal::artifacts::publish(repo.path(), &store, &[file], || Ok(())).is_err());
    assert!(!repo.path().join("result.xlsx").exists());
}
fn archive(path: &str, bytes: &[u8], link: bool) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut out);
        let mut h = tar::Header::new_gnu();
        h.set_size(if link { 0 } else { bytes.len() as u64 });
        h.set_mode(0o644);
        if link {
            h.set_entry_type(tar::EntryType::Symlink);
            h.set_link_name("/etc/passwd").unwrap();
        }
        h.set_cksum();
        builder
            .append_data(&mut h, path, if link { &[][..] } else { bytes })
            .unwrap();
        builder.finish().unwrap();
    }
    out
}
#[test]
fn archive_import_rejects_links_and_undeclared_outputs() {
    assert!(extract(&archive("out.pdf", b"PDF", true), &["out.pdf".into()]).is_err());
    assert!(extract(&archive("secret.txt", b"no", false), &["out.pdf".into()]).is_err());
    assert_eq!(
        extract(
            &archive("nested/out.pdf", b"PDF", false),
            &["nested/out.pdf".into()]
        )
        .unwrap()[0]
            .1,
        b"PDF"
    );
}
#[test]
fn binary_snapshots_and_results_survive_restart() {
    let repo = tempfile::tempdir().unwrap();
    let metadata = tempfile::tempdir().unwrap();
    let db = metadata.path().join("tasks.db");
    fs::write(repo.path().join("input.pdf"), b"%PDF\0saved").unwrap();
    let id = {
        let mut store = lyra::session::SessionStore::open(&db).unwrap();
        store
            .create(repo.path(), &["input.pdf".into()], "Edit document")
            .unwrap()
    };
    fs::write(repo.path().join("input.pdf"), b"external edit").unwrap();
    let store = lyra::session::SessionStore::open(&db).unwrap();
    let snapshot = store.materialize(&id).unwrap();
    assert_eq!(
        fs::read(snapshot.directory.path().join("input.pdf")).unwrap(),
        b"%PDF\0saved"
    );
    let w = Workspace::open(snapshot.directory.path(), &[]).unwrap();
    assert!(w.files.contains("input.pdf"));
}
struct Planner;
impl lyra::project::Planner for Planner {
    fn respond(&mut self, _: &str) -> io::Result<String> {
        Ok(serde_json::json!({"action":"plan","profile":"go","summary":"Go CLI selected for deployment","requirements":[],"assumptions":[],"questions":[],"files":[{"path":"main.go","content":"package main\nfunc main() {}\n"}]}).to_string())
    }
}
#[test]
fn project_planner_accepts_a_language_outside_legacy_profiles() {
    let result = lyra::project::plan_with_model(
        &mut Planner,
        "go-app",
        "Create a Go CLI",
        lyra::project::profile::Profile::Generic,
        "",
        &lyra::knowledge::Pack::empty(),
        2,
    )
    .unwrap();
    assert_eq!(result.work_plan.unwrap().technologies[0].name, "go");
}
#[test]
#[ignore = "requires explicitly prepared Docker image and enforced cgroups"]
fn scripts_return_binary_artifacts_in_sandbox() {
    let image = std::env::var("LYRA_TEST_IMAGE").unwrap();
    let registry:Registry=serde_json::from_value(serde_json::json!({"schema_version":1,"environments":[{"id":"python","description":"test","image":image,"program":"python","script_extension":"py"}]})).unwrap();
    let repo = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut session = lyra::universal::execution::ExecutionSession::new(
        registry,
        vec!["python".into()],
        cache.path(),
    )
    .unwrap();
    let w = Workspace::open(repo.path(), &[]).unwrap();
    let (value, files) = session
        .run(
            &w,
            &[],
            "python",
            "from pathlib import Path\nPath('/output/result.bin').write_bytes(b'abc\\x00def')",
            &[],
            &["result.bin".into()],
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(value["execution"]["exit_code"], 0);
    assert_eq!(files.len(), 1);
    assert_eq!(
        session.store.get(&files[0].replacement).unwrap(),
        b"abc\0def"
    );
    assert!(!repo.path().join("result.bin").exists());
}
#[test]
#[ignore = "requires the bundled document image and enforced cgroups"]
fn document_environment_creates_and_renders_office_and_pdf_deliverables() {
    let registry = Registry::load(std::path::Path::new(
        &std::env::var("LYRA_DOCUMENT_REGISTRY").unwrap(),
    ))
    .unwrap();
    let repo = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut session = lyra::universal::execution::ExecutionSession::new(
        registry,
        vec!["documents".into()],
        cache.path(),
    )
    .unwrap();
    let workspace = Workspace::open(repo.path(), &[]).unwrap();
    let script = "import lyra_documents as d\nd.create('/output/report.docx',{'paragraphs':['Привет, Lyra']})\nd.create('/output/book.xlsx',{'sheets':{'Data':[[1,2,'=SUM(A1:B1)']]}})\nd.create('/output/slides.pptx',{'slides':[{'title':'Lyra','paragraphs':['Data']} ]})\nd.create('/output/report.pdf',{'pages':[['Привет, Lyra']]})\nd.render('/output/report.docx','/output/preview.pdf')\nd.render('/output/report.pdf','/output/preview.png')\n";
    let outputs = [
        "report.docx",
        "book.xlsx",
        "slides.pptx",
        "report.pdf",
        "preview.pdf",
        "preview.png",
    ]
    .map(String::from);
    let (value, files) = session
        .run(
            &workspace,
            &[],
            "documents",
            script,
            &[],
            &outputs,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(value["execution"]["exit_code"], 0, "{value}");
    assert_eq!(files.len(), 6);
    assert!(files.iter().all(|f| f.replacement.bytes > 0));
    assert!(!repo.path().join("report.docx").exists());
}
#[test]
#[ignore = "requires explicit Go/PHP images and enforced cgroups"]
fn additional_registered_languages_execute_without_rust_profile_changes() {
    let registry = Registry::load(std::path::Path::new("environments/runtimes.json")).unwrap();
    let repo = tempfile::tempdir().unwrap();
    let cache = tempfile::tempdir().unwrap();
    let mut session = lyra::universal::execution::ExecutionSession::new(
        registry,
        vec!["golang".into(), "php".into()],
        cache.path(),
    )
    .unwrap();
    let workspace = Workspace::open(repo.path(), &[]).unwrap();
    for (id, script, expected) in [
        (
            "golang",
            "package main\nimport \"os\"\nfunc main(){if err:=os.WriteFile(\"/output/result.txt\",[]byte(\"go\"),0600);err!=nil{panic(err)}}",
            "go",
        ),
        (
            "php",
            "<?php file_put_contents('/output/result.txt','php');",
            "php",
        ),
    ] {
        let (value, files) = session
            .run(
                &workspace,
                &[],
                id,
                script,
                &[],
                &["result.txt".into()],
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(value["execution"]["exit_code"], 0, "{value}");
        assert_eq!(
            session.store.get(&files[0].replacement).unwrap(),
            expected.as_bytes()
        );
    }
}
#[test]
fn task_plan_does_not_grant_script_execution() {
    struct M {
        calls: usize,
    }
    impl lyra::agent::Model for M {
        fn respond(&mut self, _: &str) -> io::Result<String> {
            self.calls += 1;
            Ok(match self.calls{1=>serde_json::json!({"tool":"set_work_plan","plan":{"schema_version":1,"goal":"Analyze data","technologies":[{"name":"R","purpose":"statistics","rationale":"task requirement"}]}}).to_string(),2=>serde_json::json!({"tool":"run_script","environment":"r","script":"writeLines('bad','/output/out.txt')","inputs":[],"outputs":["out.txt"]}).to_string(),_=>serde_json::json!({"tool":"finish","summary":"Execution environment must be prepared and granted"}).to_string()})
        }
    }
    let repo = tempfile::tempdir().unwrap();
    let mut workspace = Workspace::open(repo.path(), &[]).unwrap();
    let report = lyra::agent::execute(
        &mut M { calls: 0 },
        &mut workspace,
        "Analyze data using R",
        false,
        4,
    );
    assert_eq!(report.work_plan.unwrap().technologies[0].name, "R");
    assert!(report.file_proposals.is_empty());
    assert!(
        report
            .events
            .iter()
            .any(|e| e.tool == "run_script" && e.outcome.starts_with("denied"))
    );
    assert!(!repo.path().join("out.txt").exists());
}
