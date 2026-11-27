//! sifr-md — zero-dependency Markdown → ANSI terminal renderer.
//!
//!   sifr-md [file.md]         (stdin default)
//! Reads Markdown (CommonMark-ish subset) and writes styled bytes to stdout:
//!   headings (#..####), **bold**, *italic*, `code`, ``` fences, - lists,
//!   1. ordered lists, > blockquote, --- hr, \[text\](url), ~~strike~~
//!
//! Backslash escapes the next character (any character, including multibyte
//! UTF-8) and always yields that character literally.
//!
//! No colors beyond SIFR: void/surface/dim shades + one accent (accent blue).
use std::fs;
use std::io::{self, Read, Write};

const RST: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const ITAL: &str = "\x1b[3m";
const DIM: &str = "\x1b[2m";
const DEL: &str = "\x1b[9m";
const ACCENT: &str = "\x1b[38;5;111m"; // sifr accent (bright blue)
const CODE: &str = "\x1b[38;5;251m\x1b[48;5;235m"; // light fg on dark surface
const QUOTE: &str = "\x1b[38;5;244m┃ "; // dim vertical bar

#[derive(PartialEq, Debug)]
enum Block {
    Para(String),
    H(usize, String),
    CodeFence(Vec<String>),
    List(Vec<(usize, String)>), // (depth, item)
    Quote(String),
    Hr,
}

fn flush_para(buf: &mut String, out: &mut Vec<Block>) {
    let t = buf.trim();
    if !t.is_empty() {
        out.push(Block::Para(t.to_string()));
    }
    buf.clear();
}

fn parse_blocks(text: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut para = String::new();
    let mut in_fence = false;
    let mut fence_lines: Vec<String> = Vec::new();
    let mut list: Vec<(usize, String)> = Vec::new();

    for raw in text.lines() {
        let line = raw.trim_end();
        if line.starts_with("```") {
            flush_para(&mut para, &mut out);
            if !list.is_empty() {
                out.push(Block::List(std::mem::take(&mut list)));
            }
            if in_fence {
                out.push(Block::CodeFence(std::mem::take(&mut fence_lines)));
                in_fence = false;
            } else {
                in_fence = true;
            }
            continue;
        }
        if in_fence {
            fence_lines.push(raw.to_string());
            continue;
        }
        if !list.is_empty() {
            // continuation of a list item: indented?
            if line.starts_with(' ') || line.starts_with('\t') {
                if let Some(last) = list.last_mut() {
                    let cont = line.trim();
                    if !cont.is_empty() {
                        last.1.push(' ');
                        last.1.push_str(cont);
                    }
                    continue;
                }
            } else {
                // a different block kind ends the list — but NOT another list item
                let tl = line.trim();
                let is_item =
                    tl.starts_with("- ") || tl.starts_with("* ") || split_ordered(tl).is_some();
                if !is_item {
                    out.push(Block::List(std::mem::take(&mut list)));
                }
            }
        }
        let t = line.trim();
        if t.is_empty() {
            flush_para(&mut para, &mut out);
            if !list.is_empty() {
                out.push(Block::List(std::mem::take(&mut list)));
            }
            continue;
        }
        // headings
        if let Some(rest) = t.strip_prefix("# ") {
            out.push(Block::H(1, rest.to_string()));
            continue;
        }
        if let Some(rest) = t.strip_prefix("## ") {
            out.push(Block::H(2, rest.to_string()));
            continue;
        }
        if let Some(rest) = t.strip_prefix("### ") {
            out.push(Block::H(3, rest.to_string()));
            continue;
        }
        if let Some(rest) = t.strip_prefix("#### ") {
            out.push(Block::H(4, rest.to_string()));
            continue;
        }
        // hr
        if t == "---" || t == "***" || t == "___" {
            flush_para(&mut para, &mut out);
            out.push(Block::Hr);
            continue;
        }
        // blockquote
        if let Some(rest) = t.strip_prefix("> ") {
            flush_para(&mut para, &mut out);
            out.push(Block::Quote(rest.to_string()));
            continue;
        }
        // lists
        if let Some(rest) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            flush_para(&mut para, &mut out);
            let depth = raw.len() - raw.trim_start().len();
            list.push((depth, rest.to_string()));
            continue;
        }
        if let Some((num, rest)) = split_ordered(t) {
            flush_para(&mut para, &mut out);
            let depth = raw.len() - raw.trim_start().len();
            list.push((depth, format!("{num}. {rest}")));
            continue;
        }
        // paragraph continuation
        if !para.is_empty() {
            para.push(' ');
        }
        para.push_str(t);
    }
    flush_para(&mut para, &mut out);
    if !list.is_empty() {
        out.push(Block::List(list));
    }
    out
}

