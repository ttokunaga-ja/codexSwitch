use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("switch uninstall ' [x] ")
        .tempdir()
        .unwrap();
    let exe = dir.path().join(if cfg!(windows) {
        "copied Switch.exe"
    } else {
        "copied Switch"
    });
    fs::copy(env!("CARGO_BIN_EXE_codexSwitch"), &exe).unwrap();
    for name in [
        "config.toml",
        "openrouter.key",
        "zai.key",
        "conversation.jsonl",
        "cache",
        "codexSwitch.cmd",
        "codexSwitch-script.ps1",
        "replacement.exe",
        "codexSwitch.old.exe",
    ] {
        fs::write(dir.path().join(name), format!("keep {name}")).unwrap();
    }
    (dir, exe)
}

fn invoke(exe: &PathBuf, input: &str) -> std::process::Output {
    let mut child = Command::new(exe)
        .args(["--config", "missing-invalid-config", "uninstall"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn assert_neighbors(dir: &std::path::Path) {
    for name in [
        "config.toml",
        "openrouter.key",
        "zai.key",
        "conversation.jsonl",
        "cache",
        "codexSwitch.cmd",
        "codexSwitch-script.ps1",
        "replacement.exe",
        "codexSwitch.old.exe",
    ] {
        assert_eq!(
            fs::read_to_string(dir.join(name)).unwrap(),
            format!("keep {name}")
        );
    }
}

#[test]
fn cancellation_and_help_preserve_every_file_without_config() {
    for input in ["", "\n", "n\n", "ok\n", "yes please\n"] {
        let (dir, exe) = fixture();
        let before = fs::read(&exe).unwrap();
        let output = invoke(&exe, input);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read(&exe).unwrap(), before);
        assert_neighbors(dir.path());
        let preview = String::from_utf8_lossy(&output.stderr);
        assert!(preview.contains(&fs::canonicalize(&exe).unwrap().display().to_string()));
        assert!(preview.contains("[y/N]"));
        let help = Command::new(&exe)
            .args(["uninstall", "--help"])
            .output()
            .unwrap();
        assert!(help.status.success());
        assert_eq!(fs::read(&exe).unwrap(), before);
        assert_neighbors(dir.path());
    }
}

#[test]
fn each_confirmation_deletes_only_the_invoked_executable() {
    for input in ["y\n", "yes\n", "はい\n", " YEs \n"] {
        let (dir, exe) = fixture();
        let output = invoke(&exe, input);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        #[cfg(windows)]
        {
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains("削除を予約"));
            let receipt = stderr
                .lines()
                .find_map(|line| {
                    line.strip_prefix("結果の記録: ")
                        .and_then(|line| line.split_once("（status:").map(|(path, _)| path.trim()))
                })
                .unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(45);
            loop {
                let record: serde_json::Value =
                    serde_json::from_slice(&fs::read(receipt).unwrap()).unwrap();
                if record["status"] == "deleted" {
                    break;
                }
                assert_ne!(record["status"], "failed", "{record}");
                assert!(std::time::Instant::now() < deadline, "{record}");
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            fs::remove_dir_all(std::path::Path::new(receipt).parent().unwrap()).unwrap();
        }
        assert!(!exe.exists());
        assert_neighbors(dir.path());
        assert!(dir.path().is_dir());
    }
}

#[test]
fn replacing_the_previewed_file_aborts_and_preserves_replacement() {
    use std::io::Read;
    let (dir, exe) = fixture();
    let mut child = Command::new(&exe)
        .arg("uninstall")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let mut preview = Vec::new();
    while !preview.ends_with(b"[y/N]: ") {
        let mut byte = [0];
        assert_eq!(
            stderr.read(&mut byte).unwrap(),
            1,
            "process exited before preview"
        );
        preview.push(byte[0]);
    }
    fs::rename(&exe, dir.path().join("original preserved")).unwrap();
    fs::write(&exe, "replacement content").unwrap();
    child.stdin.take().unwrap().write_all(b"yes\n").unwrap();
    let status = child.wait().unwrap();
    assert!(!status.success());
    assert_eq!(fs::read_to_string(&exe).unwrap(), "replacement content");
    assert!(dir.path().join("original preserved").exists());
    assert_neighbors(dir.path());
}
