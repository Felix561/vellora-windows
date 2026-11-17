//! Output guard for Quick Polish (spec sections 5.2 and 5.3): whitespace preparation and
//! restoration, plausibility checks on the model output, and a token-level word diff.
//!
//! Everything works on `char`s and char boundaries, never on raw UTF-8 bytes, and never
//! normalizes Unicode. All scanners are hand written and linear (or bounded) in the input.

#![allow(dead_code)]

use std::collections::HashMap;
use std::ops::Range;

use serde::Serialize;

// ------------------------------------------------------------------------------------------
// Whitespace and line endings (spec 5.3)
// ------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedSelection {
    pub core: String,
    pub leading: String,
    pub trailing: String,
    pub line_ending: LineEnding,
}

/// Splits the selection into leading whitespace, core and trailing whitespace
/// (`char::is_whitespace`) and detects the dominant line ending.
pub fn prepare(selection: &str) -> PreparedSelection {
    let without_leading = selection.trim_start();
    let leading_len = selection.len() - without_leading.len();
    let core = without_leading.trim_end();
    PreparedSelection {
        core: core.to_string(),
        leading: selection[..leading_len].to_string(),
        trailing: without_leading[core.len()..].to_string(),
        line_ending: dominant_line_ending(selection),
    }
}

/// CRLF when `\r\n` occurrences outnumber lone `\n`; otherwise LF.
fn dominant_line_ending(text: &str) -> LineEnding {
    let mut crlf = 0usize;
    let mut lf = 0usize;
    let mut prev = 0u8;
    for &byte in text.as_bytes() {
        if byte == b'\n' {
            if prev == b'\r' {
                crlf += 1;
            } else {
                lf += 1;
            }
        }
        prev = byte;
    }
    if crlf > lf {
        LineEnding::CrLf
    } else {
        LineEnding::Lf
    }
}

/// Trims the model output's own outer whitespace, normalizes its line endings to the
/// original's dominant style, and re-attaches the original leading/trailing whitespace.
pub fn restore(prepared: &PreparedSelection, model_output: &str) -> String {
    let core = model_output.trim();
    let mut out =
        String::with_capacity(prepared.leading.len() + core.len() + prepared.trailing.len() + 16);
    out.push_str(&prepared.leading);
    let newline = match prepared.line_ending {
        LineEnding::Lf => "\n",
        LineEnding::CrLf => "\r\n",
    };
    let mut chars = core.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' if chars.peek() == Some(&'\n') => {
                chars.next();
                out.push_str(newline);
            }
            '\n' => out.push_str(newline),
            _ => out.push(c),
        }
    }
    out.push_str(&prepared.trailing);
    out
}

fn to_lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

// ------------------------------------------------------------------------------------------
// Guard checks (spec 5.2)
// ------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardIssue {
    Incomplete,
    Empty,
    Preamble,
    Wrapper,
    ProtectedTokens,
    LineCount,
    TooManyChanges,
    LengthChange,
}

impl GuardIssue {
    /// History/warning code for this issue, e.g. `guard_markup`.
    pub fn reason_code(self) -> &'static str {
        match self {
            GuardIssue::Incomplete => "guard_incomplete",
            GuardIssue::Empty => "guard_empty",
            GuardIssue::Preamble => "guard_preamble",
            GuardIssue::Wrapper => "guard_wrapper",
            GuardIssue::ProtectedTokens => "guard_markup",
            GuardIssue::LineCount => "guard_line_count",
            GuardIssue::TooManyChanges => "guard_too_many_changes",
            GuardIssue::LengthChange => "guard_length",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardReport {
    pub issues: Vec<GuardIssue>,
    pub changed_words: usize,
    pub original_words: usize,
    pub fixes: usize,
}

impl GuardReport {
    pub fn passed(&self) -> bool {
        self.issues.is_empty()
    }

    /// First issue as a history reason code: "guard_incomplete", "guard_empty", "guard_preamble",
    /// "guard_wrapper", "guard_markup", "guard_line_count", "guard_too_many_changes", "guard_length".
    pub fn reason_code(&self) -> Option<&'static str> {
        self.issues.first().map(|issue| issue.reason_code())
    }
}

/// `original_core`/`result_core` are whitespace-trimmed (from prepare / model output).
/// `completed` is false when the provider reported truncation, refusal or a filter stop.
///
/// Both texts are compared with CRLF normalized to LF. Issues are listed in the order of
/// the [`GuardIssue`] variants, so the first one is the most fundamental.
pub fn check(original_core: &str, result_core: &str, completed: bool) -> GuardReport {
    let original = to_lf(original_core.trim());
    let result = to_lf(result_core.trim());
    let stats = DiffStats::compute(&original, &result);
    let mut issues = Vec::new();
    if !completed {
        issues.push(GuardIssue::Incomplete);
    }
    if result.is_empty() {
        issues.push(GuardIssue::Empty);
        return stats.into_report(issues);
    }
    if has_preamble(&original, &result) {
        issues.push(GuardIssue::Preamble);
    }
    if is_wrapped(&result) && !is_wrapped(&original) {
        issues.push(GuardIssue::Wrapper);
    }
    if protected_tokens(&original) != protected_tokens(&result) {
        issues.push(GuardIssue::ProtectedTokens);
    }
    if original.matches('\n').count() != result.matches('\n').count() {
        issues.push(GuardIssue::LineCount);
    }
    // max(2, ceil(0.4 * words))
    let word_limit = ((2 * stats.original_words).div_ceil(5)).max(2);
    if stats.changed_words > word_limit {
        issues.push(GuardIssue::TooManyChanges);
    }
    let original_len = original.chars().count();
    let length_delta = original_len.abs_diff(result.chars().count());
    // |delta| > max(10, 0.3 * len)
    if length_delta > 10 && length_delta * 10 > original_len * 3 {
        issues.push(GuardIssue::LengthChange);
    }
    stats.into_report(issues)
}

/// Phrases a model uses to introduce its answer, matched against the lowercased first line.
const PREAMBLES: [&str; 18] = [
    "here is",
    "here's",
    "here’s",
    "corrected text",
    "corrected version",
    "the corrected",
    "sure,",
    "sure!",
    "certainly",
    "of course",
    "hier ist",
    "hier sind",
    "hier die",
    "korrigierte",
    "korrigierter",
    "gerne",
    "natürlich,",
    "selbstverständlich",
];

fn first_line_lowercase(text: &str) -> String {
    text.lines().next().unwrap_or("").trim().to_lowercase()
}

/// The result's first line starts with a preamble phrase that the original's first line
/// does not start with. For the original, the phrase's trailing `,`/`!` is ignored, so
/// "Sure that works" -> "Sure, that works" is a comma fix, not a preamble.
fn has_preamble(original: &str, result: &str) -> bool {
    let result_line = first_line_lowercase(result);
    let original_line = first_line_lowercase(original);
    PREAMBLES.iter().any(|phrase| {
        result_line.starts_with(phrase)
            && !original_line.starts_with(phrase.trim_end_matches([',', '!']))
    })
}

const QUOTE_PAIRS: [(char, char); 6] = [
    ('"', '"'),
    ('\'', '\''),
    ('“', '”'),
    ('„', '“'),
    ('«', '»'),
    ('‚', '‘'),
];

/// Wrapped in a matching quote pair, or starting with a code fence.
fn is_wrapped(text: &str) -> bool {
    if text.starts_with("```") {
        return true;
    }
    let mut chars = text.chars();
    let (Some(first), Some(last)) = (chars.next(), chars.next_back()) else {
        return false;
    };
    QUOTE_PAIRS
        .iter()
        .any(|&(open, close)| first == open && last == close)
}

// ------------------------------------------------------------------------------------------
// Protected tokens
// ------------------------------------------------------------------------------------------

/// Protected tokens of `text`, in order (see the rules in the M2/M3 plan, Task B rule 5).
///
/// LaTeX mode is on when the text contains a LaTeX command or a `$`-delimited math segment
/// outside code spans and URLs. Outside LaTeX mode only backtick code, URLs and e-mail
/// addresses are protected.
pub fn protected_tokens(text: &str) -> Vec<String> {
    let latex = Scanner::new(text, true).run();
    let found = if latex.iter().any(|t| t.kind != ProtKind::Other) {
        latex
    } else {
        Scanner::new(text, false).run()
    };
    found.into_iter().map(|t| t.text.to_string()).collect()
}

/// Upper bound for a brace or bracket argument search, keeps the scanner linear.
const MAX_ARG_CHARS: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProtKind {
    Command,
    DollarMath,
    Other,
}

#[derive(Debug)]
struct ProtToken<'a> {
    kind: ProtKind,
    text: &'a str,
}