fn split_ordered(t: &str) -> Option<(usize, &str)> {
    let mut digits = String::new();
    for (i, c) in t.char_indices() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else if c == '.' && !digits.is_empty() {
            let rest = t[i + 1..].trim_start();
            if !rest.is_empty() {
                return Some((digits.parse().ok()?, rest));
            }
            return None;
        } else {
            return None;
        }
    }
    None
}

fn inline(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 32);
    let mut i = 0;
    while i < s.len() {
        // `i` always sits on a char boundary: it starts at 0 and every step
        // below advances by a whole number of characters (ASCII delimiter
        // lengths or one decoded char). All lookahead uses char_indices-style
        // `starts_with` on `&s[i..]` rather than raw byte indexing so that a
        // multibyte char can never be mistaken for syntax or cut in half.
        let rest = &s[i..];
        // **bold**
        if let Some(tail) = rest.strip_prefix("**") {
            if let Some(end) = tail.find("**") {
                out.push_str(BOLD);
                out.push_str(&inline(&tail[..end]));
                out.push_str(RST);
                i += 2 + end + 2;
                continue;
            }
        }
        // *italic*
        if let Some(tail) = rest.strip_prefix('*') {
            if let Some(end) = tail.find('*') {
                out.push_str(ITAL);
                out.push_str(&inline(&tail[..end]));
                out.push_str(RST);
                i += 1 + end + 1;
                continue;
            }
        }
        // `code`
        if let Some(tail) = rest.strip_prefix('`') {
            if let Some(end) = tail.find('`') {
                out.push_str(CODE);
                out.push_str(&tail[..end]);
                out.push_str(RST);
                i += 1 + end + 1;
                continue;
            }
        }
        // ~~strike~~
        if let Some(tail) = rest.strip_prefix("~~") {
            if let Some(end) = tail.find("~~") {
                out.push_str(DEL);
                out.push_str(&inline(&tail[..end]));
                out.push_str(RST);
                i += 2 + end + 2;
                continue;
            }
        }
        // [text](url)
        if let Some(tail) = rest.strip_prefix('[') {
            if let Some(close) = tail.find(']') {
                if let Some(after) = tail[close + 1..].strip_prefix('(') {
                    if let Some(end) = after.find(')') {
                        out.push_str("\x1b[4m");
                        out.push_str(&tail[..close]);
                        out.push_str(RST);
                        out.push_str(DIM);
                        out.push(' ');
                        out.push_str(&after[..end]);
                        out.push_str(RST);
                        i += 1 + close + 1 + 1 + end + 1;
                        continue;
                    }
                }
            }
        }
        // escape: a backslash consumes the next character (any width) and
        // emits it literally. A trailing lone backslash is kept verbatim.
        if let Some(tail) = rest.strip_prefix('\\') {
            match tail.chars().next() {
                Some(ch) => {
                    out.push(ch);
                    i += 1 + ch.len_utf8();
                }
                None => {
                    out.push('\\');
                    i += 1;
                }
            }
            continue;
        }
        // copy one whole UTF-8 char
        let ch = rest.chars().next().expect("i < s.len() guarantees a char");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn render(out: &mut impl Write, blocks: &[Block]) -> io::Result<()> {
    for b in blocks {
        match b {
            Block::Para(p) => {
                writeln!(out, "{}{}{}", inline(p), RST, RST)?;
            }
            Block::H(1, t) => writeln!(out, "{ACCENT}{BOLD}\n# {t}{RST}", t = inline(t))?,
            Block::H(2, t) => writeln!(out, "{ACCENT}{BOLD}## {t}{RST}", t = inline(t))?,
            Block::H(3, t) => writeln!(out, "{BOLD}### {t}{RST}", t = inline(t))?,
            Block::H(4, t) => writeln!(out, "{BOLD}#### {t}{RST}", t = inline(t))?,
            Block::H(_, t) => writeln!(out, "{BOLD}{t}{RST}", t = inline(t))?,
            Block::Hr => writeln!(out, "{DIM}────────────────────────────────{RST}")?,
            Block::CodeFence(lines) => {
                for l in lines {
                    writeln!(out, "{CODE}{l}{RST}")?;
                }
            }
            Block::List(items) => {
                for (depth, item) in items {
                    let indent = "  ".repeat(*depth / 2);
                    writeln!(out, "{indent}{ACCENT}•{RST} {}", inline(item))?;
                }
            }
            Block::Quote(q) => writeln!(out, "{QUOTE}{ITAL}{}{RST}", inline(q))?,
        }
    }
    Ok(())
}

const USAGE: &str = "usage: sifr-md [file.md|-]\n\nRender the documented Markdown subset as ANSI text. Reads stdin when no file is given.\n";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h") {
        print!("{USAGE}");
        return;
    }
    if args.len() == 1 && matches!(args[0].as_str(), "--version" | "-V") {
        println!("sifr-md 0.1.0");
        return;
    }
    if args.len() > 1
        || args
            .first()
            .is_some_and(|arg| arg.starts_with('-') && arg != "-")
    {
        eprintln!("sifr-md: {USAGE}");
        std::process::exit(2);
    }
    let file = args.first().filter(|arg| arg.as_str() != "-");
    let text = read_input(file.as_ref().map(|s| s.as_str()));
    let blocks = parse_blocks(&text);
    let mut out = io::stdout().lock();
    if let Err(error) = render(&mut out, &blocks) {
        if error.kind() == io::ErrorKind::BrokenPipe {
            return;
        }
        eprintln!("sifr-md: stdout write failed: {error}");
        std::process::exit(2);
    }
}

