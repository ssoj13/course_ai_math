//! Tokenizer for CGATS.17 / IT8.7 and `.cube` streams (`InSymbol` &co in
//! `cmscgats.c`).
//!
//! Reproduces the reference lexer faithfully, including its quirks:
//! - a number immediately followed by an identifier character becomes an
//!   identifier (`3D` → `SIDENT "3D"`), the numeric part re-rendered through the
//!   active double formatter;
//! - locale-free float parsing that always treats `.` as the decimal separator
//!   ([`parse_float_number`], `ReadReal`);
//! - hex (`0x…`) / binary (`0b…`) integer literals with an overflow guard that
//!   falls back to a real number;
//! - a one-character look-ahead source with a nested `.include` file stack.

use core::cmp::Ordering;

use crate::error::{Error, ErrorCode, Result};

use super::table::strcasecmp;
use super::{MAXINCLUDE, MAXSTR};

/// Lexical symbols (`SYMBOL` in `cmscgats.c`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Symbol {
    Undefined,
    /// Integer literal (`SINUM`).
    Inum,
    /// Real literal (`SDNUM`).
    Dnum,
    /// Identifier (`SIDENT`).
    Ident,
    /// Quoted string (`SSTRING`).
    Str,
    /// Comment (`SCOMMENT`).
    Comment,
    /// End of line (`SEOLN`).
    Eoln,
    /// End of stream (`SEOF`).
    Eof,
    /// Syntax error (`SSYNERROR`). Never stored in the Rust port — errors
    /// propagate via [`Result`] instead of the sticky `sy = SSYNERROR` state —
    /// but retained so the [`Symbol`] enum mirrors the C `SYMBOL` one-for-one.
    #[allow(dead_code)]
    SynError,

    // IT8 keywords
    BeginData,
    BeginDataFormat,
    EndData,
    EndDataFormat,
    Keyword,
    DataFormatId,
    Include,

    // Cube keywords
    DomainMax,
    DomainMin,
    Lut1dSize,
    Lut1dInputRange,
    Lut3dSize,
    Lut3dInputRange,
    LutInVideoRange,
    LutOutVideoRange,
    Title,
}

/// `TabKeysIT8` — sorted for the case-insensitive binary search.
static TAB_KEYS_IT8: &[(&str, Symbol)] = &[
    ("$INCLUDE", Symbol::Include),
    (".INCLUDE", Symbol::Include),
    ("BEGIN_DATA", Symbol::BeginData),
    ("BEGIN_DATA_FORMAT", Symbol::BeginDataFormat),
    ("DATA_FORMAT_IDENTIFIER", Symbol::DataFormatId),
    ("END_DATA", Symbol::EndData),
    ("END_DATA_FORMAT", Symbol::EndDataFormat),
    ("KEYWORD", Symbol::Keyword),
];

/// `TabKeysCUBE` — sorted for the case-insensitive binary search.
static TAB_KEYS_CUBE: &[(&str, Symbol)] = &[
    ("DOMAIN_MAX", Symbol::DomainMax),
    ("DOMAIN_MIN", Symbol::DomainMin),
    ("LUT_1D_SIZE", Symbol::Lut1dSize),
    ("LUT_1D_INPUT_RANGE", Symbol::Lut1dInputRange),
    ("LUT_3D_SIZE", Symbol::Lut3dSize),
    ("LUT_3D_INPUT_RANGE", Symbol::Lut3dInputRange),
    ("LUT_IN_VIDEO_RANGE", Symbol::LutInVideoRange),
    ("LUT_OUT_VIDEO_RANGE", Symbol::LutOutVideoRange),
    ("TITLE", Symbol::Title),
];

/// `BinSrchKey` — binary search of a sorted keyword table using [`strcasecmp`].
fn bin_srch_key(id: &str, tab: &[(&str, Symbol)]) -> Symbol {
    let mut l: i32 = 1;
    let mut r: i32 = tab.len() as i32;
    while r >= l {
        let x = (l + r) / 2;
        match strcasecmp(id, tab[(x - 1) as usize].0) {
            Ordering::Equal => return tab[(x - 1) as usize].1,
            Ordering::Less => r = x - 1,
            Ordering::Greater => l = x + 1,
        }
    }
    Symbol::Undefined
}

// ---- character classification (`isseparator` … `isfirstidchar`) ----