#[derive(Clone, Copy)]
enum Delim {
    /// `\(…\)`
    Paren,
    /// `\[…\]`
    Bracket,
    /// `$$…$$`
    Double,
    /// `$…$`
    Single,
}

struct Scanner<'a> {
    text: &'a str,
    chars: Vec<char>,
    /// Byte offset of each char, plus `text.len()` at the end.
    offsets: Vec<usize>,
    latex: bool,
    out: Vec<ProtToken<'a>>,
    last_end: usize,
    /// A failed delimiter search from position `i` that stopped at `limit` means every
    /// later opener of the same kind before `limit` fails too; these memos keep the scan linear.
    fail_until: [usize; 4],
    fail_ticks: HashMap<usize, usize>,
}

impl<'a> Scanner<'a> {
    fn new(text: &'a str, latex: bool) -> Self {
        let mut chars = Vec::with_capacity(text.len());
        let mut offsets = Vec::with_capacity(text.len() + 1);
        for (offset, c) in text.char_indices() {
            chars.push(c);
            offsets.push(offset);
        }
        offsets.push(text.len());
        Scanner {
            text,
            chars,
            offsets,
            latex,
            out: Vec::new(),
            last_end: 0,
            fail_until: [0; 4],
            fail_ticks: HashMap::new(),
        }
    }

    fn at(&self, i: usize) -> Option<char> {
        self.chars.get(i).copied()
    }

    fn push(&mut self, kind: ProtKind, start: usize, end: usize) {
        let text = &self.text[self.offsets[start]..self.offsets[end]];
        self.out.push(ProtToken { kind, text });
        self.last_end = end;
    }

    fn run(mut self) -> Vec<ProtToken<'a>> {
        let n = self.chars.len();
        let mut i = 0;
        while i < n {
            let c = self.chars[i];
            if self.latex {
                match c {
                    '\\' => {
                        i = self.backslash(i);
                        continue;
                    }
                    '$' => {
                        i = if self.at(i + 1) == Some('$') {
                            self.delimited(i, 2, Delim::Double)
                        } else {
                            self.delimited(i, 1, Delim::Single)
                        };
                        continue;
                    }
                    '%' => {
                        i = self.comment(i);
                        continue;
                    }
                    _ => {}
                }
            }
            if c == '`' {
                i = self.backticks(i);
                continue;
            }
            if matches!(c, 'h' | 'H' | 'w' | 'W') {
                if let Some(end) = self.url_end(i) {
                    self.push(ProtKind::Other, i, end);
                    i = end;
                    continue;
                }
            }
            if c == '@' {
                if let Some((start, end)) = self.email(i) {
                    self.push(ProtKind::Other, start, end);
                    i = end;
                    continue;
                }
            }
            i += 1;
        }
        self.out
    }

    /// `chars[k]` is `\n` and only spaces, tabs or `\r` separate it from the next `\n`.
    fn blank_line_at(&self, k: usize) -> bool {
        if self.chars[k] != '\n' {
            return false;
        }
        let mut j = k + 1;
        while let Some(c) = self.at(j) {
            match c {
                '\n' => return true,
                ' ' | '\t' | '\r' => j += 1,
                _ => return false,
            }
        }
        false
    }

    fn backslash(&mut self, i: usize) -> usize {
        match self.at(i + 1) {
            Some('(') => self.delimited(i, 2, Delim::Paren),
            Some('[') => self.delimited(i, 2, Delim::Bracket),
            Some(c) if c.is_ascii_alphabetic() || c == '@' => self.command(i),
            // An escaped character such as `\%`, `\$` or `\\`.
            Some(_) => i + 2,
            None => i + 1,
        }
    }

    /// End (exclusive) of a closing delimiter starting at `k`, if there is one.
    fn closer_at(&self, k: usize, delim: Delim) -> Option<usize> {
        let c = self.chars[k];
        let next = self.at(k + 1);
        match delim {
            Delim::Paren => (c == '\\' && next == Some(')')).then_some(k + 2),
            Delim::Bracket => (c == '\\' && next == Some(']')).then_some(k + 2),
            Delim::Double => (c == '$' && next == Some('$')).then_some(k + 2),
            // A `$` directly followed by a digit does not close inline math ("$5 and $10").
            Delim::Single => {
                (c == '$' && !next.is_some_and(|d| d.is_ascii_digit())).then_some(k + 1)
            }
        }
    }

    /// A math segment opened at `i` (opener `open_len` chars long). Math never spans a
    /// blank line; an unmatched opener is plain text.
    fn delimited(&mut self, i: usize, open_len: usize, delim: Delim) -> usize {
        let memo = delim as usize;
        if i < self.fail_until[memo] {
            return i + open_len;
        }
        let n = self.chars.len();
        let mut k = i + open_len;
        while k < n {
            if let Some(end) = self.closer_at(k, delim) {
                let kind = match delim {
                    Delim::Double | Delim::Single => ProtKind::DollarMath,
                    Delim::Paren | Delim::Bracket => ProtKind::Other,
                };
                self.push(kind, i, end);
                return end;
            }
            match self.chars[k] {
                '\\' => k += 2,
                '\n' if self.blank_line_at(k) => break,
                _ => k += 1,
            }
        }
        self.fail_until[memo] = k.min(n);
        i + open_len
    }

    /// `\name` or `\name*`; key-taking commands also protect their brace argument.
    fn command(&mut self, i: usize) -> usize {
        let mut j = i + 1;
        while self
            .at(j)
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '@')
        {
            j += 1;
        }
        let name: String = self.chars[i + 1..j].iter().collect();
        if self.at(j) == Some('*') {
            j += 1;
        }
        self.push(ProtKind::Command, i, j);
        if !takes_key_argument(&name) {
            return j;
        }
        let mut k = self.skip_spaces(j);
        // Up to two optional arguments (natbib: \citep[see][p.~5]{key}).
        for _ in 0..2 {
            if self.at(k) != Some('[') {
                break;
            }
            match self.matching(k, '[', ']') {
                Some(close) => k = self.skip_spaces(close + 1),
                None => return j,
            }
        }
        if self.at(k) == Some('{') {
            if let Some(close) = self.matching(k, '{', '}') {
                self.push(ProtKind::Other, k, close + 1);
                return close + 1;
            }
        }
        j
    }

    fn skip_spaces(&self, mut k: usize) -> usize {
        while matches!(self.at(k), Some(' ' | '\t')) {
            k += 1;
        }
        k
    }

    /// Index of the bracket closing the one at `open_idx` (nesting, `\x` escapes), searched
    /// within [`MAX_ARG_CHARS`].
    fn matching(&self, open_idx: usize, open: char, close: char) -> Option<usize> {
        let limit = self.chars.len().min(open_idx + MAX_ARG_CHARS);
        let mut depth = 0usize;
        let mut k = open_idx;
        while k < limit {
            let c = self.chars[k];
            if c == '\\' {
                k += 2;
                continue;
            }
            if c == open {
                depth += 1;
            } else if c == close {
                depth -= 1;
                if depth == 0 {
                    return Some(k);
                }
            }
            k += 1;
        }
        None
    }

    /// `%` to the end of the line (without the line ending).
    fn comment(&mut self, i: usize) -> usize {
        let n = self.chars.len();
        let mut end = i + 1;
        while end < n && self.chars[end] != '\n' {
            end += 1;
        }
        let token_end = if end > i + 1 && self.chars[end - 1] == '\r' {
            end - 1
        } else {
            end
        };
        self.push(ProtKind::Other, i, token_end);
        end
    }

    fn run_length(&self, start: usize, c: char) -> usize {
        let mut end = start;
        while self.at(end) == Some(c) {
            end += 1;
        }
        end - start
    }

    /// Code in a run of backticks closed by a run of the same length. Runs of one or two
    /// never span a blank line. In LaTeX mode, a run of one or two closed by apostrophes
    /// first (``quote'' or `quote') is a TeX quotation mark, not code.
    fn backticks(&mut self, i: usize) -> usize {
        let len = self.run_length(i, '`');
        let after = i + len;
        if self.fail_ticks.get(&len).is_some_and(|&limit| i < limit) {
            return after;
        }
        let tex_quote = self.latex && len <= 2;
        let n = self.chars.len();
        let mut k = after;
        while k < n {
            match self.chars[k] {
                '`' => {
                    let run = self.run_length(k, '`');
                    if run == len {
                        self.push(ProtKind::Other, i, k + run);
                        return k + run;
                    }
                    k += run;
                }
                '\'' if tex_quote => {
                    let run = self.run_length(k, '\'');
                    if run == len {
                        return after;
                    }
                    k += run;
                }
                '\n' if len < 3 && self.blank_line_at(k) => break,
                _ => k += 1,
            }
        }
        self.fail_ticks.insert(len, k.min(n));
        after
    }

    fn starts_with_ignore_case(&self, i: usize, prefix: &str) -> bool {
        prefix.chars().enumerate().all(|(offset, p)| {
            self.at(i + offset)
                .is_some_and(|c| c.eq_ignore_ascii_case(&p))
        })
    }

    /// A URL starting at `i` (`http://`, `https://`, `www.`) up to whitespace, without
    /// trailing `.,;:!?)`.
    fn url_end(&self, i: usize) -> Option<usize> {
        if i > 0 && self.chars[i - 1].is_alphanumeric() {
            return None;
        }
        let prefix_len = ["https://", "http://", "www."]
            .iter()
            .find(|prefix| self.starts_with_ignore_case(i, prefix))?
            .len();
        let n = self.chars.len();
        let mut end = i;
        while end < n && !self.chars[end].is_whitespace() {
            end += 1;
        }
        while end > i + prefix_len && ".,;:!?)".contains(self.chars[end - 1]) {
            end -= 1;
        }
        (end > i + prefix_len).then_some(end)
    }

    /// An e-mail address around the `@` at `at`: `local@label.label…tld`.
    fn email(&self, at: usize) -> Option<(usize, usize)> {
        let is_local = |c: char| c.is_alphanumeric() || "._%+-".contains(c);
        let mut start = at;
        while start > self.last_end && is_local(self.chars[start - 1]) {
            start -= 1;
        }
        if start == at {
            return None;
        }
        let n = self.chars.len();
        let mut end = at + 1;
        while end < n && (self.chars[end].is_alphanumeric() || matches!(self.chars[end], '.' | '-'))
        {
            end += 1;
        }
        while end > at + 1 && matches!(self.chars[end - 1], '.' | '-') {
            end -= 1;
        }
        let domain = &self.chars[at + 1..end];
        let labels: Vec<&[char]> = domain.split(|&c| c == '.').collect();
        let tld = labels.last()?;
        let valid = labels.len() >= 2
            && labels.iter().all(|label| !label.is_empty())
            && tld.len() >= 2
            && tld.iter().all(|c| c.is_alphabetic());
        valid.then_some((start, end))
    }
}