fn read_input(path: Option<&str>) -> String {
    match path {
        Some(path) => read_lossy(|| fs::read(path), || format!("cannot read {path}")),
        None => read_lossy(
            || {
                let mut buf = Vec::new();
                io::stdin().read_to_end(&mut buf)?;
                Ok(buf)
            },
            || "stdin read failed".to_string(),
        ),
    }
}

/// Read a byte source and convert it to text without panicking on invalid
/// UTF-8: valid input is taken as-is, malformed bytes become U+FFFD.
fn read_lossy(
    read: impl FnOnce() -> io::Result<Vec<u8>>,
    context: impl FnOnce() -> String,
) -> String {
    let label = context();
    let bytes = match read() {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("sifr-md: {label}: {e}");
            std::process::exit(2);
        }
    };
    match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_headings_and_hr() {
        let b = parse_blocks("# one\n\n## two\n\n---\n");
        assert_eq!(b[0], Block::H(1, "one".into()));
        assert_eq!(b[1], Block::H(2, "two".into()));
        assert_eq!(b[2], Block::Hr);
    }

    #[test]
    fn parses_code_fence_verbatim() {
        let b = parse_blocks("```\nlet x = 1;\n  indented\n```\n");
        assert_eq!(
            b[0],
            Block::CodeFence(vec!["let x = 1;".into(), "  indented".into()])
        );
    }

    #[test]
    fn inline_styles() {
        assert_eq!(inline("a **b** c"), format!("a {BOLD}b{RST} c"));
        assert_eq!(inline("*i*"), format!("{ITAL}i{RST}"));
        assert_eq!(inline("`c`"), format!("{CODE}c{RST}"));
        assert_eq!(inline("~~s~~"), format!("{DEL}s{RST}"));
    }

    #[test]
    fn inline_link() {
        let got = inline("[text](http://x)");
        assert!(got.contains("text"));
        assert!(got.contains("http://x"));
    }

    #[test]
    fn list_items_collected() {
        let b = parse_blocks("- a\n- b\n\npara\n");
        assert_eq!(b[0], Block::List(vec![(0, "a".into()), (0, "b".into())]));
        assert_eq!(b[1], Block::Para("para".into()));
    }

    #[test]
    fn paragraph_soft_wraps() {
        let b = parse_blocks("one\ntwo\nthree\n");
        assert_eq!(b[0], Block::Para("one two three".into()));
    }

    #[test]
    fn utf8_passthrough() {
        let b = parse_blocks("صفر — built from zero\n");
        assert_eq!(b[0], Block::Para("صفر — built from zero".into()));
    }

    #[test]
    fn multibyte_utf8_survives_inline() {
        for text in ["é", "—", "🙂", "صفر", "aé—🙂b", "é🙂é"] {
            assert_eq!(inline(text), text, "input {text:?}");
        }
    }

    #[test]
    fn escape_before_multibyte_char_emits_it_literally() {
        // Regression: `bytes[i + 1] as char` decoded a single byte and then
        // sliced `s[i..i + len]` mid-codepoint, panicking at src/main.rs:238.
        assert_eq!(inline(r"\é"), "é");
        assert_eq!(inline(r"\—"), "—");
        assert_eq!(inline(r"\🙂"), "🙂");
        assert_eq!(inline("a\\école"), "aécole");
        assert_eq!(inline("x\\🙂y"), "x🙂y");
        assert_eq!(inline("صفر \\🙂 صفر"), "صفر 🙂 صفر");
    }

    #[test]
    fn escape_suppresses_markdown_syntax() {
        // Intended escape behaviour preserved: the escaped character is literal.
        assert_eq!(inline(r"\*not italic\*"), "*not italic*");
        assert_eq!(inline(r"\*\*not bold\*\*"), "**not bold**");
        assert_eq!(inline(r"\`not code\`"), "`not code`");
        assert_eq!(inline(r"\~\~not strike\~\~"), "~~not strike~~");
        assert_eq!(inline(r"\[not a link\](x)"), "[not a link](x)");
        assert_eq!(inline(r"\\"), "\\");
        assert_eq!(inline(r"a\\é"), "a\\é");
    }

    #[test]
    fn trailing_backslash_is_literal() {
        assert_eq!(inline("\\"), "\\");
        assert_eq!(inline("done \\"), "done \\");
        assert_eq!(inline("🙂\\"), "🙂\\");
    }

    #[test]
    fn escape_inside_inline_constructs_is_unicode_safe() {
        assert_eq!(inline(r"**\é**"), format!("{BOLD}é{RST}"));
        assert_eq!(inline(r"*\—*"), format!("{ITAL}—{RST}"));
        assert_eq!(inline(r"`\🙂`"), format!("{CODE}\\🙂{RST}"));
        assert_eq!(inline(r"~~\é~~"), format!("{DEL}é{RST}"));
        // Link labels are emitted verbatim (no nested inline parsing), so a
        // backslash there stays literal — unchanged by this fix.
        assert_eq!(
            inline(r"[\é](http://é.example)"),
            format!("\x1b[4m\\é{RST}{DIM} http://é.example{RST}")
        );
    }

    #[test]
    fn inline_never_panics_on_arbitrary_char_boundaries() {
        // Every prefix/suffix combination of mixed-width text must render.
        let text = "aé—🙂*`~[]()\\# صفر";
        for start in text.char_indices().map(|(i, _)| i) {
            for end in text
                .char_indices()
                .map(|(i, _)| i)
                .chain(std::iter::once(text.len()))
            {
                if start <= end {
                    let _ = inline(&text[start..end]);
                }
            }
        }
    }

    #[test]
    fn invalid_utf8_is_lossily_replaced() {
        // Malformed bytes are not a panic or an error: U+FFFD replaces them.
        let broken = b"caf\xe9 \xff\xfe ok \\\xc3\xa9";
        let text = String::from_utf8_lossy(broken).into_owned();
        assert!(text.contains('\u{FFFD}'));
        let rendered = inline(&text);
        assert!(rendered.contains("caf\u{FFFD}"));
        assert!(
            rendered.contains('é'),
            "escape over repaired bytes survives"
        );
    }
}