#[inline]
fn is_separator(c: i32) -> bool {
    c == b' ' as i32 || c == b'\t' as i32
}
#[inline]
fn is_digit(c: i32) -> bool {
    (b'0' as i32..=b'9' as i32).contains(&c)
}
#[inline]
fn is_alpha(c: i32) -> bool {
    (b'A' as i32..=b'Z' as i32).contains(&c) || (b'a' as i32..=b'z' as i32).contains(&c)
}
#[inline]
fn is_alnum(c: i32) -> bool {
    is_digit(c) || is_alpha(c)
}
#[inline]
fn is_xdigit(c: i32) -> bool {
    is_digit(c) || (b'A' as i32..=b'F' as i32).contains(&c) || (b'a' as i32..=b'f' as i32).contains(&c)
}
#[inline]
fn is_middle(c: i32) -> bool {
    !is_separator(c) && c != b'#' as i32 && c != b'"' as i32 && c != b'\'' as i32 && c > 32 && c < 127
}
#[inline]
fn is_idchar(c: i32) -> bool {
    is_alnum(c) || is_middle(c)
}
#[inline]
fn is_first_idchar(c: i32) -> bool {
    c != b'-' as i32 && !is_digit(c) && is_middle(c)
}
#[inline]
fn to_upper(c: i32) -> i32 {
    if (b'a' as i32..=b'z' as i32).contains(&c) {
        c - 32
    } else {
        c
    }
}

/// `10^n` (`xpow10`).
fn xpow10(n: i32) -> f64 {
    10f64.powi(n)
}

/// Locale-free float parser (`ParseFloatNumber`): always `.` as the decimal
/// separator, with an integer-exponent overflow guard identical to the C code.
pub(crate) fn parse_float_number(buffer: Option<&str>) -> f64 {
    let bytes = match buffer {
        None => return 0.0,
        Some(s) => s.as_bytes(),
    };
    let mut i = 0usize;
    let at = |i: usize| -> i32 {
        if i < bytes.len() {
            bytes[i] as i32
        } else {
            0
        }
    };

    let mut dnum = 0.0f64;
    let mut sign = 1.0f64;

    if at(i) == b'-' as i32 || at(i) == b'+' as i32 {
        sign = if at(i) == b'-' as i32 { -1.0 } else { 1.0 };
        i += 1;
    }

    while at(i) != 0 && is_digit(at(i)) {
        dnum = dnum * 10.0 + (at(i) - b'0' as i32) as f64;
        i += 1;
    }

    if at(i) == b'.' as i32 {
        let mut frac = 0.0f64;
        let mut prec = 0i32;
        i += 1;
        while at(i) != 0 && is_digit(at(i)) {
            frac = frac * 10.0 + (at(i) - b'0' as i32) as f64;
            prec += 1;
            i += 1;
        }
        dnum += frac / xpow10(prec);
    }

    if at(i) != 0 && to_upper(at(i)) == b'E' as i32 {
        i += 1;
        let mut sgn = 1i32;
        if at(i) == b'-' as i32 {
            sgn = -1;
            i += 1;
        } else if at(i) == b'+' as i32 {
            sgn = 1;
            i += 1;
        }
        let mut e = 0i32;
        while at(i) != 0 && is_digit(at(i)) {
            let digit = at(i) - b'0' as i32;
            if (e as f64 * 10.0 + digit as f64) < 2_147_483_647.0 {
                e = e * 10 + digit;
            }
            i += 1;
        }
        e *= sgn;
        dnum *= xpow10(e);
    }

    sign * dnum
}

