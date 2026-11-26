<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset=".github/banner-dark.svg">
    <img alt="sifr-md — built from zero" src=".github/banner-light.svg" width="100%">
  </picture>
</p>

# sifr-md

zero-dependency Markdown → ANSI terminal renderer. ~350 lines of Rust.
no crates, no regex engine, no dynamic dispatch — one pass, one allocation
budget, one accent from the sifr brand (`#659eff` → ANSI 111).

```sh
sifr-md README.md
cat README.md | sifr-md
```

## subset

- headings `#`..`####`, `---` hr, paragraphs (soft-wrapped)
- lists `-`/`*` and `1.` ordered
- `**bold**` `*italic*` `` `code` `` `~~strike~~` `[text](url)`
- fenced code blocks (verbatim), `> blockquote`
- UTF-8 passthrough; `\<char>` escapes any character, multibyte included

## evidence (this machine, 2026-11-17)

```console
$ cargo test --release
test result: ok. 14 passed; 0 failed   (unit)
test result: ok. 11 passed; 0 failed   (cli)
$ ./target/release/sifr-md README.md | head -8   # renders this document
```

## honest scope

no nesting, no tables, no images, no raw HTML, no footnotes, no setext
headings — it renders the subset above and refuses to guess. for the full
CommonMark spec use a real parser; for reading a README in a terminal, this
is enough.

## build

```sh
cargo build --release && cargo test --release
```

## license

`MIT` — see `LICENSE`. built from zero.
