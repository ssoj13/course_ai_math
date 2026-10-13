//! Higher-level parsing sections for the CGATS / IT8 / `.cube` grammar
//! (`ParseIT8`, `HeaderSection`, `DataFormatSection`, `DataSection`,
//! `CookPointers`, `ParseCube`, `IsMyBlock`).
//!
//! These are `impl It8` methods that drive the [`super::lexer::Lexer`] over the
//! [`super::table::Table`] model. Error handling differs from the C reference in
//! form but not in outcome: where lcms sets `sy = SSYNERROR` and unwinds through
//! `FALSE` return values (which in turn makes `ParseIT8` return `FALSE`), the
//! Rust port returns `Err` and propagates with `?` — the net result (a failed
//! load) is identical.

use crate::error::{ErrorCode, Result};

use super::It8;
use super::lexer::Symbol;
use super::table::{WriteMode, truncate_str};
use super::{CubeData, MAXID, MAXSTR};

impl It8 {
    /// `DataFormatSection` — read the `BEGIN_DATA_FORMAT … END_DATA_FORMAT`
    /// column labels.
    pub(super) fn data_format_section(&mut self) -> Result<()> {
        let mut i_field = 0i32;

        self.lx.in_symbol()?; // Eats "BEGIN_DATA_FORMAT"
        self.lx.check_eoln()?;

        while self.lx.sy != Symbol::EndDataFormat
            && self.lx.sy != Symbol::Eoln
            && self.lx.sy != Symbol::Eof
        {
            if self.lx.sy != Symbol::Ident {
                return Err(self.lx.syn_error("Sample type expected"));
            }
            let label = self.lx.id.clone();
            self.set_data_format(i_field, &label)?;
            i_field += 1;

            self.lx.in_symbol()?;
            self.lx.skip_eoln()?;
        }

        self.lx.skip_eoln()?;
        self.lx.skip(Symbol::EndDataFormat)?;
        self.lx.skip_eoln()?;

        // The reference signals here but keeps `sy = SSYNERROR`, which makes
        // `ParseIT8` fail overall; propagating an `Err` yields the same outcome.
        if i_field != self.table().n_samples {
            return Err(self.lx.syn_error(format!(
                "Count mismatch. NUMBER_OF_FIELDS was {}, found {}\n",
                self.table().n_samples,
                i_field
            )));
        }

        Ok(())
    }

    /// `DataSection` — read the `BEGIN_DATA … END_DATA` grid, auto-wrapping rows
    /// once `NUMBER_OF_FIELDS` values have been seen.
    pub(super) fn data_section(&mut self) -> Result<()> {
        let mut i_field = 0i32;
        let mut i_set = 0i32;

        self.lx.in_symbol()?; // Eats "BEGIN_DATA"
        self.lx.check_eoln()?;

        if self.table().data.is_none() {
            self.allocate_data_set()?;
        }

        while self.lx.sy != Symbol::EndData && self.lx.sy != Symbol::Eof {
            if i_field >= self.table().n_samples {
                i_field = 0;
                i_set += 1;
            }

            if self.lx.sy != Symbol::EndData && self.lx.sy != Symbol::Eof {
                match self.lx.sy {
                    Symbol::Ident => {
                        let v = self.lx.id.clone();
                        self.set_data_idx(i_set, i_field, &v)?;
                    }
                    Symbol::Str => {
                        let v = self.lx.str.clone();
                        self.set_data_idx(i_set, i_field, &v)?;
                    }
                    _ => {
                        let buf = self.lx.get_val(255, "Sample data expected")?;
                        self.set_data_idx(i_set, i_field, &buf)?;
                    }
                }

                i_field += 1;
                self.lx.in_symbol()?;
                self.lx.skip_eoln()?;
            }
        }

        self.lx.skip_eoln()?;
        self.lx.skip(Symbol::EndData)?;
        self.lx.skip_eoln()?;

        if (i_set + 1) != self.table().n_patches {
            return Err(self.lx.syn_error(format!(
                "Count mismatch. NUMBER_OF_SETS was {}, found {}\n",
                self.table().n_patches,
                i_set + 1
            )));
        }

        Ok(())
    }