fn takes_key_argument(name: &str) -> bool {
    matches!(
        name,
        "ref" | "eqref" | "label" | "input" | "include" | "begin" | "end"
    ) || name.starts_with("cite")
}

// ------------------------------------------------------------------------------------------
// Tokenizer and word diff
// ------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "op", content = "text", rename_all = "snake_case")]
pub enum DiffOp {
    Equal(String),
    Insert(String),
    Delete(String),
}

/// Token-level diff (words, single punctuation chars, whitespace runs), Myers O(ND),
/// adjacent ops of the same kind merged. Falls back to one Delete + one Insert when the
/// edit distance exceeds 4,000 tokens.
///
/// Within each change hunk the Delete comes before the Insert.
pub fn word_diff(original: &str, result: &str) -> Vec<DiffOp> {
    let diff = TokenDiff::compute(original, result);
    let mut ops = Vec::new();
    for seg in &diff.segments {
        let a_text = slice(original, &diff.a, &seg.a);
        if seg.equal {
            ops.push(DiffOp::Equal(a_text.to_string()));
            continue;
        }
        if !a_text.is_empty() {
            ops.push(DiffOp::Delete(a_text.to_string()));
        }
        let b_text = slice(result, &diff.b, &seg.b);
        if !b_text.is_empty() {
            ops.push(DiffOp::Insert(b_text.to_string()));
        }
    }
    ops
}

/// Maximum token edit distance computed exactly; beyond it the diff falls back.
const MAX_EDIT_DISTANCE: usize = 4_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokKind {
    Word,
    Space,
    Punct,
}

/// A token as a byte range of its text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tok {
    start: usize,
    end: usize,
    kind: TokKind,
}

/// Combining marks, joiners, variation selectors and emoji modifiers attach to the
/// preceding token, so grapheme clusters are never split.
fn is_attaching(c: char) -> bool {
    matches!(c,
        '\u{0300}'..='\u{036F}'
        | '\u{1AB0}'..='\u{1AFF}'
        | '\u{1DC0}'..='\u{1DFF}'
        | '\u{20D0}'..='\u{20FF}'
        | '\u{FE20}'..='\u{FE2F}'
        | '\u{200C}' | '\u{200D}'
        | '\u{FE00}'..='\u{FE0F}'
        | '\u{1F3FB}'..='\u{1F3FF}'
        | '\u{E0020}'..='\u{E007F}'
        | '\u{E0100}'..='\u{E01EF}')
}

/// Words (alphanumeric runs, with inner apostrophes), single punctuation/symbol chars and
/// whitespace runs.
fn tokenize(text: &str) -> Vec<Tok> {
    let mut toks: Vec<Tok> = Vec::new();
    let mut chars = text.char_indices().peekable();
    let mut after_joiner = false;
    while let Some((start, c)) = chars.next() {
        let end = start + c.len_utf8();
        let joined_by_zwj = after_joiner;
        after_joiner = c == '\u{200D}';
        if let Some(last) = toks.last_mut() {
            let extends = match last.kind {
                TokKind::Space => c.is_whitespace(),
                TokKind::Punct => is_attaching(c) || joined_by_zwj,
                TokKind::Word => {
                    is_attaching(c)
                        || joined_by_zwj
                        || c.is_alphanumeric()
                        || (matches!(c, '\'' | '’')
                            && chars
                                .peek()
                                .is_some_and(|&(_, next)| next.is_alphanumeric()))
                }
            };
            if extends {
                last.end = end;
                continue;
            }
        }
        let kind = if c.is_whitespace() {
            TokKind::Space
        } else if c.is_alphanumeric() {
            TokKind::Word
        } else {
            TokKind::Punct
        };
        toks.push(Tok { start, end, kind });
    }
    toks
}

fn slice<'t>(text: &'t str, toks: &[Tok], range: &Range<usize>) -> &'t str {
    if range.is_empty() {
        ""
    } else {
        &text[toks[range.start].start..toks[range.end - 1].end]
    }
}

/// A run of equal tokens, or a change hunk (deleted `a` tokens, inserted `b` tokens).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Segment {
    equal: bool,
    a: Range<usize>,
    b: Range<usize>,
}

#[derive(Default)]
struct SegmentBuilder {
    segments: Vec<Segment>,
    a: usize,
    b: usize,
}

impl SegmentBuilder {
    fn equal(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        match self.segments.last_mut() {
            Some(seg) if seg.equal => {
                seg.a.end += count;
                seg.b.end += count;
            }
            _ => self.segments.push(Segment {
                equal: true,
                a: self.a..self.a + count,
                b: self.b..self.b + count,
            }),
        }
        self.a += count;
        self.b += count;
    }

    fn change(&mut self, deleted: usize, inserted: usize) {
        if deleted == 0 && inserted == 0 {
            return;
        }
        match self.segments.last_mut() {
            Some(seg) if !seg.equal => {
                seg.a.end += deleted;
                seg.b.end += inserted;
            }
            _ => self.segments.push(Segment {
                equal: false,
                a: self.a..self.a + deleted,
                b: self.b..self.b + inserted,
            }),
        }
        self.a += deleted;
        self.b += inserted;
    }
}

struct TokenDiff {
    a: Vec<Tok>,
    b: Vec<Tok>,
    segments: Vec<Segment>,
}

