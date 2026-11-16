#![allow(missing_docs)]
// Real programs as children. The krypton binary is itself a program on both Windows and Linux:
// `-c SCRIPT` runs a script, and `-` reads a script from stdin.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("krypton-d1-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Run `script` with the krypton binary bound to `$K`, in `dir`. Returns (stdout, status).
fn run(script: &str, dir: &Path) -> (String, i32) {
    let k = env!("CARGO_BIN_EXE_krypton");
    let full = format!("K='{k}'; {script}");
    let out = Command::new(k)
        .args(["-c", &full])
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .expect("run krypton");
    let stdout = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
    (stdout, out.status.code().unwrap_or(-1))
}

#[test]
fn pipeline_moves_data_between_real_programs() {
    let dir = temp_dir("pipe");
    let script = r#""$K" -c "echo 'echo deep'" | "$K" - | "$K" -c 'read x; echo got:$x'"#;
    let (out, code) = run(script, &dir);
    assert_eq!((out.as_str(), code), ("got:deep\n", 0));
}

#[test]
fn pipeline_status_is_the_last_stage() {
    let dir = temp_dir("status");
    let (out, code) = run(r#""$K" -c 'exit 3' | "$K" -c 'exit 5'; echo $?"#, &dir);
    assert_eq!((out.as_str(), code), ("5\n", 0));
}

#[test]
fn redirections_write_append_read_and_stderr() {
    let dir = temp_dir("redir");
    fs::write(dir.join("in.txt"), "from-file\n").expect("write input");
    let script = concat!(
        r#""$K" -c 'echo a' > out.txt; "$K" -c 'echo b' >> out.txt; "#,
        r#""$K" -c 'echo oops >&2' 2> err.txt; "#,
        r#""$K" -c 'read line; echo $line' < in.txt"#,
    );
    let (out, code) = run(script, &dir);
    let written = fs::read_to_string(dir.join("out.txt")).expect("out.txt");
    let err = fs::read_to_string(dir.join("err.txt")).expect("err.txt");
    assert_eq!(out, "from-file\n");
    assert_eq!(code, 0);
    assert_eq!(written.replace("\r\n", "\n"), "a\nb\n");
    assert_eq!(err.replace("\r\n", "\n"), "oops\n");
}

#[test]
fn exported_variables_reach_child_programs_and_braces_work() {
    let dir = temp_dir("env");
    let (out, _) = run(r#"export X=7; "$K" -c 'echo inner:$X'; echo ${X}"#, &dir);
    assert_eq!(out, "inner:7\n7\n");
}

#[test]
fn unknown_command_gives_127() {
    let dir = temp_dir("missing");
    let (out, _) = run("nosuchprogram_d1_xyz; echo $?", &dir);
    assert_eq!(out, "127\n");
}

#[test]
fn cd_tilde_uses_home() {
    let dir = temp_dir("home");
    let k = env!("CARGO_BIN_EXE_krypton");
    let out = Command::new(k)
        .args(["-c", "cd ~; pwd"])
        .current_dir(&dir)
        .env("HOME", &dir)
        .env("USERPROFILE", &dir)
        .stdin(Stdio::null())
        .output()
        .expect("run krypton");
    let shown = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(Path::new(&shown), dir.as_path());
}

#[cfg(windows)]
#[test]
fn batch_files_run_through_cmd_with_safe_quoting() {
    let dir = temp_dir("batch");
    fs::write(
        dir.join("show.cmd"),
        "@echo off\r\necho BATOK %1 %2\r\nexit /b 4\r\n",
    )
    .expect("write batch");
    let (out, code) = run(r#"./show.cmd one 'two three'; echo rc=$?"#, &dir);
    assert_eq!(out, "BATOK one \"two three\"\nrc=4\n");
    assert_eq!(code, 0);
    let (out, _) = run(r#"./show.cmd 'bad%arg'; echo rc=$?"#, &dir);
    assert!(out.starts_with("rc=126") || out.contains("rc=126"), "{out}");
}

/// The words of a one-command script as text: literal and escaped characters joined, no expansion.
fn words_text(src: &str, esc: krypton::Escapes) -> Vec<String> {
    use krypton::ast::{Command, Part};
    fn text(parts: &[Part], out: &mut String) {
        for part in parts {
            match part {
                Part::Lit(s) | Part::Single(s) => out.push_str(s),
                Part::Escaped(c) => out.push(*c),
                Part::Double(inner) => text(inner, out),
                other => panic!("unexpected part {other:?}"),
            }
        }
    }
    let script = krypton::parse(src, esc).expect("parse");
    let Command::Simple(cmd) = &script.items[0].first.cmds[0] else {
        panic!("{src:?} is not a simple command");
    };
    cmd.words
        .iter()
        .map(|w| {
            let mut s = String::new();
            text(w, &mut s);
            s
        })
        .collect()
}

#[test]
fn windows_mode_backslash_is_a_path_separator() {
    use krypton::Escapes::Windows;
    assert_eq!(words_text(r"echo C:\a\b", Windows), ["echo", r"C:\a\b"]);
    assert_eq!(
        words_text(r#"echo "C:\Program Files\x""#, Windows),
        ["echo", r"C:\Program Files\x"]
    );
    assert_eq!(words_text(r"echo \$HOME", Windows), ["echo", "$HOME"]);
    assert_eq!(words_text(r"echo C:\foo\", Windows), ["echo", r"C:\foo\"]);
    assert!(krypton::parse(r"echo \", Windows).is_ok());
}

#[test]
fn posix_mode_backslash_still_escapes() {
    use krypton::Escapes::Posix;
    assert_eq!(words_text(r"echo C:\a\b", Posix), ["echo", "C:ab"]);
    assert_eq!(words_text(r"echo \$HOME", Posix), ["echo", "$HOME"]);
    assert!(krypton::parse(r"echo \", Posix).is_err());
}

#[cfg(windows)]
#[test]
fn cmd_builtins_type_and_dir_run_through_cmd() {
    let dir = temp_dir("cmdb");
    let file = dir.join("k.txt");
    fs::write(&file, "hello-cmd\r\n").expect("write");
    let (out, _) = run(&format!(r#"type "{}"; echo rc=$?"#, file.display()), &dir);
    assert!(out.contains("hello-cmd"), "{out}");
    let (out, _) = run(
        &format!(r#"dir "{}" > /dev/null; echo rc=$?"#, file.display()),
        &dir,
    );
    assert!(out.ends_with("rc=0\n"), "{out}");
    // `ver` is cmd-only (no PATH program), so this runs through the cmd fallback.
    let (out, _) = run("ver; echo rc=$?", &dir);
    assert!(
        out.contains("Microsoft Windows") && out.ends_with("rc=0\n"),
        "{out}"
    );
}

#[cfg(windows)]
#[test]
fn git_bash_drive_path_maps_for_redirection_target() {
    let dir = temp_dir("gitpath");
    let native = dir.join("g.txt");
    let s = native.to_string_lossy().into_owned();
    let git = format!(
        "/{}{}",
        s[..1].to_ascii_lowercase(),
        s[2..].replace('\\', "/")
    );
    let (_, code) = run(&format!("echo hi > {git}"), &dir);
    assert_eq!(code, 0);
    assert_eq!(
        fs::read_to_string(&native)
            .expect("g.txt")
            .replace("\r\n", "\n"),
        "hi\n"
    );
}
