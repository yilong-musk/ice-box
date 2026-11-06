// SPDX-License-Identifier: GPL-3.0-or-later

//! Go `regexp` compatibility check for regex matchers.
//!
//! sing-box compiles `domain_regex`, `process_path_regex` and
//! `package_name_regex` with Go's `regexp.Compile`. A pattern Go rejects
//! stops the core from starting, so a rule-set must only carry patterns Go
//! is certain to accept. `regex-syntax` parses a superset of Go's syntax;
//! this module walks its AST and accepts only constructs whose Go form is
//! identical, mirrors Go's repeat-count limits, and bounds the compiled
//! size far below Go's own limits.
//!
//! The allow-list is intentionally conservative: rejecting an exotic but
//! valid pattern only drops one rule-set, while accepting one Go refuses
//! takes the whole proxy down.

use regex_syntax::ast::{
    self, AssertionKind, Ast, ClassSet, ClassSetItem, ClassUnicodeKind, Flag, FlagsItemKind,
    GroupKind, HexLiteralKind, LiteralKind, RepetitionKind, RepetitionRange, SpecialLiteralKind,
};
use regex_syntax::hir::{self, Hir, HirKind};

/// Go's limit on a single repeat count and on the product of nested
/// counted repeats (`regexp/syntax` `repeatIsValid`).
const GO_MAX_REPEAT: u32 = 1000;

/// AST nesting accepted; Go's own height limit is 1000.
const NEST_LIMIT: u32 = 64;

/// Upper bound on the estimated compiled size. Go refuses programs over
/// about 3.3M instructions or 33M runes; the estimate below counts every
/// class rune as an instruction, so this leaves a wide margin.
const MAX_COST: u64 = 1_000_000;

/// General categories in Go's `unicode.Categories`, plus `Any`.
const GO_CATEGORIES: &[&str] = &[
    "Any", "C", "Cc", "Cf", "Co", "Cs", "L", "Ll", "Lm", "Lo", "Lt", "Lu", "M", "Mc", "Me", "Mn",
    "N", "Nd", "Nl", "No", "P", "Pc", "Pd", "Pe", "Pf", "Pi", "Po", "Ps", "S", "Sc", "Sk", "Sm",
    "So", "Z", "Zl", "Zp", "Zs",
];

/// Long-standing scripts in Go's `unicode.Scripts`. Go matches names
/// exactly (Rust is case- and underscore-insensitive), so only these exact
/// spellings pass.
const GO_SCRIPTS: &[&str] = &[
    "Arabic",
    "Armenian",
    "Bengali",
    "Bopomofo",
    "Common",
    "Cyrillic",
    "Devanagari",
    "Ethiopic",
    "Georgian",
    "Greek",
    "Gujarati",
    "Gurmukhi",
    "Han",
    "Hangul",
    "Hebrew",
    "Hiragana",
    "Inherited",
    "Kannada",
    "Katakana",
    "Khmer",
    "Lao",
    "Latin",
    "Malayalam",
    "Mongolian",
    "Myanmar",
    "Oriya",
    "Sinhala",
    "Tamil",
    "Telugu",
    "Thai",
    "Tibetan",
];

/// One-letter Unicode classes Go accepts (`\pL`).
const GO_ONE_LETTER: &[char] = &['C', 'L', 'M', 'N', 'P', 'S', 'Z'];

/// Check that Go's `regexp.Compile` accepts `pattern` with the same
/// meaning. The error names the first offending construct.
pub(crate) fn check(pattern: &str) -> Result<(), String> {
    let ast = ast::parse::ParserBuilder::new()
        .nest_limit(NEST_LIMIT)
        .build()
        .parse(pattern)
        .map_err(|err| err.kind().to_string())?;
    let mut checker = Checker { pattern };
    checker.ast(&ast, GO_MAX_REPEAT)?;
    let hir = hir::translate::TranslatorBuilder::new()
        .build()
        .translate(pattern, &ast)
        .map_err(|err| err.kind().to_string())?;
    if cost(&hir) > MAX_COST {
        return Err("pattern compiles too large".into());
    }
    Ok(())
}

struct Checker<'a> {
    pattern: &'a str,
}