impl TokenDiff {
    fn compute(original: &str, result: &str) -> Self {
        let a = tokenize(original);
        let b = tokenize(result);
        let mut ids: HashMap<&str, u32> = HashMap::new();
        let a_ids = intern(&mut ids, original, &a);
        let b_ids = intern(&mut ids, result, &b);
        let mut builder = SegmentBuilder::default();
        diff_ids(&a_ids, &b_ids, &mut builder, Some(MAX_EDIT_DISTANCE));
        TokenDiff {
            a,
            b,
            segments: builder.segments,
        }
    }
}

/// Maps each token text to a small integer id so the diff compares integers.
fn intern<'t>(ids: &mut HashMap<&'t str, u32>, text: &'t str, toks: &[Tok]) -> Vec<u32> {
    toks.iter()
        .map(|t| {
            let next = ids.len() as u32;
            *ids.entry(&text[t.start..t.end]).or_insert(next)
        })
        .collect()
}

/// Linear-space Myers diff (divide and conquer on the middle snake). With `cap`, a
/// top-level edit distance above `cap` produces one delete + one insert instead.
fn diff_ids(a: &[u32], b: &[u32], out: &mut SegmentBuilder, cap: Option<usize>) {
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    out.equal(prefix);
    let (a, b) = (&a[prefix..], &b[prefix..]);
    let suffix = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (a, b) = (&a[..a.len() - suffix], &b[..b.len() - suffix]);
    if a.is_empty() || b.is_empty() {
        out.change(a.len(), b.len());
    } else {
        match bisect(a, b, cap) {
            Some((x, y)) => {
                diff_ids(&a[..x], &b[..y], out, None);
                diff_ids(&a[x..], &b[y..], out, None);
            }
            None => out.change(a.len(), b.len()),
        }
    }
    out.equal(suffix);
}

/// Finds the middle snake of the shortest edit script between `a` and `b` (which differ in
/// their first and last elements) and returns the split point. `None` when there is no
/// common element, or when the edit distance exceeds `cap`.
fn bisect(a: &[u32], b: &[u32], cap: Option<usize>) -> Option<(usize, usize)> {
    let n = a.len() as isize;
    let m = b.len() as isize;
    let full = (n + m + 1) / 2;
    // A distance D is found by step ceil(D / 2), so cap / 2 + 1 steps cover D <= cap.
    let max_d = match cap {
        Some(cap) => full.min((cap / 2 + 1) as isize),
        None => full,
    };
    let v_offset = max_d;
    let v_len = 2 * max_d + 2;
    let mut v1 = vec![-1isize; v_len as usize];
    let mut v2 = vec![-1isize; v_len as usize];
    v1[(v_offset + 1) as usize] = 0;
    v2[(v_offset + 1) as usize] = 0;
    let delta = n - m;
    // With an odd delta the forward path overlaps the reverse path; otherwise the reverse.
    let front = delta % 2 != 0;
    let (mut k1start, mut k1end, mut k2start, mut k2end) = (0isize, 0isize, 0isize, 0isize);
    for d in 0..max_d {
        let mut k1 = -d + k1start;
        while k1 <= d - k1end {
            let k1o = (v_offset + k1) as usize;
            let mut x1 = if k1 == -d || (k1 != d && v1[k1o - 1] < v1[k1o + 1]) {
                v1[k1o + 1]
            } else {
                v1[k1o - 1] + 1
            };
            let mut y1 = x1 - k1;
            while x1 < n && y1 < m && a[x1 as usize] == b[y1 as usize] {
                x1 += 1;
                y1 += 1;
            }
            v1[k1o] = x1;
            if x1 > n {
                k1end += 2;
            } else if y1 > m {
                k1start += 2;
            } else if front {
                let k2o = v_offset + delta - k1;
                if k2o >= 0 && k2o < v_len && v2[k2o as usize] != -1 {
                    let x2 = n - v2[k2o as usize];
                    if x1 >= x2 {
                        return Some((x1 as usize, y1 as usize));
                    }
                }
            }
            k1 += 2;
        }
        let mut k2 = -d + k2start;
        while k2 <= d - k2end {
            let k2o = (v_offset + k2) as usize;
            let mut x2 = if k2 == -d || (k2 != d && v2[k2o - 1] < v2[k2o + 1]) {
                v2[k2o + 1]
            } else {
                v2[k2o - 1] + 1
            };
            let mut y2 = x2 - k2;
            while x2 < n && y2 < m && a[(n - x2 - 1) as usize] == b[(m - y2 - 1) as usize] {
                x2 += 1;
                y2 += 1;
            }
            v2[k2o] = x2;
            if x2 > n {
                k2end += 2;
            } else if y2 > m {
                k2start += 2;
            } else if !front {
                let k1o = v_offset + delta - k2;
                if k1o >= 0 && k1o < v_len && v1[k1o as usize] != -1 {
                    let x1 = v1[k1o as usize];
                    let y1 = v_offset + x1 - k1o;
                    if x1 >= n - x2 {
                        return Some((x1 as usize, y1 as usize));
                    }
                }
            }
            k2 += 2;
        }
    }
    None
}

/// Word counts and hunk count derived from the token diff (rules 7 and 8).
struct DiffStats {
    changed_words: usize,
    original_words: usize,
    fixes: usize,
}

impl DiffStats {
    fn compute(original: &str, result: &str) -> Self {
        let diff = TokenDiff::compute(original, result);
        let words = |toks: &[Tok], range: &Range<usize>| {
            toks[range.clone()]
                .iter()
                .filter(|t| t.kind == TokKind::Word)
                .count()
        };
        let only_space = |toks: &[Tok], range: &Range<usize>| {
            toks[range.clone()].iter().all(|t| t.kind == TokKind::Space)
        };
        let mut deleted = 0;
        let mut inserted = 0;
        let mut fixes = 0;
        for seg in diff.segments.iter().filter(|seg| !seg.equal) {
            deleted += words(&diff.a, &seg.a);
            inserted += words(&diff.b, &seg.b);
            if !(only_space(&diff.a, &seg.a) && only_space(&diff.b, &seg.b)) {
                fixes += 1;
            }
        }
        DiffStats {
            changed_words: deleted.max(inserted),
            original_words: words(&diff.a, &(0..diff.a.len())),
            fixes,
        }
    }

