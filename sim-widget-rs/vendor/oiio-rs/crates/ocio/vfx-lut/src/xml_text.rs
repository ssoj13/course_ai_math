//! THE way the XML readers in this crate (`.cc`, `.ccc`, `.cdl`, CLF/CTF) collect an element's
//! text.
//!
//! quick-xml reports text as a run of events: `Text` pieces, `GeneralRef` for every entity
//! or character reference (`&lt;`, `&amp;`, `&#x41;`) and `CData`. The readers used to keep only
//! `Text` (so `a &lt;b&gt;` read as `ab`) with `trim_text(true)` trimming each piece (so the
//! spaces around a reference vanished too). Readers now append every piece here, turn trimming
//! off, and trim the whole value where they use it.

use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesCData, BytesRef, BytesText};

use crate::{LutError, LutResult};

fn err(e: impl std::fmt::Display) -> LutError {
    LutError::ParseError(format!("XML text: {e}"))
}

/// Appends a `Text` event's content as written (quick-xml events are UTF-8 `str` since 0.42; no EOL
/// normalization, matching what the readers did before).
pub(crate) fn push_text(out: &mut String, t: &BytesText) {
    out.push_str(t);
}

/// Appends a `CData` event's content, verbatim.
pub(crate) fn push_cdata(out: &mut String, c: &BytesCData) {
    out.push_str(c);
}

/// Appends what a `GeneralRef` event stands for: a character reference, or one of XML's five
/// predefined entities. Any other entity needs a DTD these formats do not have, so it is an
/// error rather than silently dropped text.
pub(crate) fn push_ref(out: &mut String, r: &BytesRef) -> LutResult<()> {
    if let Some(c) = r.resolve_char_ref().map_err(err)? {
        out.push(c);
        return Ok(());
    }
    let name: &str = r;
    let text = resolve_predefined_entity(name)
        .ok_or_else(|| LutError::ParseError(format!("XML text: unknown entity &{name};")))?;
    out.push_str(text);
    Ok(())
}