/// `printf("%.<prec>g", v)` equivalent used by the default `DEFAULT_DBL_FORMAT`
/// (`"%.10g"`) and by [`Lexer::get_val`] / value stringification. Reproduces the
/// C `%g` rules: pick `%e`/`%f` by exponent, strip trailing zeros, emit a signed
/// two-plus-digit exponent.
pub(crate) fn fmt_g(v: f64, prec: usize) -> String {
    if v == 0.0 {
        return if v.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    if v.is_nan() {
        return "nan".into();
    }
    if v.is_infinite() {
        return if v < 0.0 { "-inf".into() } else { "inf".into() };
    }

    let p = if prec == 0 { 1 } else { prec };

    // `%e` with `p-1` fractional digits yields `p` significant digits and lets us
    // read the (rounded) decimal exponent.
    let e_str = format!("{:.*e}", p - 1, v);
    let epos = e_str.find('e').expect("scientific format contains 'e'");
    let exp: i32 = e_str[epos + 1..].parse().expect("valid exponent");

    if exp >= -4 && exp < p as i32 {
        let fprec = (p as i32 - 1 - exp).max(0) as usize;
        let mut s = format!("{:.*}", fprec, v);
        strip_trailing(&mut s);
        s
    } else {
        let mut mant = e_str[..epos].to_string();
        strip_trailing(&mut mant);
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{}e{}{:02}", mant, sign, exp.abs())
    }
}

/// Apply a printf floating-point conversion spec to `v`.
///
/// `cmsIT8DefineDblFormat` (`cmscgats.c:3049-3062`) stores the format string VERBATIM and hands
/// it to `snprintf` at four sites (`cmsIT8SetPropertyDbl` `:1550-1557`, `GetVal`'s SDNUM case
/// `:2073`, `cmsIT8SetDataRowColDbl` `:2851-2861`, `InSymbol` `:963-966`), so any printf float
/// spec is legal - not just `%g`. Honouring only `%.<n>g` silently rewrote `%.4f` as `%.10g`,
/// so a file written by lcms with a custom formatter did not round-trip.
///
/// Handles `%[flags][width][.precision][efgEFG]` with an optional literal prefix/suffix.
/// Unsupported by design (no lcms caller uses them): `*` width/precision, length modifiers,
/// and the `#` flag's "keep trailing zeros" effect on `%g`. An unparseable spec falls back to
/// `%.10g`, the library default (`DEFAULT_DBL_FORMAT`).
pub(crate) fn format_printf_double(spec: &str, v: f64) -> String {
    let Some(pct) = spec.find('%') else {
        return fmt_g(v, 10);
    };
    let b = spec.as_bytes();
    let mut i = pct + 1;

    let (mut left, mut zero, mut plus, mut space) = (false, false, false, false);
    while i < b.len() {
        match b[i] {
            b'-' => left = true,
            b'0' => zero = true,
            b'+' => plus = true,
            b' ' => space = true,
            b'#' => {}
            _ => break,
        }
        i += 1;
    }

    let mut width = 0usize;
    while i < b.len() && b[i].is_ascii_digit() {
        width = width * 10 + (b[i] - b'0') as usize;
        i += 1;
    }

    let mut prec: Option<usize> = None;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let mut p = 0usize;
        while i < b.len() && b[i].is_ascii_digit() {
            p = p * 10 + (b[i] - b'0') as usize;
            i += 1;
        }
        prec = Some(p);
    }

    if i >= b.len() {
        return fmt_g(v, 10);
    }
    let conv = b[i] as char;
    let suffix = &spec[i + 1..];
    let prefix = &spec[..pct];

    // C's default precision is 6 for every float conversion.
    let mut body = match conv {
        'f' | 'F' => format!("{:.*}", prec.unwrap_or(6), v.abs()),
        'e' | 'E' => fmt_e(v.abs(), prec.unwrap_or(6)),
        'g' | 'G' => fmt_g(v.abs(), prec.unwrap_or(6)),
        _ => return fmt_g(v, 10),
    };
    if conv.is_uppercase() {
        body = body.to_uppercase();
    }

    let sign = if v.is_sign_negative() {
        "-"
    } else if plus {
        "+"
    } else if space {
        " "
    } else {
        ""
    };

    let mut out = format!("{sign}{body}");
    if out.len() < width {
        let pad = width - out.len();
        if left {
            out.push_str(&" ".repeat(pad));
        } else if zero {
            // Zero padding goes after the sign, not before it.
            out = format!("{sign}{}{body}", "0".repeat(pad));
        } else {
            out = format!("{}{out}", " ".repeat(pad));
        }
    }
    format!("{prefix}{out}{suffix}")
}

/// `printf("%.<prec>e", v)` - mantissa plus a signed, at-least-two-digit exponent.
fn fmt_e(v: f64, prec: usize) -> String {
    if v == 0.0 {
        return format!("{:.*}e+00", prec, 0.0);
    }
    if v.is_nan() {
        return "nan".into();
    }
    if v.is_infinite() {
        return "inf".into();
    }
    let s = format!("{:.*e}", prec, v);
    let epos = s.find('e').expect("scientific format contains 'e'");
    let exp: i32 = s[epos + 1..].parse().expect("valid exponent");
    let sign = if exp < 0 { '-' } else { '+' };
    format!("{}e{}{:02}", &s[..epos], sign, exp.abs())
}