impl Checker<'_> {
    fn text(&self, span: &ast::Span) -> &str {
        self.pattern
            .get(span.start.offset..span.end.offset)
            .unwrap_or("")
    }

    /// `budget` is what remains of Go's nested repeat product limit.
    fn ast(&mut self, node: &Ast, budget: u32) -> Result<(), String> {
        match node {
            Ast::Empty(_) | Ast::Dot(_) => Ok(()),
            Ast::Flags(set) => self.flags(&set.flags),
            Ast::Literal(lit) => literal(lit),
            Ast::Assertion(assertion) => match assertion.kind {
                AssertionKind::StartLine
                | AssertionKind::EndLine
                | AssertionKind::StartText
                | AssertionKind::EndText
                | AssertionKind::WordBoundary
                | AssertionKind::NotWordBoundary => Ok(()),
                _ => Err(format!("assertion {}", self.text(&assertion.span))),
            },
            Ast::ClassUnicode(class) => self.unicode_class(class),
            Ast::ClassPerl(_) => Ok(()),
            Ast::ClassBracketed(class) => self.bracketed(class),
            Ast::Repetition(rep) => self.repetition(rep, budget),
            Ast::Group(group) => {
                match &group.kind {
                    GroupKind::CaptureIndex(_) => {}
                    GroupKind::CaptureName {
                        starts_with_p,
                        name,
                    } => {
                        let valid = !name.name.is_empty()
                            && name
                                .name
                                .bytes()
                                .all(|b| b == b'_' || b.is_ascii_alphanumeric());
                        if !starts_with_p || !valid {
                            return Err(format!("capture name {:?}", name.name));
                        }
                    }
                    GroupKind::NonCapturing(flags) => self.flags(flags)?,
                }
                self.ast(&group.ast, budget)
            }
            Ast::Alternation(alt) => alt.asts.iter().try_for_each(|a| self.ast(a, budget)),
            Ast::Concat(concat) => concat.asts.iter().try_for_each(|a| self.ast(a, budget)),
        }
    }

    /// Go accepts the flags `i`, `m`, `s` and `U`, with at most one `-`
    /// that must be followed by a flag.
    fn flags(&self, flags: &ast::Flags) -> Result<(), String> {
        let mut after_negation = None;
        for item in &flags.items {
            match item.kind {
                FlagsItemKind::Negation => {
                    if after_negation.is_some() {
                        return Err("repeated flag negation".into());
                    }
                    after_negation = Some(0);
                }
                FlagsItemKind::Flag(flag) => {
                    if !matches!(
                        flag,
                        Flag::CaseInsensitive
                            | Flag::MultiLine
                            | Flag::DotMatchesNewLine
                            | Flag::SwapGreed
                    ) {
                        return Err(format!("flag in {}", self.text(&flags.span)));
                    }
                    if let Some(count) = after_negation.as_mut() {
                        *count += 1;
                    }
                }
            }
        }
        if after_negation == Some(0) {
            return Err("dangling flag negation".into());
        }
        Ok(())
    }

    fn unicode_class(&self, class: &ast::ClassUnicode) -> Result<(), String> {
        let ok = match &class.kind {
            ClassUnicodeKind::OneLetter(c) => GO_ONE_LETTER.contains(c),
            ClassUnicodeKind::Named(name) => {
                // Compare against the raw braces so that no Rust-only
                // spelling (spaces, case folding) slips through. `\p{^X}`
                // is valid Go, but Rust cannot translate it, so it fails.
                let raw = self.text(&class.span);
                let inner = raw.get(3..raw.len().saturating_sub(1)).unwrap_or("");
                inner == name
                    && (GO_CATEGORIES.contains(&name.as_str())
                        || GO_SCRIPTS.contains(&name.as_str()))
            }
            ClassUnicodeKind::NamedValue { .. } => false,
        };
        if ok {
            Ok(())
        } else {
            Err(format!("unicode class {}", self.text(&class.span)))
        }
    }

    fn bracketed(&self, class: &ast::ClassBracketed) -> Result<(), String> {
        match &class.kind {
            ClassSet::Item(item) => self.class_item(item, true),
            ClassSet::BinaryOp(_) => Err(format!("class operator in {}", self.text(&class.span))),
        }
    }

    /// Go has no nested classes or set operators; only flat unions of
    /// literals, ranges and named classes are accepted.
    fn class_item(&self, item: &ClassSetItem, top: bool) -> Result<(), String> {
        match item {
            ClassSetItem::Empty(_) | ClassSetItem::Ascii(_) | ClassSetItem::Perl(_) => Ok(()),
            ClassSetItem::Literal(lit) => class_literal(lit),
            ClassSetItem::Range(range) => {
                class_literal(&range.start)?;
                class_literal(&range.end)
            }
            ClassSetItem::Unicode(class) => self.unicode_class(class),
            ClassSetItem::Union(union) if top => union
                .items
                .iter()
                .try_for_each(|item| self.class_item(item, false)),
            ClassSetItem::Union(_) | ClassSetItem::Bracketed(_) => Err("nested class".into()),
        }
    }

    fn repetition(&mut self, rep: &ast::Repetition, budget: u32) -> Result<(), String> {
        match rep.ast.as_ref() {
            Ast::Repetition(_) => return Err("nested repetition operator".into()),
            Ast::Empty(_) | Ast::Flags(_) => return Err("repetition without operand".into()),
            _ => {}
        }
        let budget = match &rep.op.kind {
            RepetitionKind::ZeroOrOne | RepetitionKind::ZeroOrMore | RepetitionKind::OneOrMore => {
                budget
            }
            RepetitionKind::Range(range) => {
                // Go only reads `{n}`, `{n,}` and `{n,m}` with plain
                // digits; anything else is a literal there.
                let raw = self.text(&rep.op.span);
                let raw = raw.strip_suffix('?').unwrap_or(raw);
                let body = raw
                    .strip_prefix('{')
                    .and_then(|r| r.strip_suffix('}'))
                    .unwrap_or("");
                let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
                let well_formed = match body.split_once(',') {
                    None => digits(body),
                    Some((min, max)) => digits(min) && (max.is_empty() || digits(max)),
                };
                if !well_formed {
                    return Err(format!("repeat {raw}"));
                }
                let (min, max) = match *range {
                    RepetitionRange::Exactly(n) => (n, Some(n)),
                    RepetitionRange::AtLeast(n) => (n, None),
                    RepetitionRange::Bounded(a, b) => (a, Some(b)),
                };
                if min > GO_MAX_REPEAT || max.is_some_and(|m| m > GO_MAX_REPEAT) {
                    return Err(format!("repeat count over {GO_MAX_REPEAT} in {raw}"));
                }
                // Mirror `repeatIsValid`: a zero max ends the check.
                let m = max.unwrap_or(min);
                if max == Some(0) {
                    return self.ast(&rep.ast, budget);
                }
                if m > budget {
                    return Err(format!("nested repeat counts exceed {GO_MAX_REPEAT}"));
                }
                if m > 0 {
                    budget / m
                } else {
                    budget
                }
            }
        };
        self.ast(&rep.ast, budget)
    }
}

