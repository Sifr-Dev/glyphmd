//! sifr-md — zero-dependency Markdown → ANSI terminal renderer.
//!
//!   sifr-md [file.md]         (stdin default)
//! Reads Markdown (CommonMark-ish subset) and writes styled bytes to stdout:
//!   headings (#..####), **bold**, *italic*, `code`, ``` fences, - lists,
//!   1. ordered lists, > blockquote, --- hr, [text](url), ~~strike~~
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
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // **bold**
        if bytes[i] == b'*' && bytes.get(i + 1) == Some(&b'*') {
            if let Some(end) = find_pair(s, i + 2, "**") {
                out.push_str(BOLD);
                out.push_str(&inline(&s[i + 2..end]));
                out.push_str(RST);
                i = end + 2;
                continue;
            }
        }
        // *italic*
        if bytes[i] == b'*' {
            if let Some(end) = find_pair(s, i + 1, "*") {
                out.push_str(ITAL);
                out.push_str(&inline(&s[i + 1..end]));
                out.push_str(RST);
                i = end + 1;
                continue;
            }
        }
        // `code`
        if bytes[i] == b'`' {
            if let Some(end) = find_pair(s, i + 1, "`") {
                out.push_str(CODE);
                out.push_str(&s[i + 1..end]);
                out.push_str(RST);
                i = end + 1;
                continue;
            }
        }
        // ~~strike~~
        if bytes[i] == b'~' && bytes.get(i + 1) == Some(&b'~') {
            if let Some(end) = find_pair(s, i + 2, "~~") {
                out.push_str(DEL);
                out.push_str(&inline(&s[i + 2..end]));
                out.push_str(RST);
                i = end + 2;
                continue;
            }
        }
        // [text](url)
        if bytes[i] == b'[' {
            if let Some(close) = find_pair(s, i + 1, "]") {
                if bytes.get(close + 1) == Some(&b'(') {
                    if let Some(end) = find_pair(s, close + 2, ")") {
                        out.push_str("\x1b[4m");
                        out.push_str(&s[i + 1..close]);
                        out.push_str(RST);
                        out.push_str(DIM);
                        out.push(' ');
                        out.push_str(&s[close + 2..end]);
                        out.push_str(RST);
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        // escape / literal char
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            out.push(bytes[i + 1] as char);
            i += 2;
            continue;
        }
        // copy one utf8 char
        let ch_len = utf8_len(bytes[i]);
        out.push_str(&s[i..i + ch_len]);
        i += ch_len;
    }
    out
}

fn find_pair(s: &str, from: usize, pat: &str) -> Option<usize> {
    s[from..].find(pat).map(|p| from + p)
}

fn utf8_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
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
    let text = match file {
        Some(path) => fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!("sifr-md: cannot read {path}: {e}");
            std::process::exit(2);
        }),
        None => {
            let mut s = String::new();
            io::stdin().read_to_string(&mut s).unwrap_or_else(|e| {
                eprintln!("sifr-md: stdin read failed: {e}");
                std::process::exit(2);
            });
            s
        }
    };
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
}