    fn into_report(self, issues: Vec<GuardIssue>) -> GuardReport {
        GuardReport {
            issues,
            changed_words: self.changed_words,
            original_words: self.original_words,
            fixes: self.fixes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn report(original: &str, result: &str) -> GuardReport {
        check(original, result, true)
    }

    fn issues(original: &str, result: &str) -> Vec<GuardIssue> {
        report(original, result).issues
    }

    fn eq(text: &str) -> DiffOp {
        DiffOp::Equal(text.to_string())
    }
    fn ins(text: &str) -> DiffOp {
        DiffOp::Insert(text.to_string())
    }
    fn del(text: &str) -> DiffOp {
        DiffOp::Delete(text.to_string())
    }

    /// Rebuilds (original, result) from diff ops.
    fn reconstruct(ops: &[DiffOp]) -> (String, String) {
        let mut a = String::new();
        let mut b = String::new();
        for op in ops {
            match op {
                DiffOp::Equal(t) => {
                    a.push_str(t);
                    b.push_str(t);
                }
                DiffOp::Delete(t) => a.push_str(t),
                DiffOp::Insert(t) => b.push_str(t),
            }
        }
        (a, b)
    }

    // ---------------------------------------------------------------- Rule 1: whitespace

    #[test]
    fn prepare_splits_outer_whitespace() {
        let p = prepare("  \tHallo Welt.\r\n\r\n");
        assert_eq!(p.leading, "  \t");
        assert_eq!(p.core, "Hallo Welt.");
        assert_eq!(p.trailing, "\r\n\r\n");
        assert_eq!(p.line_ending, LineEnding::CrLf);
    }

    #[test]
    fn prepare_without_outer_whitespace_has_empty_parts() {
        let p = prepare("abc");
        assert_eq!(p.leading, "");
        assert_eq!(p.core, "abc");
        assert_eq!(p.trailing, "");
        assert_eq!(p.line_ending, LineEnding::Lf);
    }

    #[test]
    fn prepare_whitespace_only_selection() {
        let p = prepare(" \r\n\t");
        assert_eq!(p.core, "");
        assert_eq!(p.leading, " \r\n\t");
        assert_eq!(p.trailing, "");
        assert_eq!(p.line_ending, LineEnding::CrLf);
    }

    #[test]
    fn prepare_keeps_interior_whitespace() {
        assert_eq!(prepare(" a  b\t\tc\n\nd ").core, "a  b\t\tc\n\nd");
    }

    #[test]
    fn prepare_treats_unicode_whitespace_as_outer() {
        let p = prepare("\u{a0}\u{3000}Text\u{2003}");
        assert_eq!(p.leading, "\u{a0}\u{3000}");
        assert_eq!(p.core, "Text");
        assert_eq!(p.trailing, "\u{2003}");
    }

    #[test]
    fn dominant_line_ending() {
        assert_eq!(prepare("a\nb").line_ending, LineEnding::Lf);
        assert_eq!(prepare("a\r\nb").line_ending, LineEnding::CrLf);
        assert_eq!(prepare("a\r\nb\r\nc\nd").line_ending, LineEnding::CrLf);
        // A tie or an LF majority is LF.
        assert_eq!(prepare("a\r\nb\nc").line_ending, LineEnding::Lf);
        assert_eq!(prepare("a\r\nb\nc\nd").line_ending, LineEnding::Lf);
        // A lone CR is not a line ending.
        assert_eq!(prepare("a\rb").line_ending, LineEnding::Lf);
        assert_eq!(prepare("abc").line_ending, LineEnding::Lf);
    }

    #[test]
    fn restore_reattaches_original_outer_whitespace() {
        let p = prepare("\n  Ich glaube dass er kommt.  \n");
        assert_eq!(
            restore(&p, "  Ich glaube, dass er kommt.\n\n"),
            "\n  Ich glaube, dass er kommt.  \n"
        );
    }

    #[test]
    fn restore_normalizes_line_endings_to_crlf() {
        let p = prepare("a\r\nb\r\n");
        assert_eq!(restore(&p, "a\nb\r\nc"), "a\r\nb\r\nc\r\n");
    }

    #[test]
    fn restore_normalizes_line_endings_to_lf() {
        let p = prepare("a\nb");
        assert_eq!(restore(&p, "a\r\nb\nc"), "a\nb\nc");
    }

    #[test]
    fn restore_mixed_original_uses_dominant_style() {
        let p = prepare("a\r\nb\r\nc\nd");
        assert_eq!(restore(&p, "a\r\nb\r\nc\nd"), "a\r\nb\r\nc\r\nd");
        let p = prepare("a\r\nb\nc\nd");
        assert_eq!(restore(&p, "a\r\nb\nc\nd"), "a\nb\nc\nd");
    }

    #[test]
    fn restore_keeps_lone_carriage_return() {
        let p = prepare("a\r\nb");
        assert_eq!(restore(&p, "a\rb\nc"), "a\rb\r\nc");
    }

    #[test]
    fn restore_keeps_interior_whitespace() {
        let p = prepare("x");
        assert_eq!(
            restore(&p, "a  b\t c\u{a0}d\n\n e"),
            "a  b\t c\u{a0}d\n\n e"
        );
    }

    #[test]
    fn restore_preserves_unicode_exactly() {
        let decomposed =
            "Cafe\u{301} Gru\u{308}\u{df}e \u{1F44D}\u{1F3FD} \u{1F468}\u{200D}\u{1F469}";
        let selection = format!(" {decomposed}\n");
        let p = prepare(&selection);
        assert_eq!(p.core, decomposed);
        let restored = restore(&p, decomposed);
        assert_eq!(restored.as_bytes(), selection.as_bytes());

        let precomposed = "Grüße aus der Straße, ÄÖÜ äöü ß";
        let p = prepare(precomposed);
        assert_eq!(restore(&p, precomposed).as_bytes(), precomposed.as_bytes());
    }

    #[test]
    fn restore_of_unchanged_core_is_byte_identical() {
        for s in ["  Hallo\r\nWelt\r\n", "\tx\ny\n", "ß", "", "   ", "👍"] {
            let p = prepare(s);
            assert_eq!(restore(&p, &p.core), s, "{s:?}");
        }
    }

    #[test]
    fn restore_whitespace_only_selection() {
        let p = prepare("  ");
        assert_eq!(restore(&p, " \n "), "  ");
    }

    // ---------------------------------------------------------------- Rule 2: complete

    #[test]
    fn incomplete_output_fails() {
        let r = check("Hallo Welt.", "Hallo Welt.", false);
        assert_eq!(r.issues, vec![GuardIssue::Incomplete]);
        assert_eq!(r.reason_code(), Some("guard_incomplete"));
        assert!(!r.passed());
    }

    #[test]
    fn completed_unchanged_output_passes() {
        let r = report("Hallo Welt.", "Hallo Welt.");
        assert!(r.passed());
        assert_eq!(r.reason_code(), None);
        assert_eq!(r.changed_words, 0);
        assert_eq!(r.original_words, 2);
        assert_eq!(r.fixes, 0);
    }

    #[test]
    fn empty_output_fails() {
        assert_eq!(issues("Hallo", ""), vec![GuardIssue::Empty]);
        assert_eq!(issues("Hallo", " \n\t "), vec![GuardIssue::Empty]);
        assert_eq!(report("Hallo", "").reason_code(), Some("guard_empty"));
    }

    #[test]
    fn incomplete_is_listed_before_empty() {
        let r = check("Hallo", "", false);
        assert_eq!(r.issues, vec![GuardIssue::Incomplete, GuardIssue::Empty]);
        assert_eq!(r.reason_code(), Some("guard_incomplete"));
    }

    // ---------------------------------------------------------------- Rule 3: preamble

    const COMMA_ORIGINAL: &str = "Ich glaube dass er kommt.";
    const COMMA_FIXED: &str = "Ich glaube, dass er kommt.";

    #[test]
    fn preamble_variants_fail_in_both_languages() {
        let prefixes = [
            "Here is the corrected text: ",
            "Here's the text: ",
            "Here’s the text: ",
            "Corrected text: ",
            "Corrected version: ",
            "The corrected text: ",
            "Sure, ",
            "Sure! ",
            "Certainly. ",
            "Of course: ",
            "Hier ist der korrigierte Text: ",
            "Hier sind die Korrekturen: ",
            "Hier die korrigierte Fassung: ",
            "Korrigierte Fassung: ",
            "Korrigierter Text: ",
            "Gerne: ",
            "Natürlich, ",
            "Selbstverständlich: ",
            "HERE IS the text: ",
        ];
        for prefix in prefixes {
            let result = format!("{prefix}{COMMA_FIXED}");
            let r = report(COMMA_ORIGINAL, &result);
            assert!(
                r.issues.contains(&GuardIssue::Preamble),
                "{result:?}: {r:?}"
            );
            assert_eq!(r.reason_code(), Some("guard_preamble"), "{result:?}");
        }
    }

    #[test]
    fn preamble_on_its_own_line_fails() {
        let r = report(
            COMMA_ORIGINAL,
            &format!("Here is the corrected text:\n\n{COMMA_FIXED}"),
        );
        assert!(r.issues.contains(&GuardIssue::Preamble));
        assert!(r.issues.contains(&GuardIssue::LineCount));
        assert_eq!(r.reason_code(), Some("guard_preamble"));
    }

    #[test]
    fn preamble_phrase_already_in_original_passes() {
        assert!(report(
            "Here is the plan for tomorow.",
            "Here is the plan for tomorrow."
        )
        .passed());
        assert!(report(
            "Gerne komme ich morgen vorbei",
            "Gerne komme ich morgen vorbei."
        )
        .passed());
        assert!(report("Natürlich das geht", "Natürlich, das geht.").passed());
        assert!(report("Sure that works for me.", "Sure, that works for me.").passed());
        assert!(report("Certainly not tomorow.", "Certainly not tomorrow.").passed());
    }

    #[test]
    fn preamble_only_checks_the_first_line() {
        assert!(report(
            "The file is attached.\nhere is teh link.",
            "The file is attached.\nHere is the link."
        )
        .passed());
    }

    // ---------------------------------------------------------------- Rule 4: wrapper

    #[test]
    fn quoted_output_fails() {
        let pairs = [
            ("\"", "\""),
            ("'", "'"),
            ("“", "”"),
            ("„", "“"),
            ("«", "»"),
            ("‚", "‘"),
        ];
        for (open, close) in pairs {
            let result = format!("{open}{COMMA_FIXED}{close}");
            let r = report(COMMA_ORIGINAL, &result);
            assert!(r.issues.contains(&GuardIssue::Wrapper), "{result:?}: {r:?}");
            assert_eq!(r.reason_code(), Some("guard_wrapper"), "{result:?}");
        }
    }

    #[test]
    fn code_fenced_output_fails() {
        for result in [
            format!("```\n{COMMA_FIXED}\n```"),
            format!("```text\n{COMMA_FIXED}\n```"),
        ] {
            let r = report(COMMA_ORIGINAL, &result);
            assert!(r.issues.contains(&GuardIssue::Wrapper), "{result:?}");
            assert_eq!(r.reason_code(), Some("guard_wrapper"));
        }
    }

    #[test]
    fn quotes_already_in_original_pass() {
        assert!(report(
            "\"Ich glaube dass er kommt.\"",
            "\"Ich glaube, dass er kommt.\""
        )
        .passed());
        assert!(report(
            "„Ich glaube dass er kommt.“",
            "„Ich glaube, dass er kommt.“"
        )
        .passed());
    }

    #[test]
    fn inner_quotes_are_not_a_wrapper() {
        assert!(report(
            "Er sagte \"hallo\" und ging",
            "Er sagte \"hallo\" und ging."
        )
        .passed());
        assert!(report(
            "\"Hallo\" sagte er und ging",
            "\"Hallo\", sagte er und ging."
        )
        .passed());
    }

    #[test]
    fn fence_already_in_original_passes() {
        let text = "```\nlet x = 1;\n```";
        assert!(report(text, text).passed());
    }

    // ---------------------------------------------------------------- Rule 5: protected tokens

    fn tokens(text: &str) -> Vec<String> {
        protected_tokens(text)
    }

    #[test]
    fn prose_percent_is_not_a_comment() {
        assert!(tokens("Etwa 50 % der Fälle, also 30 % weniger.").is_empty());
    }

    #[test]
    fn unmatched_dollar_is_plain_text() {
        assert!(tokens("Das kostet $5.").is_empty());
        assert!(tokens("It costs $5 and $10 today.").is_empty());
    }

    #[test]
    fn escaped_dollar_is_not_a_delimiter() {
        assert_eq!(tokens(r"Preis \$5 und $x$ hier"), vec!["$x$"]);
        assert!(tokens(r"Preis \$5 und \$6").is_empty());
    }

    #[test]
    fn math_segments_are_single_tokens() {
        assert_eq!(
            tokens(r"Sei $a_{1}$ und $$\sum_i x_i$$ sowie \(y\) und \[z^2\]."),
            vec![r"$a_{1}$", r"$$\sum_i x_i$$", r"\(y\)", r"\[z^2\]"]
        );
    }

    #[test]
    fn math_does_not_span_a_blank_line() {
        assert_eq!(tokens("Preis $5\n\nund 3$ mehr \\emph{x}"), vec![r"\emph"]);
    }

    #[test]
    fn latex_commands_and_key_arguments() {
        let text = r"\section*{Intro} Siehe \ref{sec:a}, \eqref{eq:1}, \cite[p.~5]{knuth} und \citep{a,b}. \label{x} \input{ch1} \include{ch2} \begin{itemize} \item A \end{itemize} \make@title";
        assert_eq!(
            tokens(text),
            vec![
                r"\section*",
                r"\ref",
                "{sec:a}",
                r"\eqref",
                "{eq:1}",
                r"\cite",
                "{knuth}",
                r"\citep",
                "{a,b}",
                r"\label",
                "{x}",
                r"\input",
                "{ch1}",
                r"\include",
                "{ch2}",
                r"\begin",
                "{itemize}",
                r"\item",
                r"\end",
                "{itemize}",
                r"\make@title",
            ]
        );
    }

    #[test]
    fn key_argument_with_nested_braces() {
        assert_eq!(
            tokens(r"\label{sec:{nested}intro} text"),
            vec![r"\label", "{sec:{nested}intro}"]
        );
    }

    #[test]
    fn natbib_citation_with_two_optional_arguments() {
        assert_eq!(
            tokens(r"\citep[see][p.~5]{key} text"),
            vec![r"\citep", "{key}"]
        );
    }

    #[test]
    fn percent_comment_in_latex_mode() {
        assert_eq!(
            tokens("Text \\emph{a} 50\\% mehr % Kommentar hier\nweiter"),
            vec![r"\emph", "% Kommentar hier"]
        );
        assert_eq!(tokens("\\emph{a} % c\r\nx"), vec![r"\emph", "% c"]);
    }

    #[test]
    fn code_urls_and_emails() {
        let text = "Nutze `cargo test` oder ```rust\nfn main() {}\n``` und schau auf https://example.com/a?b=1, www.vellora.app. Mail: max.mustermann@example.de!";
        assert_eq!(
            tokens(text),
            vec![
                "`cargo test`",
                "```rust\nfn main() {}\n```",
                "https://example.com/a?b=1",
                "www.vellora.app",
                "max.mustermann@example.de",
            ]
        );
    }

    #[test]
    fn url_trailing_punctuation_is_stripped() {
        assert_eq!(tokens("(siehe http://a.de/x)."), vec!["http://a.de/x"]);
    }

    #[test]
    fn near_misses_are_not_urls_or_emails() {
        assert!(
            tokens("Die www-Seite, das http-Protokoll, @handle, a@b, x@y.z1, www. leer").is_empty()
        );
    }

    #[test]
    fn latex_quotes_are_not_code() {
        assert_eq!(
            tokens("Er sagte ``hallo'' und `ja' im \\emph{Text}."),
            vec![r"\emph"]
        );
    }

    #[test]
    fn unmatched_backtick_is_plain() {
        assert!(tokens("Ein ` Zeichen").is_empty());
    }

    #[test]
    fn code_span_hides_backslashes_from_latex_detection() {
        assert_eq!(
            tokens("Pfad `C:\\Users` mit 50 % Rabatt"),
            vec!["`C:\\Users`"]
        );
    }

    #[test]
    fn changed_protected_token_fails() {
        let original = "Siehe `cargo test` und https://example.com.";
        let r = report(original, "Siehe `cargo tests` und https://example.com.");
        assert_eq!(r.issues, vec![GuardIssue::ProtectedTokens]);
        assert_eq!(r.reason_code(), Some("guard_markup"));
    }

    #[test]
    fn reordered_protected_tokens_fail() {
        assert_eq!(
            issues("Erst $x$ und dann $y$.", "Erst $y$ und dann $x$."),
            vec![GuardIssue::ProtectedTokens]
        );
    }

    #[test]
    fn missing_protected_token_fails() {
        assert!(issues(
            "Siehe https://a.de und https://b.de bitte.",
            "Siehe https://a.de und bitte."
        )
        .contains(&GuardIssue::ProtectedTokens));
    }

    // ---------------------------------------------------------------- Rule 6: line count

    #[test]
    fn added_line_fails() {
        let r = report(
            "Erste Zeile\nZweite Zeile",
            "Erste Zeile\nZweite Zeile\nDritte Zeile",
        );
        assert!(r.issues.contains(&GuardIssue::LineCount));
        assert_eq!(r.reason_code(), Some("guard_line_count"));
    }

    #[test]
    fn removed_line_fails() {
        assert_eq!(
            issues("Erste Zeile\n\nZweite Zeile", "Erste Zeile\nZweite Zeile"),
            vec![GuardIssue::LineCount]
        );
    }

    #[test]
    fn crlf_and_lf_line_counts_are_equal() {
        assert!(report("Erste Zeile\r\nZweite Zeile", "Erste Zeile\nZweite Zeile").passed());
    }

    // ---------------------------------------------------------------- Rule 7: size

    #[test]
    fn rewritten_sentence_fails_size() {
        let r = report(
            "The quick brown fox jumps over the lazy dog.",
            "A fast auburn fox leaps above a sleepy hound.",
        );
        assert!(r.issues.contains(&GuardIssue::TooManyChanges), "{r:?}");
        assert_eq!(r.reason_code(), Some("guard_too_many_changes"));
        assert_eq!(r.original_words, 9);
        assert_eq!(r.changed_words, 8);
    }

    #[test]
    fn short_text_allows_two_changed_words() {
        assert!(report("Das ist gut", "Das war schlecht").passed());
        assert_eq!(
            issues("Das ist gut", "Dies war schlecht"),
            vec![GuardIssue::TooManyChanges]
        );
    }

    #[test]
    fn forty_percent_word_limit() {
        let original = "eins zwei drei vier fünf sechs sieben acht neun zehn";
        // 4 of 10 changed: allowed.
        assert!(report(
            original,
            "EINS ZWEI DREI VIER fünf sechs sieben acht neun zehn"
        )
        .passed());
        // 5 of 10 changed: too many.
        assert_eq!(
            issues(
                original,
                "EINS ZWEI DREI VIER FÜNF sechs sieben acht neun zehn"
            ),
            vec![GuardIssue::TooManyChanges]
        );
    }

    #[test]
    fn length_change_limit_is_thirty_percent() {
        let original = "x".repeat(100);
        assert!(report(&original, &"x".repeat(130)).passed());
        assert!(report(&original, &"x".repeat(70)).passed());
        let r = report(&original, &"x".repeat(131));
        assert_eq!(r.issues, vec![GuardIssue::LengthChange]);
        assert_eq!(r.reason_code(), Some("guard_length"));
        assert_eq!(
            issues(&original, &"x".repeat(69)),
            vec![GuardIssue::LengthChange]
        );
    }

    #[test]
    fn length_change_limit_is_at_least_ten_chars() {
        assert!(report("ab", "abcdefghijkl").passed());
        assert_eq!(
            issues("ab", "abcdefghijklm"),
            vec![GuardIssue::LengthChange]
        );
    }

    #[test]
    fn length_is_measured_in_chars_not_bytes() {
        // 10 chars, but 20 bytes more than the original.
        assert!(report("a", "aääääääääää").passed());
    }

    // ---------------------------------------------------------------- Rule 8: fixes

    #[test]
    fn fixes_count_change_hunks() {
        assert_eq!(report("a b c", "x b y").fixes, 2);
        assert_eq!(report(COMMA_ORIGINAL, COMMA_FIXED).fixes, 1);
    }

    #[test]
    fn whitespace_only_hunks_are_not_fixes() {
        let r = report("Hallo  Welt, wie geht es", "Hallo Welt, wie geht es");
        assert!(r.passed());
        assert_eq!(r.fixes, 0);
        assert_eq!(r.changed_words, 0);
    }

    #[test]
    fn apostrophe_inside_a_word_is_one_word() {
        let r = report("I dont know it", "I don't know it");
        assert_eq!(r.changed_words, 1);
        assert_eq!(r.original_words, 4);
    }

    // ---------------------------------------------------------------- Fixtures

    #[test]
    fn fixture_german_comma_fix_passes() {
        let r = report(
            "Ich glaube dass er morgen kommt, weil er es versprochen hat.",
            "Ich glaube, dass er morgen kommt, weil er es versprochen hat.",
        );
        assert!(r.passed(), "{r:?}");
        assert_eq!(r.fixes, 1);
        assert_eq!(r.changed_words, 0);
        assert_eq!(r.original_words, 11);
    }

    #[test]
    fn fixture_english_typo_fix_passes() {
        let r = report(
            "I recieve the report tomorow and will send it to you.",
            "I receive the report tomorrow and will send it to you.",
        );
        assert!(r.passed(), "{r:?}");
        assert_eq!(r.fixes, 2);
        assert_eq!(r.changed_words, 2);
    }

    #[test]
    fn fixture_single_word_typo_passes() {
        let r = report("teh", "the");
        assert!(r.passed(), "{r:?}");
        assert_eq!(r.fixes, 1);
        assert_eq!(r.changed_words, 1);
        assert_eq!(r.original_words, 1);
    }

    const LATEX: &str = r"Die Funktion $x^2$ wächst schnell siehe \cite[p.~5]{knuth} und \ref{sec:{nested}intro}.
Etwa 50\% der Fälle sind betroffen. % TODO: Quelle prüfen
\begin{equation}
  E = mc^2 \label{eq:energy}
\end{equation}";

    #[test]
    fn fixture_latex_paragraph_untouched_passes() {
        assert!(report(LATEX, LATEX).passed());
        let polished = LATEX.replace("schnell siehe", "schnell, siehe");
        let r = report(LATEX, &polished);
        assert!(r.passed(), "{r:?}");
        assert_eq!(r.fixes, 1);
    }

    #[test]
    fn fixture_latex_tokens() {
        assert_eq!(
            tokens(LATEX),
            vec![
                "$x^2$",
                r"\cite",
                "{knuth}",
                r"\ref",
                "{sec:{nested}intro}",
                "% TODO: Quelle prüfen",
                r"\begin",
                "{equation}",
                r"\label",
                "{eq:energy}",
                r"\end",
                "{equation}",
            ]
        );
    }

    #[test]
    fn fixture_latex_fix_inside_math_fails() {
        let r = report(LATEX, &LATEX.replace("$x^2$", "$x^{2}$"));
        assert_eq!(r.issues, vec![GuardIssue::ProtectedTokens]);
    }

    #[test]
    fn fixture_latex_fix_of_cite_key_fails() {
        let r = report(LATEX, &LATEX.replace("{knuth}", "{Knuth}"));
        assert_eq!(r.issues, vec![GuardIssue::ProtectedTokens]);
    }

    #[test]
    fn fixture_latex_fix_inside_comment_fails() {
        let r = report(LATEX, &LATEX.replace("Quelle prüfen", "Quelle überprüfen"));
        assert_eq!(r.issues, vec![GuardIssue::ProtectedTokens]);
    }

    #[test]
    fn fixture_latex_escaped_percent_change_fails() {
        // Turning "50\%" into "50 %" starts a comment and swallows the rest of the line.
        let r = report(LATEX, &LATEX.replace(r"50\%", "50 %"));
        assert!(r.issues.contains(&GuardIssue::ProtectedTokens));
    }

    const MARKDOWN: &str = "Installiere es mit `cargo install vellora` und lies die Doku unter https://vellora.app/docs.\nBei Fragen schreib an support@vellora.app oder teh team.";

    #[test]
    fn fixture_markdown_with_code_and_url_passes() {
        let r = report(MARKDOWN, &MARKDOWN.replace("teh team", "the team"));
        assert!(r.passed(), "{r:?}");
        assert_eq!(r.fixes, 1);
    }

    #[test]
    fn fixture_markdown_changed_url_or_code_fails() {
        for changed in [
            MARKDOWN.replace("/docs", "/Docs"),
            MARKDOWN.replace("cargo install", "Cargo install"),
            MARKDOWN.replace("support@", "Support@"),
        ] {
            assert_eq!(
                issues(MARKDOWN, &changed),
                vec![GuardIssue::ProtectedTokens],
                "{changed:?}"
            );
        }
    }

    // ---------------------------------------------------------------- Reason codes

    #[test]
    fn reason_codes_for_every_issue() {
        let cases = [
            (GuardIssue::Incomplete, "guard_incomplete"),
            (GuardIssue::Empty, "guard_empty"),
            (GuardIssue::Preamble, "guard_preamble"),
            (GuardIssue::Wrapper, "guard_wrapper"),
            (GuardIssue::ProtectedTokens, "guard_markup"),
            (GuardIssue::LineCount, "guard_line_count"),
            (GuardIssue::TooManyChanges, "guard_too_many_changes"),
            (GuardIssue::LengthChange, "guard_length"),
        ];
        for (issue, code) in cases {
            let r = GuardReport {
                issues: vec![issue, GuardIssue::Empty],
                changed_words: 0,
                original_words: 0,
                fixes: 0,
            };
            assert_eq!(r.reason_code(), Some(code));
            assert!(!r.passed());
        }
    }

    #[test]
    fn guard_issue_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&GuardIssue::TooManyChanges).unwrap(),
            "\"too_many_changes\""
        );
    }

    // ---------------------------------------------------------------- word_diff

    #[test]
    fn identical_texts_are_one_equal_op() {
        assert_eq!(
            word_diff("Hallo Welt", "Hallo Welt"),
            vec![eq("Hallo Welt")]
        );
    }

    #[test]
    fn empty_inputs() {
        assert!(word_diff("", "").is_empty());
        assert_eq!(word_diff("", "neu"), vec![ins("neu")]);
        assert_eq!(word_diff("alt", ""), vec![del("alt")]);
    }

    #[test]
    fn replaced_word() {
        assert_eq!(
            word_diff("Ich habe teh Katze", "Ich habe the Katze"),
            vec![eq("Ich habe "), del("teh"), ins("the"), eq(" Katze")]
        );
    }

    #[test]
    fn inserted_punctuation_is_its_own_token() {
        assert_eq!(
            word_diff("Ich glaube dass", "Ich glaube, dass"),
            vec![eq("Ich glaube"), ins(","), eq(" dass")]
        );
    }

    #[test]
    fn separate_hunks_and_delete_before_insert() {
        assert_eq!(
            word_diff("a b c d", "a x y d"),
            vec![
                eq("a "),
                del("b"),
                ins("x"),
                eq(" "),
                del("c"),
                ins("y"),
                eq(" d"),
            ]
        );
    }

    #[test]
    fn adjacent_ops_of_the_same_kind_are_merged() {
        assert_eq!(
            word_diff("eins zwei", "drei"),
            vec![del("eins zwei"), ins("drei")]
        );
        assert_eq!(
            word_diff("Ende.", "Ende, wirklich."),
            vec![eq("Ende"), ins(", wirklich"), eq(".")]
        );
    }

    #[test]
    fn unicode_is_never_split() {
        let pairs = [
            ("Cafe\u{301} gut", "Cafe\u{301} sehr gut"),
            ("Gru\u{308}ße 👍🏽 ok", "Grüße 👍🏾 okay"),
            ("👨\u{200D}👩 Familie", "👨\u{200D}👧 Familie"),
        ];
        for (a, b) in pairs {
            let ops = word_diff(a, b);
            assert_eq!(reconstruct(&ops), (a.to_string(), b.to_string()));
            for op in &ops {
                let (DiffOp::Equal(t) | DiffOp::Insert(t) | DiffOp::Delete(t)) = op;
                let first = t.chars().next().unwrap();
                assert!(
                    !matches!(first, '\u{300}'..='\u{36F}' | '\u{1F3FB}'..='\u{1F3FF}' | '\u{200D}'),
                    "{ops:?}"
                );
            }
        }
        // The family emoji differs only after the joiner: the whole cluster is replaced.
        assert_eq!(
            word_diff("👨\u{200D}👩 Familie", "👨\u{200D}👧 Familie"),
            vec![del("👨\u{200D}👩"), ins("👨\u{200D}👧"), eq(" Familie")]
        );
    }

    #[test]
    fn diff_serializes_with_op_and_text() {
        assert_eq!(
            serde_json::to_string(&word_diff("a b", "a c")).unwrap(),
            r#"[{"op":"equal","text":"a "},{"op":"delete","text":"b"},{"op":"insert","text":"c"}]"#
        );
    }

    /// Small deterministic xorshift generator for the property tests.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    fn random_text(rng: &mut Rng, max_tokens: u64) -> String {
        const PIECES: [&str; 8] = ["a", "b", "c", "dd", " ", ",", ".", "\n"];
        let len = rng.below(max_tokens + 1);
        (0..len).map(|_| PIECES[rng.below(8) as usize]).collect()
    }

    fn lcs_len(a: &[&str], b: &[&str]) -> usize {
        let mut prev = vec![0usize; b.len() + 1];
        for x in a {
            let mut cur = vec![0usize; b.len() + 1];
            for (j, y) in b.iter().enumerate() {
                cur[j + 1] = if x == y {
                    prev[j] + 1
                } else {
                    cur[j].max(prev[j + 1])
                };
            }
            prev = cur;
        }
        prev[b.len()]
    }

    #[test]
    fn diff_reconstructs_and_is_minimal_on_random_inputs() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..2000 {
            let a = random_text(&mut rng, 30);
            let b = random_text(&mut rng, 30);
            let ops = word_diff(&a, &b);
            assert_eq!(reconstruct(&ops), (a.clone(), b.clone()), "{a:?} {b:?}");

            // Minimality: the token edit count equals |a| + |b| - 2 * LCS.
            let ta = tokenize(&a);
            let tb = tokenize(&b);
            let sa: Vec<&str> = ta.iter().map(|t| &a[t.start..t.end]).collect();
            let sb: Vec<&str> = tb.iter().map(|t| &b[t.start..t.end]).collect();
            let edits: usize = ops
                .iter()
                .map(|op| match op {
                    DiffOp::Equal(_) => 0,
                    DiffOp::Delete(t) | DiffOp::Insert(t) => tokenize(t).len(),
                })
                .sum();
            assert_eq!(
                edits,
                sa.len() + sb.len() - 2 * lcs_len(&sa, &sb),
                "{a:?} {b:?}"
            );

            // No two adjacent ops of the same kind; deletes come before inserts.
            for pair in ops.windows(2) {
                assert_ne!(
                    std::mem::discriminant(&pair[0]),
                    std::mem::discriminant(&pair[1]),
                    "{ops:?}"
                );
                assert!(
                    !matches!((&pair[0], &pair[1]), (DiffOp::Insert(_), DiffOp::Delete(_))),
                    "{ops:?}"
                );
            }
        }
    }

    fn numbered_words(prefix: &str, count: usize) -> String {
        (0..count)
            .map(|i| format!("{prefix}{i}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn falls_back_beyond_4000_token_edit_distance() {
        // 2,100 words replaced on each side: 4,200 token edits.
        let a = numbered_words("a", 2100);
        let b = numbered_words("b", 2100);
        assert_eq!(word_diff(&a, &b), vec![del(&a), ins(&b)]);

        // The common prefix and suffix stay equal around the fallback.
        let a2 = format!("Anfang {a} Ende");
        let b2 = format!("Anfang {b} Ende");
        assert_eq!(
            word_diff(&a2, &b2),
            vec![eq("Anfang "), del(&a), ins(&b), eq(" Ende")]
        );
    }

    #[test]
    fn exact_diff_up_to_4000_token_edit_distance() {
        // 2,000 words replaced on each side: exactly 4,000 token edits.
        let a = numbered_words("a", 2000);
        let b = numbered_words("b", 2000);
        let ops = word_diff(&a, &b);
        assert_eq!(ops.len(), 2000 * 3 - 1);
        assert_eq!(reconstruct(&ops), (a, b));
    }

    fn long_text(chars: usize) -> String {
        let sentence = "Der schnelle braune Fuchs springt über den faulen Hund, siehe https://example.com und `code`.\n";
        let mut text = String::new();
        while text.chars().count() < chars {
            text.push_str(sentence);
        }
        text
    }

    #[test]
    fn large_inputs_finish_quickly() {
        let original = long_text(30_000);
        let mut result = String::new();
        for (i, line) in original.lines().enumerate() {
            if i % 10 == 0 {
                result.push_str(&line.replace("Fuchs springt", "Fuchs, springt"));
            } else {
                result.push_str(line);
            }
            result.push('\n');
        }
        let unrelated =
            "Lorem ipsum dolor sit amet, consectetur adipiscing elit sed do.\n".repeat(470);

        let start = Instant::now();
        let r = check(original.trim(), result.trim(), true);
        assert!(r.passed(), "{:?}", r.issues);
        assert!(r.fixes > 20);
        let ops = word_diff(&original, &result);
        assert_eq!(reconstruct(&ops), (original.clone(), result.clone()));

        let r = check(original.trim(), unrelated.trim(), true);
        assert!(r.issues.contains(&GuardIssue::TooManyChanges));
        let ops = word_diff(&original, &unrelated);
        assert_eq!(reconstruct(&ops), (original.clone(), unrelated.clone()));

        // Pathological inputs for the token scanner.
        for text in [
            "$5 ".repeat(10_000),
            "\\cite{".repeat(5_000),
            "`` ".repeat(10_000),
            "` ``".repeat(7_500),
            "\\( ".repeat(10_000),
            "$$ ".repeat(10_000),
            "a@".repeat(15_000),
            "\\emph{x} `".repeat(3_000),
        ] {
            let _ = protected_tokens(&text);
            let _ = check(&text, &text, true);
        }
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "took {:?}",
            start.elapsed()
        );
    }
}