fn literal(lit: &ast::Literal) -> Result<(), String> {
    let ok = match &lit.kind {
        LiteralKind::Verbatim | LiteralKind::Meta | LiteralKind::Superfluous => true,
        LiteralKind::HexFixed(kind) | LiteralKind::HexBrace(kind) => *kind == HexLiteralKind::X,
        LiteralKind::Special(kind) => *kind != SpecialLiteralKind::Space,
        LiteralKind::Octal => false,
    };
    // Go treats escaped ASCII punctuation as itself; escaped letters and
    // digits are only the forms listed above.
    if ok && !(lit.kind == LiteralKind::Superfluous && lit.c.is_ascii_alphanumeric()) {
        Ok(())
    } else {
        Err(format!("escape for {:?}", lit.c))
    }
}

/// Inside a class, Go treats `[` and `-` specially, and Rust uses `&`,
/// `~` and `-` for set operators; keep those escaped-or-absent semantics
/// identical by requiring the plain literal forms above.
fn class_literal(lit: &ast::Literal) -> Result<(), String> {
    if lit.kind == LiteralKind::Verbatim && lit.c == '[' {
        return Err("unescaped [ in class".into());
    }
    literal(lit)
}

/// Estimated compiled size: one unit per literal byte, two per class range
/// bound, multiplied through repetitions.
fn cost(hir: &Hir) -> u64 {
    match hir.kind() {
        HirKind::Empty | HirKind::Look(_) => 1,
        HirKind::Literal(lit) => lit.0.len() as u64,
        HirKind::Class(hir::Class::Unicode(class)) => 1 + 2 * class.ranges().len() as u64,
        HirKind::Class(hir::Class::Bytes(class)) => 1 + 2 * class.ranges().len() as u64,
        HirKind::Repetition(rep) => {
            let times = u64::from(rep.max.unwrap_or(rep.min.max(1)).max(1));
            cost(&rep.sub).saturating_mul(times).saturating_add(2)
        }
        HirKind::Capture(cap) => cost(&cap.sub).saturating_add(2),
        HirKind::Concat(subs) => subs.iter().map(cost).fold(0, u64::saturating_add),
        HirKind::Alternation(subs) => subs
            .iter()
            .map(cost)
            .fold(subs.len() as u64, u64::saturating_add),
    }
}

#[cfg(test)]
mod tests {
    use super::check;

