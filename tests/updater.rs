#![cfg(feature = "desktop")]
use lyra::desktop::updater::{install, network, package::*};
use semver::Version;
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command},
    sync::{OnceLock, atomic::AtomicBool},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

fn manifest(root: &Path, version: &str) -> Manifest {
    fs::write(root.join("VERSION"), format!("{version}\n")).unwrap();
    let m = Manifest {
        schema_version: 1,
        version: Version::parse(version).unwrap(),
        target: "x86_64-pc-windows-msvc".into(),
        flavor: "cpu".into(),
        files: MANAGED
            .into_iter()
            .map(|name| {
                (
                    name.to_owned(),
                    FileInfo {
                        size: fs::metadata(root.join(name)).unwrap().len(),
                        sha256: hash_file(&root.join(name)).unwrap(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>(),
    };
    fs::write(root.join(MANIFEST), serde_json::to_vec(&m).unwrap()).unwrap();
    m
}
fn raw_package(extra: Option<&str>) -> (TempDir, String) {
    let t = tempfile::tempdir().unwrap();
    let source = t.path().join("source");
    fs::create_dir(&source).unwrap();
    for name in MANAGED {
        fs::write(source.join(name), b"test content").unwrap();
    }
    manifest(&source, "0.0.2");
    let f = fs::File::create(t.path().join("package.zip")).unwrap();
    let mut zip = zip::ZipWriter::new(f);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for name in MANAGED.into_iter().chain([MANIFEST]) {
        zip.start_file(name, options).unwrap();
        zip.write_all(&fs::read(source.join(name)).unwrap())
            .unwrap();
    }
    if let Some(extra) = extra {
        zip.start_file(extra, options).unwrap();
        zip.write_all(b"extra").unwrap();
    }
    zip.finish().unwrap();
    let hash = hash_file(&t.path().join("package.zip")).unwrap();
    (t, hash)
}
#[test]
fn package_checks_hash_version_paths_duplicates_and_cancellation() {
    let v = Version::parse("0.0.2").unwrap();
    let no = AtomicBool::new(false);
    let (t, h) = raw_package(Some("docs/help.md"));
    let p = unpack(t, &v, &h, &no).unwrap();
    p.manifest.verify(&p.payload()).unwrap();
    assert!(!p.payload().join("docs").exists());
    for extra in [
        "../escape",
        "docs/../../escape",
        "C:/escape",
        "docs\\escape",
        "LYRA.EXE",
    ] {
        let (t, h) = raw_package(Some(extra));
        assert!(unpack(t, &v, &h, &no).is_err(), "{extra}");
    }
    let (t, _) = raw_package(None);
    let path = t.path().to_owned();
    assert!(unpack(t, &v, &"0".repeat(64), &no).is_err());
    assert!(!path.exists());
    let (t, h) = raw_package(None);
    assert!(unpack(t, &Version::parse("0.0.3").unwrap(), &h, &no).is_err());
    let (t, h) = raw_package(None);
    assert_eq!(
        unpack(t, &v, &h, &AtomicBool::new(true))
            .err()
            .unwrap()
            .kind(),
        std::io::ErrorKind::Interrupted
    );
    assert!(expected_hash(format!("{}  {ASSET}\n", "a".repeat(64)).as_bytes()).is_ok());
    assert!(
        expected_hash(format!("{0}  {ASSET}\n{0}  {ASSET}", "a".repeat(64)).as_bytes()).is_err()
    );
}
fn release(tag: &str, preview: bool) -> serde_json::Value {
    serde_json::json!({"tag_name":tag,"draft":false,"prerelease":preview,"assets":[{"name":ASSET,"size":512,"browser_download_url":format!("https://github.com/DraimCiDo/Lyra/releases/download/{tag}/{ASSET}")},{"name":SUMS,"size":100,"browser_download_url":format!("https://github.com/DraimCiDo/Lyra/releases/download/{tag}/{SUMS}")}]})
}
#[test]
fn release_channels_ignore_drafts_downgrades_and_foreign_urls() {
    let current = Version::parse("1.0.0").unwrap();
    let stable = release("v1.1.0", false);
    let preview = release("v1.2.0-dev.1", true);
    let json = serde_json::to_vec(&vec![
        preview.clone(),
        stable.clone(),
        release("v0.9.0", false),
    ])
    .unwrap();
    assert_eq!(
        network::select_release(&json, &current, false)
            .unwrap()
            .unwrap()
            .version
            .to_string(),
        "1.1.0"
    );
    assert_eq!(
        network::select_release(&json, &current, true)
            .unwrap()
            .unwrap()
            .version
            .to_string(),
        "1.2.0-dev.1"
    );
    let mut evil = stable.clone();
    evil["assets"][0]["browser_download_url"] = "https://example.com/lyra.zip".into();
    let mut draft = preview.clone();
    draft["draft"] = true.into();
    let mut wrong = preview;
    wrong["prerelease"] = false.into();
    assert!(
        network::select_release(
            &serde_json::to_vec(&vec![evil, draft, wrong, release("v1.0.0+build", false)]).unwrap(),
            &current,
            true
        )
        .unwrap()
        .is_none()
    );
    assert!(network::select_release(b"invalid", &current, true).is_err());
}
static FIXTURES: OnceLock<TempDir> = OnceLock::new();
fn fixture(version: &str) -> PathBuf {
    let t = FIXTURES.get_or_init(|| {
        let t = tempfile::tempdir().unwrap();
        for v in ["0.0.1", "0.0.2"] {
            let output = Command::new("rustc")
                .current_dir(env!("CARGO_MANIFEST_DIR"))
                .args([
                    "--edition=2024",
                    "-C",
                    "opt-level=1",
                    "tests/fixtures/update-app.rs",
                    "-o",
                ])
                .arg(t.path().join(format!("{v}.exe")))
                .env("LYRA_UPDATE_FIXTURE_VERSION", v)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let helper = t.path().join("helper.exe");
        fs::copy(env!("CARGO_BIN_EXE_lyra-desktop"), &helper).unwrap();
        // Linux DWARF makes the debug GUI hundreds of MiB. Ship builds already strip it.
        // Exercise the exact helper code with shipping-like file size, keeping real timeouts.
        #[cfg(target_os = "linux")]
        assert!(
            Command::new("strip")
                .arg("--strip-debug")
                .arg(&helper)
                .status()
                .unwrap()
                .success()
        );
        t
    });
    t.path().join(format!("{version}.exe"))
}
struct Parent(Child);
impl Drop for Parent {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn native_update(fail_health: bool, lock_cli: bool) {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("Приложение с пробелами");
    fs::create_dir(&root).unwrap();
    let root = root.canonicalize().unwrap();
    for name in ["lyra-desktop.exe", "lyra.exe"] {
        fs::copy(fixture("0.0.1"), root.join(name)).unwrap();
    }
    fs::write(root.join("THIRD_PARTY_NOTICES.txt"), "old license").unwrap();
    let old = manifest(&root, "0.0.1");
    for name in ["model.gguf", "settings.json", "project.rs"] {
        fs::write(root.join(name), "user data").unwrap();
    }
    if fail_health {
        fs::write(root.join("fail-health"), "").unwrap();
    }
    let d = tempfile::tempdir().unwrap();
    fs::create_dir(d.path().join("payload")).unwrap();
    let payload = d.path().join("payload");
    for name in ["lyra-desktop.exe", "lyra.exe"] {
        fs::copy(fixture("0.0.2"), payload.join(name)).unwrap();
    }
    fs::write(payload.join("THIRD_PARTY_NOTICES.txt"), "new license").unwrap();
    let new = manifest(&payload, "0.0.2");
    let pkg = Package {
        directory: d,
        manifest: new,
    };
    let mut parent = Parent(
        Command::new(root.join("lyra-desktop.exe"))
            .arg("--hold")
            .spawn()
            .unwrap(),
    );
    #[cfg(windows)]
    let locked = if lock_cli {
        use std::os::windows::fs::OpenOptionsExt;
        Some(
            fs::OpenOptions::new()
                .read(true)
                .share_mode(1)
                .open(root.join("lyra.exe"))
                .unwrap(),
        )
    } else {
        None
    };
    let cache = t.path().join("cache");
    install::prepare(
        pkg,
        &root,
        parent.0.id(),
        &fixture("0.0.1").parent().unwrap().join("helper.exe"),
        &cache,
        true,
    )
    .unwrap();
    old.verify(&root).unwrap();
    assert!(!root.join("started.txt").exists());
    parent.0.kill().unwrap();
    parent.0.wait().unwrap();
    let start = Instant::now();
    let receipt = loop {
        if let Some(r) = install::read_receipt(&cache) {
            break r;
        }
        assert!(
            start.elapsed() < Duration::from_secs(40),
            "helper receipt timed out"
        );
        thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(
        receipt.success,
        !fail_health && !lock_cli,
        "{}",
        receipt.message
    );
    #[cfg(windows)]
    drop(locked);
    let expected = if receipt.success { "0.0.2" } else { "0.0.1" };
    let start = Instant::now();
    loop {
        if fs::read_to_string(root.join("started.txt")).ok().as_deref() == Some(expected)
            && !root.join(".lyra-update.lock").exists()
        {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(10));
        thread::sleep(Duration::from_millis(50));
    }
    let m = Manifest::read(&root).unwrap();
    assert_eq!(m.version.to_string(), expected);
    m.verify(&root).unwrap();
    for name in ["model.gguf", "settings.json", "project.rs"] {
        assert_eq!(fs::read_to_string(root.join(name)).unwrap(), "user data");
    }
    let start = Instant::now();
    while fs::read_dir(&root).unwrap().flatten().any(|e| {
        e.file_name()
            .to_string_lossy()
            .starts_with(".lyra-update-stage-")
    }) {
        assert!(start.elapsed() < Duration::from_secs(10));
        thread::sleep(Duration::from_millis(50));
    }
}
#[test]
fn native_helper_waits_replaces_restarts_and_preserves_user_files() {
    native_update(false, false);
}
#[test]
fn failed_startup_rolls_back_and_restarts_previous_version() {
    native_update(true, false);
}
#[test]
#[cfg(windows)]
fn windows_locked_cli_rolls_back_already_replaced_gui() {
    native_update(false, true);
}

#[test]
fn modified_installed_files_and_unsupported_manifests_are_rejected() {
    let (t, h) = raw_package(None);
    let p = unpack(
        t,
        &Version::parse("0.0.2").unwrap(),
        &h,
        &AtomicBool::new(false),
    )
    .unwrap();
    let mut m = p.manifest.clone();
    m.flavor = "cuda".into();
    assert!(m.verify(&p.payload()).is_err());
    m = p.manifest.clone();
    m.files.get_mut("lyra.exe").unwrap().size = ARCHIVE_LIMIT + 1;
    assert!(m.validate().is_err());
    fs::write(p.payload().join("lyra.exe"), "user modification").unwrap();
    assert!(p.manifest.verify(&p.payload()).is_err());
}

#[test]
fn version_probe_accepts_both_real_entry_points() {
    let v = Version::parse(env!("CARGO_PKG_VERSION")).unwrap();
    // The GUI accepts --worker --version; the standalone CLI accepts only --version.
    // Use distribution names even on Unix to exercise both protocols.
    let t = tempfile::tempdir().unwrap();
    for (name, source) in [
        ("lyra.exe", env!("CARGO_BIN_EXE_lyra")),
        ("lyra-desktop.exe", env!("CARGO_BIN_EXE_lyra-desktop")),
    ] {
        fs::copy(source, t.path().join(name)).unwrap();
        install::probe(&t.path().join(name), &v).unwrap();
        assert!(install::probe(&t.path().join(name), &Version::parse("0.0.0").unwrap()).is_err());
    }
}