    /// `HeaderSection` — read properties until a data section begins. Handles the
    /// `KEYWORD` / `DATA_FORMAT_IDENTIFIER` extensions and the `WRITE_PAIR`
    /// dictionary split on `';'` / `','`.
    pub(super) fn header_section(&mut self) -> Result<()> {
        while self.lx.sy != Symbol::Eof
            && self.lx.sy != Symbol::BeginDataFormat
            && self.lx.sy != Symbol::BeginData
        {
            match self.lx.sy {
                Symbol::Keyword => {
                    self.lx.in_symbol()?;
                    let buf = self.lx.get_val(MAXSTR - 1, "Keyword expected")?;
                    self.add_available_property(&buf, WriteMode::Uncooked)?;
                    self.lx.in_symbol()?;
                }

                Symbol::DataFormatId => {
                    self.lx.in_symbol()?;
                    let buf = self.lx.get_val(MAXSTR - 1, "Keyword expected")?;
                    self.add_available_sample_id(&buf)?;
                    self.lx.in_symbol()?;
                }

                Symbol::Ident => {
                    let var_name = truncate_str(&self.lx.id, MAXID - 1);

                    // Look the keyword up in the valid list; in non-strict mode an
                    // unknown keyword is registered on the fly (uncooked).
                    let (found, idx) = self.valid_keywords.locate(&var_name, None);
                    let write_as = if found {
                        self.valid_keywords.nodes[idx.expect("found implies index")].write_as
                    } else {
                        self.add_available_property(&var_name, WriteMode::Uncooked)?;
                        WriteMode::Uncooked
                    };

                    self.lx.in_symbol()?;
                    let buf = self.lx.get_val(MAXSTR - 1, "Property data expected")?;

                    if write_as != WriteMode::Pair {
                        let mode = if self.lx.sy == Symbol::Str {
                            WriteMode::Stringify
                        } else {
                            WriteMode::Uncooked
                        };
                        self.table_mut()
                            .header
                            .add(&var_name, None, Some(&buf), mode)?;
                    } else {
                        if self.lx.sy != Symbol::Str {
                            return Err(self.lx.syn_error(format!(
                                "Invalid value '{buf}' for property '{var_name}'."
                            )));
                        }

                        // Chop the string into "subkey, value" pairs on ';', with
                        // ',' (rightmost) splitting each pair; spaces are trimmed.
                        for seg in buf.split(';') {
                            let comma = seg.rfind(',').ok_or_else(|| {
                                self.lx
                                    .syn_error(format!("Invalid value for property '{var_name}'."))
                            })?;
                            let sub = seg[..comma].trim_matches(' ');
                            let val = seg[comma + 1..].trim_matches(' ');
                            if sub.is_empty() || val.is_empty() {
                                return Err(self.lx.syn_error(format!(
                                    "Invalid value for property '{var_name}'."
                                )));
                            }
                            self.table_mut().header.add(
                                &var_name,
                                Some(sub),
                                Some(val),
                                WriteMode::Pair,
                            )?;
                        }
                    }

                    self.lx.in_symbol()?;
                }

                Symbol::Eoln => {}

                _ => return Err(self.lx.syn_error("expected keyword or identifier")),
            }

            self.lx.skip_eoln()?;
        }

        Ok(())
    }

    /// `ParseIT8` — the top-level driver: optional sheet-type line, then a stream
    /// of header/data sections, allocating a fresh table after each data block.
    pub(super) fn parse_it8(&mut self, nosheet: bool) -> Result<()> {
        if !nosheet {
            let ty = self.lx.read_type();
            self.tables[0].sheet_type = ty;
        }

        self.lx.in_symbol()?;
        self.lx.skip_eoln()?;

        while self.lx.sy != Symbol::Eof {
            match self.lx.sy {
                Symbol::BeginDataFormat => self.data_format_section()?,

                Symbol::BeginData => {
                    self.data_section()?;

                    if self.lx.sy != Symbol::Eof {
                        self.alloc_table()?;
                        self.n_table = self.tables.len() - 1;

                        // Optional sheet type for the next table. Only `<ident>
                        // <eoln>` or a quoted string count as a type line.
                        if !nosheet {
                            if self.lx.sy == Symbol::Ident {
                                if self.lx.skip_separators_at_newline() {
                                    let id = self.lx.id.clone();
                                    self.set_sheet_type(&id);
                                    self.lx.in_symbol()?;
                                } else {
                                    self.set_sheet_type("");
                                }
                            } else if self.lx.sy == Symbol::Str {
                                let s = self.lx.str.clone();
                                self.set_sheet_type(&s);
                                self.lx.in_symbol()?;
                            }
                        }
                    }
                }

                Symbol::Eoln => self.lx.skip_eoln()?,

                _ => self.header_section()?,
            }
        }

        Ok(())
    }