/// Drop trailing fractional zeros (and a dangling `.`), matching `%g` without the
/// `#` flag.
fn strip_trailing(s: &mut String) {
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
}

/// One source frame on the include stack (`FILECTX`). The Rust port keeps every
/// source fully in memory; `pos` is the one-character look-ahead cursor.
struct Frame {
    /// File name for diagnostics; empty for the in-memory root.
    name: String,
    /// Raw bytes of the source.
    buf: Vec<u8>,
    /// Read cursor.
    pos: usize,
}

/// The tokenizer state (`cmsIT8` parser fields) plus the source stack.
pub(crate) struct Lexer {
    /// Current symbol (`sy`).
    pub sy: Symbol,
    /// Current look-ahead character (`ch`); `0` marks end of stream.
    pub ch: i32,
    /// Integer value of the current `Inum` (`inum`).
    pub inum: i32,
    /// Real value of the current `Dnum` (`dnum`).
    pub dnum: f64,
    /// Text of the current identifier (`id`).
    pub id: String,
    /// Text of the current string (`str`).
    pub str: String,
    /// `.cube` mode selects `TabKeysCUBE` (`IsCUBE`).
    pub is_cube: bool,
    /// Line counter for diagnostics (`lineno`).
    pub lineno: i32,
    /// Active double formatter (`DoubleFormatter`), e.g. `"%.10g"`.
    pub double_formatter: String,
    /// Source stack; index 0 is the root, deeper frames are `.include`d files.
    stack: Vec<Frame>,
}

impl Lexer {
    /// Build a lexer over an in-memory source (`cmsIT8LoadFromMem` root frame).
    pub fn from_mem(name: &str, bytes: &[u8]) -> Self {
        Self {
            sy: Symbol::Undefined,
            ch: b' ' as i32,
            inum: 0,
            dnum: 0.0,
            id: String::new(),
            str: String::new(),
            is_cube: false,
            lineno: 1,
            double_formatter: super::DEFAULT_DBL_FORMAT.to_string(),
            stack: vec![Frame {
                name: name.to_string(),
                buf: bytes.to_vec(),
                pos: 0,
            }],
        }
    }

    /// The name of the source currently being read (for error messages).
    fn current_name(&self) -> &str {
        &self.stack.last().expect("stack is never empty").name
    }

    /// Build a `Line N` syntax error (`SynError`).
    pub fn syn_error(&self, msg: impl AsRef<str>) -> Error {
        Error::new(
            ErrorCode::CorruptionDetected,
            format!("{}: Line {}, {}", self.current_name(), self.lineno, msg.as_ref()),
        )
    }

    /// Read the next character (`NextCh`). Pops exhausted `.include` frames,
    /// substituting a space, and yields `0` at true end of stream.
    fn next_ch(&mut self) {
        let sp = self.stack.len() - 1;
        let frame = &mut self.stack[sp];
        if frame.pos < frame.buf.len() {
            let b = frame.buf[frame.pos];
            if b == 0 {
                // Embedded NUL == end of an in-memory source; do not advance.
                self.ch = 0;
            } else {
                self.ch = b as i32;
                frame.pos += 1;
            }
        } else if sp > 0 {
            self.stack.pop();
            self.ch = b' ' as i32; // Whitespace to be ignored.
        } else {
            self.ch = 0;
        }
    }

    /// `Check` — assert the current symbol, else a syntax error.
    pub fn check(&self, sy: Symbol, err: &str) -> Result<()> {
        if self.sy != sy {
            return Err(self.syn_error(err));
        }
        Ok(())
    }

    /// `CheckEOLN` — require at least one end-of-line, then swallow the rest.
    pub fn check_eoln(&mut self) -> Result<()> {
        self.check(Symbol::Eoln, "Expected separator")?;
        while self.sy == Symbol::Eoln {
            self.in_symbol()?;
        }
        Ok(())
    }

    /// `Skip` — consume `sy` if present (and not EOF/error).
    pub fn skip(&mut self, sy: Symbol) -> Result<()> {
        if self.sy == sy && self.sy != Symbol::Eof {
            self.in_symbol()?;
        }
        Ok(())
    }

    /// `SkipEOLN` — consume a run of end-of-line symbols.
    pub fn skip_eoln(&mut self) -> Result<()> {
        while self.sy == Symbol::Eoln {
            self.in_symbol()?;
        }
        Ok(())
    }