    /// Patterns Go compiles; all must pass.
    pub(crate) const ACCEPTED: &[&str] = &[
        r"^a+$",
        r"(^|\.)18tv[1-5]\.com$",
        r"^(.+\.)?xn--[a-z0-9]{1,63}$",
        r"a{1000}",
        r"a{0,1000}",
        r"a{3,}",
        r"(a{10}){100}",
        r"(a{1000}){0}x",
        r"(?i)google\.com",
        r"(?i:abc)x",
        r"(?is-mU:x)",
        r"(?P<host>[a-z]+)\.com",
        r"\pL+",
        r"\p{L}\p{Lu}\p{Any}",
        r"\p{Han}+",
        r"\P{L}",
        r"\PL",
        r"[\p{Latin}\d_-]+",
        r"[[:alpha:][:^digit:]]",
        r"\x41\x{4E2D}",
        r"\a\f\t\n\r\v",
        r"\A\bfoo\B\z",
        r"a*?b+?c??d{2,3}?",
        r"[^\]a-]",
        r"[]a]",
        r"\.\+\*\?\(\)\|\[\]\{\}\^\$\#\&\-\~",
        r"\%\/\!",
        r"a|",
        r"()",
        r".*\W\S\D",
        r"[\x{4E00}-\x{9FFF}]",
    ];

    /// Patterns `regex-syntax` parses but Go refuses; all must be rejected.
    pub(crate) const GO_REJECTS: &[&str] = &[
        r"a{1001}",
        r"(a{10}){101}",
        r"((a{10}){10}){11}",
        r"\p{Greekk}",
        r"\p{sc=Greek}",
        r"\p{Alphabetic}",
        r"\u0041",
        r"\U00000041",
        r"(?x)a b",
        r"(?u)a",
        r"(?R)a",
        r"(?P<na.me>a)",
    ];

    /// Valid Go, rejected anyway: Go reads them with a different meaning
    /// (`&&`, `~~`, `--` and nested `[` are literals there, `\b{start}` is
    /// `\b` then a literal), Rust cannot translate them, or the spelling
    /// depends on the Go version.
    const CONSERVATIVE: &[&str] = &[
        r"[a&&b]",
        r"[a~~b]",
        r"[[a-z]--[aeiou]]",
        r"\b{start}x",
        r"\<a\>",
        r"\p{greek}",
        r"\p{^Greek}",
        r"\p{Adlam}",
        r"(?<name>a)",
        r"\Qa.b\E",
    ];

    #[test]
    fn accepts_go_compatible_patterns() {
        for pattern in ACCEPTED {
            assert_eq!(check(pattern), Ok(()), "{pattern}");
        }
    }

    #[test]
    fn rejects_patterns_go_refuses() {
        for pattern in GO_REJECTS.iter().chain(CONSERVATIVE) {
            assert!(check(pattern).is_err(), "{pattern:?}");
        }
    }

    #[test]
    fn rejects_oversized_programs() {
        let big = format!("({}){{1000}}", r"\pL".repeat(20));
        assert!(check(&big).is_err());
    }

    /// Differential check against the real core: everything accepted here
    /// must compile in sing-box, everything in `GO_REJECTS` must fail there.
    /// Set `ICE_SING_BOX_BIN` to a sing-box binary to run it.
    #[test]
    fn matches_sing_box_when_provided() {
        let Some(bin) = std::env::var_os("ICE_SING_BOX_BIN") else {
            return;
        };
        let dir = std::env::temp_dir().join(format!("ice-go-regex-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let run = |name: &str, patterns: &[&str]| {
            let file = dir.join(name);
            let body = serde_json::json!({"version": 3, "rules": [{"domain_regex": patterns}]});
            std::fs::write(&file, body.to_string()).unwrap();
            // Run from the directory with a relative path so that a Windows
            // binary under WSL can open the file too.
            let out = std::process::Command::new(&bin)
                .current_dir(&dir)
                .args(["rule-set", "match", "-f", "source", name, "example.com"])
                .output()
                .expect("run sing-box");
            (
                out.status.success(),
                String::from_utf8_lossy(&out.stderr).into_owned(),
            )
        };
        let (ok, err) = run("accepted.json", ACCEPTED);
        assert!(ok, "sing-box refused an accepted pattern: {err}");
        let accepted_by_go: Vec<&str> = GO_REJECTS
            .iter()
            .enumerate()
            .filter(|(idx, pattern)| run(&format!("reject-{idx}.json"), &[pattern]).0)
            .map(|(_, pattern)| *pattern)
            .collect();
        assert!(
            accepted_by_go.is_empty(),
            "sing-box accepts {accepted_by_go:?}; move them to CONSERVATIVE"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