    /// `CookPointers` — resolve `SAMPLE_ID` column positions and rewrite `LABEL` /
    /// `$`-prefixed cross-table references into `"Label nTable Type"`.
    ///
    /// Void in the reference: a missing `DATA_FORMAT` or an out-of-range write is
    /// signalled but does not fail an otherwise-successful load, so this method
    /// signals through the context and returns instead of propagating.
    pub(super) fn cook_pointers(&mut self) {
        let n_old = self.n_table;
        let n_tables = self.tables.len();

        // Collect rewrites to apply after the read phase, avoiding overlapping
        // borrows between the source header lists and the destination data grid.
        let mut writes: Vec<(usize, i32, i32, String)> = Vec::new();

        for j in 0..n_tables {
            self.tables[j].sample_id = 0;
            let n_samples = self.tables[j].n_samples;

            for id_field in 0..n_samples {
                let fld = match &self.tables[j].data_format {
                    None => {
                        self.ctx
                            .signal(ErrorCode::CorruptionDetected, "Undefined DATA_FORMAT");
                        return;
                    }
                    Some(df) => df
                        .get(id_field as usize)
                        .and_then(|s| s.clone()),
                };
                let fld = match fld {
                    Some(f) => f,
                    None => continue,
                };

                if fld.eq_ignore_ascii_case("SAMPLE_ID") {
                    self.tables[j].sample_id = id_field;
                }

                // "LABEL" (an extension) and any "$"-prefixed column hold forward
                // references to other tables.
                if fld.eq_ignore_ascii_case("LABEL") || fld.as_bytes().first() == Some(&b'$') {
                    for i in 0..self.tables[j].n_patches {
                        let label = match self.tables[j].get_data(i, id_field) {
                            Some(l) => l.to_string(),
                            None => continue,
                        };

                        // Last matching table wins (the reference re-writes on
                        // every match).
                        let mut newval: Option<String> = None;
                        for (k, table) in self.tables.iter().enumerate() {
                            let (found, idx) = table.header.locate(&label, None);
                            if found {
                                let ty = table.header.nodes[idx.expect("found implies index")]
                                    .value
                                    .clone()
                                    .unwrap_or_default();
                                newval = Some(format!("{label} {k} {ty}"));
                            }
                        }

                        if let Some(v) = newval {
                            writes.push((j, i, id_field, v));
                        }
                    }
                }
            }
        }

        for (j, i, field, v) in writes {
            self.n_table = j;
            if let Err(e) = self.set_data_idx(i, field, &v) {
                self.ctx.signal(e.code, &e.message);
            }
        }

        self.n_table = n_old;
    }

    /// `ReadNumbers` — read `n` numeric values then require an end-of-line.
    fn read_numbers(&mut self, arr: &mut [f64]) -> Result<()> {
        for slot in arr.iter_mut() {
            *slot = match self.lx.sy {
                Symbol::Inum => self.lx.inum as f64,
                Symbol::Dnum => self.lx.dnum,
                _ => return Err(self.lx.syn_error("Number expected")),
            };
            self.lx.in_symbol()?;
        }
        self.lx.check_eoln()
    }