    /// `ReadReal` — continue reading a real number from an already-scanned integer.
    fn read_real(&mut self, inum: i32) {
        self.dnum = inum as f64;

        while is_digit(self.ch) {
            self.dnum = self.dnum * 10.0 + (self.ch - b'0' as i32) as f64;
            self.next_ch();
        }

        if self.ch == b'.' as i32 {
            let mut frac = 0.0f64;
            let mut prec = 0i32;
            self.next_ch();
            while is_digit(self.ch) {
                frac = frac * 10.0 + (self.ch - b'0' as i32) as f64;
                prec += 1;
                self.next_ch();
            }
            self.dnum += frac / xpow10(prec);
        }

        if to_upper(self.ch) == b'E' as i32 {
            self.next_ch();
            let mut sgn = 1i32;
            if self.ch == b'-' as i32 {
                sgn = -1;
                self.next_ch();
            } else if self.ch == b'+' as i32 {
                sgn = 1;
                self.next_ch();
            }
            let mut e = 0i32;
            while is_digit(self.ch) {
                let digit = self.ch - b'0' as i32;
                if (e as f64 * 10.0 + digit as f64) < 2_147_483_647.0 {
                    e = e * 10 + digit;
                }
                self.next_ch();
            }
            e *= sgn;
            self.dnum *= xpow10(e);
        }
    }

    /// `InStringSymbol` — read a single- or double-quoted string (no escapes).
    fn in_string_symbol(&mut self) -> Result<()> {
        while is_separator(self.ch) {
            self.next_ch();
        }

        if self.ch == b'\'' as i32 || self.ch == b'"' as i32 {
            let sng = self.ch;
            self.str.clear();
            self.next_ch();
            while self.ch != sng {
                if self.ch == b'\n' as i32 || self.ch == b'\r' as i32 || self.ch == 0 {
                    break;
                }
                self.str.push(self.ch as u8 as char);
                self.next_ch();
            }
            self.sy = Symbol::Str;
            self.next_ch();
            Ok(())
        } else {
            Err(self.syn_error("String expected"))
        }
    }

    /// `ReadType` — the special first line: the sheet type text up to a tab or
    /// end of line.
    pub fn read_type(&mut self) -> String {
        let mut out = String::new();
        let mut cnt = 0i32;
        while is_separator(self.ch) {
            self.next_ch();
        }
        while self.ch != b'\r' as i32
            && self.ch != b'\n' as i32
            && self.ch != b'\t' as i32
            && self.ch != 0
        {
            if cnt < MAXSTR as i32 {
                out.push(self.ch as u8 as char);
                cnt += 1;
            }
            self.next_ch();
        }
        out
    }

    /// Skip separators on the look-ahead cursor and report whether the next
    /// character is an end-of-line. Used by `ParseIT8`'s special "is the next
    /// token a sheet-type line?" disambiguation after a data section.
    pub fn skip_separators_at_newline(&mut self) -> bool {
        while is_separator(self.ch) {
            self.next_ch();
        }
        self.ch == b'\n' as i32 || self.ch == b'\r' as i32
    }

    /// `GetVal` — the current symbol rendered as a string (for header/data
    /// values). `max` truncates like the C buffer size.
    pub fn get_val(&self, max: usize, error_title: &str) -> Result<String> {
        let s = match self.sy {
            Symbol::Eoln => String::new(),
            Symbol::Ident => super::table::truncate_str(&self.id, max),
            // C truncates EVERY case at `max` (`cmscgats.c:2065-2085`), numbers included.
            Symbol::Inum => super::table::truncate_str(&format!("{}", self.inum), max),
            Symbol::Dnum => super::table::truncate_str(&self.format_dbl(self.dnum), max),
            Symbol::Str => super::table::truncate_str(&self.str, max),
            _ => return Err(self.syn_error(error_title)),
        };
        Ok(s)
    }

    /// Format a `f64` through the active double formatter (`cmsIT8DefineDblFormat`).
    pub fn format_dbl(&self, v: f64) -> String {
        format_printf_double(&self.double_formatter, v)
    }

