use super::*;

const EXAMPLE: &str = "---\nstatus: working\n---\n\n## Progress\n\n- [x] Step done\n- [ ] Step to do\n\n## Summary\n\n## Links\n";

#[test]
fn the_adr_example_is_working() {
    assert_eq!(parse_status(EXAMPLE), Status::Working);
}

#[test]
fn each_known_status_parses() {
    for (word, want) in [
        ("working", Status::Working),
        ("done", Status::Done),
        ("failed", Status::Failed),
    ] {
        assert_eq!(parse_status(&format!("---\nstatus: {word}\n---\n")), want);
    }
}

#[test]
fn no_frontmatter_is_unreadable() {
    assert_eq!(parse_status("## Progress\n"), Status::Unreadable);
    assert_eq!(parse_status(""), Status::Unreadable);
    assert_eq!(parse_status("status: done\n"), Status::Unreadable);
}

#[test]
fn frontmatter_without_status_is_unreadable() {
    assert_eq!(
        parse_status("---\ntitle: x\n---\nstatus: done\n"),
        Status::Unreadable
    );
}

#[test]
fn an_unclosed_frontmatter_is_unreadable() {
    assert_eq!(parse_status("---\nstatus: done\n"), Status::Unreadable);
}

#[test]
fn an_unknown_status_value_is_unreadable() {
    assert_eq!(
        parse_status("---\nstatus: finished\n---\n"),
        Status::Unreadable
    );
    assert_eq!(parse_status("---\nstatus:\n---\n"), Status::Unreadable);
    assert_eq!(parse_status("---\nstatus: Done\n---\n"), Status::Unreadable);
}

#[test]
fn crlf_line_ends_parse() {
    assert_eq!(
        parse_status("---\r\nstatus: done\r\n---\r\n\r\nbody\r\n"),
        Status::Done
    );
}

#[test]
fn a_quoted_value_and_a_byte_order_mark_parse() {
    assert_eq!(
        parse_status("---\nstatus: \"failed\"\n---\n"),
        Status::Failed
    );
    assert_eq!(
        parse_status("\u{feff}---\nstatus: done\n---\n"),
        Status::Done
    );
}

#[test]
fn the_hash_of_a_fixed_input() {
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn read_reports_missing_working_and_unreadable_files() {
    let dir = tempfile::tempdir().expect("tempdir");
    let missing = read(&dir.path().join("a.md")).expect("reads");
    assert_eq!(
        missing,
        ResultInfo {
            status: Status::Missing,
            sha256: None
        }
    );

    std::fs::write(dir.path().join("a.md"), EXAMPLE).expect("write");
    let info = read(&dir.path().join("a.md")).expect("reads");
    assert_eq!(info.status, Status::Working);
    assert_eq!(
        info.sha256.as_deref(),
        Some(sha256_hex(EXAMPLE.as_bytes()).as_str())
    );

    std::fs::write(dir.path().join("b.md"), [0xff, 0xfe, 0x00]).expect("write");
    let info = read(&dir.path().join("b.md")).expect("reads");
    assert_eq!(info.status, Status::Unreadable);
    assert!(info.sha256.is_some());
}

#[test]
fn the_hash_changes_when_the_body_changes_but_the_status_does_not() {
    let a = sha256_hex(format!("{EXAMPLE}more\n").as_bytes());
    assert_ne!(a, sha256_hex(EXAMPLE.as_bytes()));
}

#[test]
fn an_unreadable_path_is_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    // A folder where the file should be.
    assert!(read(dir.path()).is_err());
}

#[test]
fn single_quotes_and_a_trailing_comment_parse() {
    assert_eq!(parse_status("---\nstatus: 'done'\n---\n"), Status::Done);
    assert_eq!(
        parse_status("---\nstatus: working # still going\n---\n"),
        Status::Working
    );
    assert_eq!(
        parse_status("---\nstatus: \"failed\" # no\n---\n"),
        Status::Failed
    );
    assert_eq!(
        parse_status("---\nstatus: # only a comment\n---\n"),
        Status::Unreadable
    );
}
