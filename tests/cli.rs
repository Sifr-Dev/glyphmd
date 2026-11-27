use std::io::Write;
use std::process::{Command, Stdio};

fn run(args: &[&str], input: &[u8]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sifr-md"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Err(error) = child.stdin.take().unwrap().write_all(input) {
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::BrokenPipe,
            "stdin write failed: {error}"
        );
    }
    child.wait_with_output().unwrap()
}

#[test]
fn help_and_version_are_cli_operations() {
    let help = run(&["--help"], b"");
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("usage: sifr-md"));
    let version = run(&["--version"], b"");
    assert!(version.status.success());
    assert_eq!(version.stdout, b"sifr-md 0.1.0\n");
}

#[test]
fn invalid_arguments_fail_without_rendering() {
    for args in [vec!["--unknown"], vec!["one", "two"]] {
        let output = run(&args, b"# not rendered\n");
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("usage: sifr-md"));
    }
}

#[test]
fn dash_reads_stdin() {
    let output = run(&["-"], b"plain\n");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"plain\x1b[0m\x1b[0m\n");
}

#[cfg(unix)]
#[test]
fn closed_stdout_is_not_a_panic() {
    use std::os::unix::io::FromRawFd;
    let mut descriptors = [0; 2];
    unsafe { assert_eq!(libc_pipe(descriptors.as_mut_ptr()), 0) };
    let (read_fd, write_fd) = (descriptors[0], descriptors[1]);
    unsafe { libc_close(read_fd) };
    let mut child = Command::new(env!("CARGO_BIN_EXE_sifr-md"))
        .stdin(Stdio::piped())
        .stdout(unsafe { Stdio::from_raw_fd(write_fd) })
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"plain\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0));
}

/// Regression: `\` before a multibyte char panicked at src/main.rs:238
/// ("start byte index N is not a char boundary"). The escaped char must be
/// emitted literally, whole, with no panic and exit code 0.
#[test]
fn escaping_multibyte_characters_renders_literally() {
    for (input, expected_char) in [
        ("\\é\n", "\u{e9}"),     // 2-byte
        ("\\—\n", "\u{2014}"),   // 3-byte
        ("\\🙂\n", "\u{1F642}"), // 4-byte
    ] {
        let output = run(&["-"], input.as_bytes());
        assert_eq!(
            output.status.code(),
            Some(0),
            "input {input:?} panicked; stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
        assert_eq!(
            output.stdout,
            format!("{expected_char}\x1b[0m\x1b[0m\n").as_bytes(),
            "input {input:?}"
        );
    }
}

#[test]
fn escapes_inside_mixed_multibyte_text_are_unicode_safe() {
    let output = run(&["-"], "صفر \\🙂 é \\— fin\n".as_bytes());
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, "صفر 🙂 é — fin\x1b[0m\x1b[0m\n".as_bytes());
}

#[test]
fn escapes_still_neutralize_markdown_syntax() {
    let output = run(&["-"], b"\\*not italic\\* and \\*\\*not bold\\*\\*\n");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        output.stdout,
        "*not italic* and **not bold**\x1b[0m\x1b[0m\n".as_bytes()
    );
}

/// A backslash escapes exactly one character (CommonMark), so `\**` is a
/// literal `*` followed by live italic syntax — pre-existing behaviour kept.
#[test]
fn escape_consumes_only_one_character() {
    let output = run(&["-"], b"\\**x**\n");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        output.stdout,
        "*\x1b[3mx\x1b[0m*\x1b[0m\x1b[0m\n".as_bytes()
    );
}

#[test]
fn trailing_backslash_and_lone_backslash_are_literal() {
    for (input, expected) in [
        ("\\\n", "\\\u{1b}[0m\u{1b}[0m\n"),
        ("ab\\\n", "ab\\\u{1b}[0m\u{1b}[0m\n"),
        ("🙂\\\n", "🙂\\\u{1b}[0m\u{1b}[0m\n"),
    ] {
        let output = run(&["-"], input.as_bytes());
        assert_eq!(output.status.code(), Some(0), "input {input:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "input {input:?}"
        );
    }
}

/// Invalid UTF-8 used to abort because `read_to_string` rejects it. It must now
/// render lossily (U+FFFD) instead of failing, from stdin and from a real file.
#[test]
fn invalid_utf8_is_rendered_lossily_not_rejected() {
    let broken: &[u8] = b"caf\xe9 \xff\xfe ok \\\xc3\xa9\n";
    let stdin_output = run(&["-"], broken);
    assert_eq!(
        stdin_output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&stdin_output.stderr)
    );
    let text = String::from_utf8_lossy(&stdin_output.stdout);
    assert!(text.contains('\u{FFFD}'), "got {text:?}");
    assert!(text.contains("caf\u{FFFD}"), "got {text:?}");
    assert!(
        text.contains('é'),
        "escape pair must survive as é: {text:?}"
    );

    let path = std::env::temp_dir().join("sifr-md-invalid-utf8.md");
    std::fs::write(&path, broken).unwrap();
    let file_output = run(&[path.to_str().unwrap()], b"");
    std::fs::remove_file(&path).ok();
    assert_eq!(file_output.status.code(), Some(0));
    assert_eq!(file_output.stdout, stdin_output.stdout);
}

/// Boundaries: truncated multibyte sequences (start, middle, end of a char).
#[test]
fn truncated_multibyte_boundaries_do_not_panic() {
    let cases: [&[u8]; 6] = [
        b"\xf0\x9f\x99",       // emoji missing last byte
        b"a\xe2\x80",          // em dash missing last byte
        b"\xc3",               // é truncated to lead byte
        b"\\\xf0\x9f\x99",     // escape + truncated emoji
        b"\\\xf0\x9f\x99\x82", // escape + complete emoji
        b"\\\xff",             // escape + invalid byte
    ];
    for case in cases {
        let output = run(&["-"], case);
        assert_eq!(
            output.status.code(),
            Some(0),
            "case {case:?} exited {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
    }
}

#[cfg(unix)]
extern "C" {
    #[link_name = "pipe"]
    fn libc_pipe(descriptors: *mut i32) -> i32;
    #[link_name = "close"]
    fn libc_close(descriptor: i32) -> i32;
}