    /// Push a `.include`d file onto the source stack (`InSymbol`'s include
    /// handling). Resolves the path relative to the current file.
    fn push_include(&mut self, rel: &str) -> Result<()> {
        if self.stack.len() >= MAXINCLUDE {
            return Err(self.syn_error("Too many recursion levels"));
        }
        let base = self.current_name().to_string();
        let abs = build_absolute_path(rel, &base)
            .ok_or_else(|| self.syn_error("File path too long"))?;
        let bytes = std::fs::read(&abs)
            .map_err(|_| self.syn_error(format!("File {abs} not found")))?;
        self.stack.push(Frame {
            name: abs,
            buf: bytes,
            pos: 0,
        });
        self.ch = b' ' as i32;
        Ok(())
    }

    /// `InSymbol` — scan the next symbol.
    pub fn in_symbol(&mut self) -> Result<()> {
        loop {
            // Skip separators.
            while is_separator(self.ch) {
                self.next_ch();
            }

            if is_first_idchar(self.ch) {
                // Identifier (possibly a keyword).
                self.id.clear();
                loop {
                    self.id.push(self.ch as u8 as char);
                    self.next_ch();
                    if !is_idchar(self.ch) {
                        break;
                    }
                }
                let key = bin_srch_key(
                    &self.id,
                    if self.is_cube { TAB_KEYS_CUBE } else { TAB_KEYS_IT8 },
                );
                self.sy = if key == Symbol::Undefined {
                    Symbol::Ident
                } else {
                    key
                };
            } else if is_digit(self.ch)
                || self.ch == b'.' as i32
                || self.ch == b'-' as i32
                || self.ch == b'+' as i32
            {
                self.scan_number()?;
                // `scan_number` fully resolves the symbol (number or ident quirk).
            } else {
                match self.ch {
                    // EOF markers: 0x1a, 0, -1.
                    0x1a | 0 | -1 => {
                        self.sy = Symbol::Eof;
                    }
                    // CR / CRLF.
                    x if x == b'\r' as i32 => {
                        self.next_ch();
                        if self.ch == b'\n' as i32 {
                            self.next_ch();
                        }
                        self.sy = Symbol::Eoln;
                        self.lineno += 1;
                    }
                    x if x == b'\n' as i32 => {
                        self.next_ch();
                        self.sy = Symbol::Eoln;
                        self.lineno += 1;
                    }
                    // Comment to end of line.
                    x if x == b'#' as i32 => {
                        self.next_ch();
                        while self.ch != 0 && self.ch != b'\n' as i32 && self.ch != b'\r' as i32 {
                            self.next_ch();
                        }
                        self.sy = Symbol::Comment;
                    }
                    // String.
                    x if x == b'\'' as i32 || x == b'"' as i32 => {
                        self.in_string_symbol()?;
                    }
                    other => {
                        return Err(self.syn_error(format!("Unrecognized character: 0x{other:x}")));
                    }
                }
            }

            if self.sy != Symbol::Comment {
                break;
            }
        }

        // Handle the `.include` special token.
        if self.sy == Symbol::Include {
            self.in_string_symbol()?;
            self.check(Symbol::Str, "Filename expected")?;
            let rel = self.str.clone();
            self.push_include(&rel)?;
            self.ch = b' ' as i32;
            self.in_symbol()?;
        }

        Ok(())
    }

    /// The numeric scanner branch of `InSymbol`, including the
    /// number-followed-by-identifier quirk.
    fn scan_number(&mut self) -> Result<()> {
        let mut sign = 1i32;
        if self.ch == b'-' as i32 {
            sign = -1;
            self.next_ch();
        } else if self.ch == b'+' as i32 {
            sign = 1;
            self.next_ch();
        }

        self.inum = 0;
        self.sy = Symbol::Inum;

        if self.ch == b'0' as i32 {
            // 0x… (hex) or 0b… (binary).
            self.next_ch();
            if to_upper(self.ch) == b'X' as i32 {
                self.next_ch();
                while is_xdigit(self.ch) {
                    let up = to_upper(self.ch);
                    let j = if (b'A' as i32..=b'F' as i32).contains(&up) {
                        up - b'A' as i32 + 10
                    } else {
                        self.ch - b'0' as i32
                    };
                    if self.inum as f64 * 16.0 + j as f64 > 2_147_483_647.0 {
                        return Err(self.syn_error("Invalid hexadecimal number"));
                    }
                    self.inum = self.inum * 16 + j;
                    self.next_ch();
                }
                return Ok(());
            }
            if to_upper(self.ch) == b'B' as i32 {
                self.next_ch();
                while self.ch == b'0' as i32 || self.ch == b'1' as i32 {
                    let j = self.ch - b'0' as i32;
                    if self.inum as f64 * 2.0 + j as f64 > 2_147_483_647.0 {
                        return Err(self.syn_error("Invalid binary number"));
                    }
                    self.inum = self.inum * 2 + j;
                    self.next_ch();
                }
                return Ok(());
            }
        }

        while is_digit(self.ch) {
            let digit = self.ch - b'0' as i32;
            if self.inum as f64 * 10.0 + digit as f64 > 2_147_483_647.0 {
                self.read_real(self.inum);
                self.sy = Symbol::Dnum;
                self.dnum *= sign as f64;
                return Ok(());
            }
            self.inum = self.inum * 10 + digit;
            self.next_ch();
        }

        if self.ch == b'.' as i32 {
            self.read_real(self.inum);
            self.sy = Symbol::Dnum;
            self.dnum *= sign as f64;
            return Ok(());
        }

        self.inum *= sign;

        // Numbers followed by identifier characters are taken as identifiers.
        if is_idchar(self.ch) {
            let prefix = if self.sy == Symbol::Inum {
                format!("{}", self.inum)
            } else {
                self.format_dbl(self.dnum)
            };
            self.id.clear();
            self.id.push_str(&prefix);
            loop {
                self.id.push(self.ch as u8 as char);
                self.next_ch();
                if !is_idchar(self.ch) {
                    break;
                }
            }
            self.sy = Symbol::Ident;
        }

        Ok(())
    }
}

