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
    child.stdin.take().unwrap().write_all(input).unwrap();
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

#[cfg(unix)]
extern "C" {
    #[link_name = "pipe"]
    fn libc_pipe(descriptors: *mut i32) -> i32;
    #[link_name = "close"]
    fn libc_close(descriptor: i32) -> i32;
}
