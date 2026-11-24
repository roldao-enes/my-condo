//! The pipeline end to end, through the real `condo-release` executable, the way CI runs it.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn tool(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_condo-release"))
        .args(args)
        .output()
        .unwrap()
}

fn ok(args: &[&str]) -> String {
    let output = tool(args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn s(path: &Path) -> &str {
    path.to_str().unwrap()
}

#[test]
fn keygen_manifest_sign_verify_and_tamper() {
    let dir = tempfile::tempdir().unwrap();
    let key = dir.path().join("private.key");
    let public = dir.path().join("public.txt");
    let manifest = dir.path().join("latest.json");
    let sig = dir.path().join("latest.json.sig");
    let dmg = dir.path().join("meu-condominio-0.2.0-macos.dmg");
    fs::write(&dmg, "disk image").unwrap();

    let printed = ok(&["keygen", s(&key)]);
    fs::write(&public, printed.lines().last().unwrap()).unwrap();
    // A key is never overwritten, and is private to its owner.
    assert!(!tool(&["keygen", s(&key)]).status.success());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&key).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    ok(&[
        "manifest",
        "--version",
        "v0.2.0",
        "--published",
        "2026-12-01T00:00:00Z",
        "--base-url",
        "https://example.test/dl/v0.2.0",
        "--out",
        s(&manifest),
        s(&dmg),
    ]);
    ok(&["sign", "--key-file", s(&key), s(&manifest)]);
    ok(&["verify", "--public-keys", s(&public), s(&manifest), s(&sig)]);

    // The key can also come from the environment, as the CI secret does.
    let env_output = Command::new(env!("CARGO_BIN_EXE_condo-release"))
        .args(["sign", "--key-env", "CONDO_TEST_KEY", s(&manifest)])
        .env("CONDO_TEST_KEY", fs::read_to_string(&key).unwrap())
        .output()
        .unwrap();
    assert!(env_output.status.success());

    // Changing the published manifest after signing is caught.
    let text = fs::read_to_string(&manifest)
        .unwrap()
        .replace("0.2.0", "0.9.0");
    fs::write(&manifest, text).unwrap();
    let verdict = tool(&["verify", "--public-keys", s(&public), s(&manifest), s(&sig)]);
    assert!(!verdict.status.success());
    assert!(String::from_utf8_lossy(&verdict.stderr).contains("signature"));
}

#[test]
fn a_build_tag_is_not_a_publishable_version() {
    let dir = tempfile::tempdir().unwrap();
    let dmg = dir.path().join("a-macos.dmg");
    fs::write(&dmg, "x").unwrap();
    let out = dir.path().join("latest.json");
    let output = tool(&[
        "manifest",
        "--version",
        "v0.1.0-build.42",
        "--published",
        "t",
        "--base-url",
        "https://example.test",
        "--out",
        s(&out),
        s(&dmg),
    ]);
    assert!(!output.status.success());
    assert!(!out.exists());
}