/// `isabsolutepath` — a leading directory separator, or a Windows drive letter.
fn is_absolute_path(path: &str) -> bool {
    let b = path.as_bytes();
    if b.is_empty() {
        return false;
    }
    if b[0] == DIR_CHAR {
        return true;
    }
    #[cfg(windows)]
    {
        if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
            return true;
        }
    }
    false
}

/// Platform directory separator (`DIR_CHAR`).
#[cfg(windows)]
const DIR_CHAR: u8 = b'\\';
#[cfg(not(windows))]
const DIR_CHAR: u8 = b'/';

/// `BuildAbsolutePath` — resolve `rel` against the directory of `base`.
fn build_absolute_path(rel: &str, base: &str) -> Option<String> {
    if is_absolute_path(rel) {
        return Some(rel.to_string());
    }
    // Replace the file component of `base` with `rel`.
    let idx = base.as_bytes().iter().rposition(|&c| c == DIR_CHAR)?;
    let mut out = String::with_capacity(idx + 1 + rel.len());
    out.push_str(&base[..=idx]);
    out.push_str(rel);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::format_printf_double;

    /// `cmsIT8DefineDblFormat` stores the printf spec verbatim and `snprintf`s with it
    /// (`cmscgats.c:3049-3062` + four call sites). Honouring only `%.<n>g` rewrote every other
    /// spec as `%.10g`, so a sheet written by lcms with a custom formatter did not round-trip.
    #[test]
    fn double_formatter_honours_non_g_printf_specs() {
        // The reported failure: %.4f was silently downgraded to %.10g.
        assert_eq!(format_printf_double("%.4f", 1.23456789), "1.2346");
        assert_eq!(format_printf_double("%.2f", 1.23456789), "1.23");
        assert_eq!(format_printf_double("%.10g", 1.23456789), "1.23456789");

        // Default precision is 6 for every float conversion, as in C.
        assert_eq!(format_printf_double("%f", 1.5), "1.500000");

        // %e keeps a signed, at-least-two-digit exponent.
        assert_eq!(format_printf_double("%.3e", 12345.0), "1.234e+04");
        assert_eq!(format_printf_double("%.3E", 12345.0), "1.234E+04");
        assert_eq!(format_printf_double("%.2e", 0.00012), "1.20e-04");

        // Sign, width and zero-padding.
        assert_eq!(format_printf_double("%.1f", -2.25), "-2.2");
        assert_eq!(format_printf_double("%+.1f", 2.25), "+2.2");
        assert_eq!(format_printf_double("%8.2f", 3.5), "    3.50");
        assert_eq!(format_printf_double("%-8.2f", 3.5), "3.50    ");
        assert_eq!(format_printf_double("%08.2f", -3.5), "-0003.50");

        // Literal text around the conversion survives.
        assert_eq!(format_printf_double("[%.1f]", 2.0), "[2.0]");

        // An unparseable spec falls back to the library default, not a panic.
        assert_eq!(format_printf_double("no-spec", 1.5), format_printf_double("%.10g", 1.5));
    }
}