    /// `ParseCube` — parse a `.cube` stream into a [`CubeData`]. The 3D CLUT is
    /// stored BGR-swapped (channel indices 2/1/0) exactly like the reference, so
    /// that the later device-link bridge maps R/G/B correctly.
    pub(super) fn parse_cube(&mut self) -> Result<CubeData> {
        let mut domain_min = [0.0f64, 0.0, 0.0];
        let mut domain_max = [1.0f64, 1.0, 1.0];
        let mut check_0_1 = [0.0f64, 1.0];
        let mut shaper_size = 0i32;
        let mut lut_size = 0i32;
        let mut title = String::new();
        let mut shaper: Option<Vec<f32>> = None;
        let mut clut: Option<Vec<f32>> = None;

        self.lx.in_symbol()?;

        while self.lx.sy != Symbol::Eof {
            match self.lx.sy {
                Symbol::Title => {
                    self.lx.in_symbol()?;
                    self.lx.check(Symbol::Str, "Title string expected")?;
                    title = truncate_str(&self.lx.str, MAXSTR - 1);
                    self.lx.in_symbol()?;
                }

                Symbol::DomainMin => {
                    self.lx.in_symbol()?;
                    self.read_numbers(&mut domain_min)?;
                }

                Symbol::DomainMax => {
                    self.lx.in_symbol()?;
                    self.read_numbers(&mut domain_max)?;
                }

                Symbol::Lut1dSize => {
                    self.lx.in_symbol()?;
                    self.lx.check(Symbol::Inum, "Shaper size expected")?;
                    shaper_size = self.lx.inum;
                    if !(2..=65536).contains(&shaper_size) {
                        return Err(self
                            .lx
                            .syn_error(format!("LUT_1D_SIZE '{shaper_size}' is out of bounds")));
                    }
                    self.lx.in_symbol()?;
                }

                Symbol::Lut3dSize => {
                    self.lx.in_symbol()?;
                    self.lx.check(Symbol::Inum, "LUT size expected")?;
                    lut_size = self.lx.inum;
                    self.lx.in_symbol()?;
                }

                Symbol::Lut1dInputRange | Symbol::Lut3dInputRange => {
                    self.lx.in_symbol()?;
                    self.read_numbers(&mut check_0_1)?;
                    if check_0_1[0] != 0.0 || check_0_1[1] != 1.0 {
                        return Err(self.lx.syn_error("Unsupported format"));
                    }
                }

                Symbol::Eoln => {
                    self.lx.in_symbol()?;
                }

                Symbol::LutInVideoRange | Symbol::LutOutVideoRange => {
                    return Err(self.lx.syn_error("Unsupported format"));
                }

                // Read and create tables.
                Symbol::Inum | Symbol::Dnum => {
                    if shaper_size > 0 {
                        let ss = shaper_size as usize;
                        let mut shp = vec![0f32; 3 * ss];
                        for i in 0..ss {
                            let mut nums = [0.0f64; 3];
                            self.read_numbers(&mut nums)?;
                            shp[i] = ((nums[0] - domain_min[0]) / (domain_max[0] - domain_min[0]))
                                as f32;
                            shp[i + ss] = ((nums[1] - domain_min[1])
                                / (domain_max[1] - domain_min[1]))
                                as f32;
                            shp[i + 2 * ss] = ((nums[2] - domain_min[2])
                                / (domain_max[2] - domain_min[2]))
                                as f32;
                        }
                        shaper = Some(shp);
                    }

                    if lut_size > 0 {
                        // Professional LUT tools cap at 65×65×65.
                        if !(2..=65).contains(&lut_size) {
                            return Err(self
                                .lx
                                .syn_error(format!("LUT size '{lut_size}' is not allowed")));
                        }
                        let ls = lut_size as usize;
                        let nodes = ls * ls * ls;
                        let mut lut = vec![0f32; nodes * 3];
                        for i in 0..nodes {
                            let mut nums = [0.0f64; 3];
                            self.read_numbers(&mut nums)?;
                            lut[i * 3 + 2] = ((nums[0] - domain_min[0])
                                / (domain_max[0] - domain_min[0]))
                                as f32;
                            lut[i * 3 + 1] = ((nums[1] - domain_min[1])
                                / (domain_max[1] - domain_min[1]))
                                as f32;
                            lut[i * 3] = ((nums[2] - domain_min[2])
                                / (domain_max[2] - domain_min[2]))
                                as f32;
                        }
                        clut = Some(lut);
                    }

                    self.lx.check(Symbol::Eof, "Extra symbols found in file")?;
                }

                _ => return Err(self.lx.syn_error("Unsupported format")),
            }
        }

        Ok(CubeData {
            title,
            shaper_size,
            lut_size,
            domain_min,
            domain_max,
            shaper,
            clut,
        })
    }
}

/// `IsMyBlock` — sniff whether a buffer looks like a CGATS/IT8 stream. Returns 0
/// if not, otherwise the word count of the first line (1 or 2), used to decide
/// whether the first line is a sheet-type line.
pub(super) fn is_my_block(buffer: &[u8]) -> i32 {
    let mut words = 1i32;
    let mut space = 0i32;
    let mut quot = 0i32;

    let n = buffer.len();
    if n < 10 {
        return 0; // Too small.
    }
    let n = n.min(132);

    for &b in &buffer[1..n] {
        match b {
            b'\n' | b'\r' => {
                return if quot == 1 || words > 2 { 0 } else { words };
            }
            b'\t' | b' ' => {
                if quot == 0 && space == 0 {
                    space = 1;
                }
            }
            b'"' => {
                quot = if quot == 0 { 1 } else { 0 };
            }
            _ => {
                if b < 32 {
                    return 0;
                }
                if b > 127 {
                    return 0;
                }
                words += space;
                space = 0;
            }
        }
    }

    0
}
