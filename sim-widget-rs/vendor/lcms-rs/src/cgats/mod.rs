//! CGATS.17 / IT8.7 / `.cube` text parser (`cmscgats.c`).
//!
//! A pure-Rust port of lcms2's `cmsIT8*` family: a reader/writer for the ASCII
//! measurement-data interchange formats (CGATS.17, ANSI IT8.7/x, CCMX/CCSS) plus
//! the Adobe/IRIDAS `.cube` 3D-LUT format. The C reference stores everything in a
//! bump allocator threaded through intrusive linked lists; this port replaces
//! that with owned [`Table`]/[`KeyValueList`] structures ([`table`]) while
//! reproducing the exact lexer quirks ([`lexer`]) and parser behaviour
//! ([`parser`]).
//!
//! # Layout
//! - [`lexer`] — the tokenizer (`InSymbol`, `ReadReal`, `ParseFloatNumber`, the
//!   number-becomes-identifier quirk, the `.include` file stack).
//! - [`table`] — the owned data model (`TABLE`, `KEYVALUE`, `WRITEMODE`).
//! - [`parser`] — the grammar (`ParseIT8`, section readers, `CookPointers`,
//!   `ParseCube`, `IsMyBlock`).
//!
//! # Public entry points
//! [`It8::load_from_mem`] / [`It8::load_from_file`] parse a sheet;
//! [`It8::save_to_mem`] / [`It8::save_to_file`] serialise it back. The remaining
//! methods mirror the `cmsIT8Get*` / `cmsIT8Set*` accessors. `.cube` parsing is
//! exposed through [`It8::load_cube_from_file`] returning [`CubeData`]; see the
//! note on the device-link bridge at [`create_device_link_from_cube_file`].

mod lexer;
mod parser;
mod table;

use std::path::Path;

use crate::color::gamma::ToneCurve;
use crate::context::Context;
use crate::error::{Error, ErrorCode, Result};
use crate::named::mlu::Mlu;
use crate::pipeline::{Pipeline, Stage, StageLoc};
use crate::profile::Profile;
use crate::tags::descriptor::tag_sig;
use crate::tags::TagValue;
use crate::types::sig::{class, colorspace};

use lexer::{Lexer, parse_float_number};
use table::{KeyValueList, Table, WriteMode, satoi, satob, truncate_str};

// ---- reference limits (`#define`s in cmscgats.c) ----

/// Max length of an identifier (`MAXID`).
const MAXID: usize = 128;
/// Max length of a string (`MAXSTR`).
const MAXSTR: usize = 1024;
/// Max number of tables in a single stream (`MAXTABLES`).
const MAXTABLES: usize = 255;
/// Max number of nested includes (`MAXINCLUDE`).
const MAXINCLUDE: usize = 20;
/// Default double formatter (`DEFAULT_DBL_FORMAT`).
const DEFAULT_DBL_FORMAT: &str = "%.10g";

/// Predefined header properties (`PredefinedProperties`): the well-known keywords
/// and how each is serialised.
static PREDEFINED_PROPERTIES: &[(&str, WriteMode)] = &[
    ("NUMBER_OF_FIELDS", WriteMode::Uncooked),
    ("NUMBER_OF_SETS", WriteMode::Uncooked),
    ("ORIGINATOR", WriteMode::Stringify),
    ("FILE_DESCRIPTOR", WriteMode::Stringify),
    ("CREATED", WriteMode::Stringify),
    ("DESCRIPTOR", WriteMode::Stringify),
    ("DIFFUSE_GEOMETRY", WriteMode::Stringify),
    ("MANUFACTURER", WriteMode::Stringify),
    ("MANUFACTURE", WriteMode::Stringify),
    ("PROD_DATE", WriteMode::Stringify),
    ("SERIAL", WriteMode::Stringify),
    ("MATERIAL", WriteMode::Stringify),
    ("INSTRUMENTATION", WriteMode::Stringify),
    ("MEASUREMENT_SOURCE", WriteMode::Stringify),
    ("PRINT_CONDITIONS", WriteMode::Stringify),
    ("SAMPLE_BACKING", WriteMode::Stringify),
    ("CHISQ_DOF", WriteMode::Stringify),
    ("MEASUREMENT_GEOMETRY", WriteMode::Stringify),
    ("FILTER", WriteMode::Stringify),
    ("POLARIZATION", WriteMode::Stringify),
    ("WEIGHTING_FUNCTION", WriteMode::Pair),
    ("COMPUTATIONAL_PARAMETER", WriteMode::Pair),
    ("TARGET_TYPE", WriteMode::Stringify),
    ("COLORANT", WriteMode::Stringify),
    ("TABLE_DESCRIPTOR", WriteMode::Stringify),
    ("TABLE_NAME", WriteMode::Stringify),
];

/// Predefined dataset sample identifiers (`PredefinedSampleID`).
static PREDEFINED_SAMPLE_ID: &[&str] = &[
    "SAMPLE_ID",
    "STRING",
    "CMYK_C",
    "CMYK_M",
    "CMYK_Y",
    "CMYK_K",
    "D_RED",
    "D_GREEN",
    "D_BLUE",
    "D_VIS",
    "D_MAJOR_FILTER",
    "RGB_R",
    "RGB_G",
    "RGB_B",
    "SPECTRAL_NM",
    "SPECTRAL_PCT",
    "SPECTRAL_DEC",
    "XYZ_X",
    "XYZ_Y",
    "XYZ_Z",
    "XYY_X",
    "XYY_Y",
    "XYY_CAPY",
    "LAB_L",
    "LAB_A",
    "LAB_B",
    "LAB_C",
    "LAB_H",
    "LAB_DE",
    "LAB_DE_94",
    "LAB_DE_CMC",
    "LAB_DE_2000",
    "MEAN_DE",
    "STDEV_X",
    "STDEV_Y",
    "STDEV_Z",
    "STDEV_L",
    "STDEV_A",
    "STDEV_B",
    "STDEV_DE",
    "CHI_SQD_PAR",
];

/// An open IT8 handler (`cmsIT8`): a set of [`Table`]s plus the tokenizer state
/// and the stream-wide valid-keyword / valid-sample lists.
pub struct It8 {
    /// Tokenizer + source stack.
    lx: Lexer,
    /// The tables in this stream (`Tab`); `tables.len()` is `TablesCount`.
    tables: Vec<Table>,
    /// The currently selected table (`nTable`).
    n_table: usize,
    /// Stream-wide allowed keywords (`ValidKeywords`).
    valid_keywords: KeyValueList,
    /// Stream-wide allowed sample identifiers (`ValidSampleID`).
    valid_sample_id: KeyValueList,
    /// The threading context, for `cmsSignalError`-style diagnostics.
    ctx: Context,
}

/// Parsed contents of a `.cube` stream (`ParseCube` outputs).
///
/// The shaper is laid out as three contiguous per-channel blocks of
/// `shaper_size` floats; the CLUT is `lut_size³` RGB triples stored **BGR-swapped**
/// (index `+2` = R, `+1` = G, `+0` = B) exactly as the reference builds it.
#[derive(Debug, Clone)]
pub struct CubeData {
    /// `TITLE` string, if present.
    pub title: String,
    /// `LUT_1D_SIZE` (0 when absent).
    pub shaper_size: i32,
    /// `LUT_3D_SIZE` (0 when absent).
    pub lut_size: i32,
    /// `DOMAIN_MIN`.
    pub domain_min: [f64; 3],
    /// `DOMAIN_MAX`.
    pub domain_max: [f64; 3],
    /// 1D shaper table (`3 * shaper_size` floats) if a `LUT_1D_SIZE` was given.
    pub shaper: Option<Vec<f32>>,
    /// 3D CLUT (`lut_size³ * 3` floats, BGR-swapped) if a `LUT_3D_SIZE` was given.
    pub clut: Option<Vec<f32>>,
}

impl It8 {
    // ---- construction (`cmsIT8Alloc`) ----

    /// `cmsIT8Alloc` — an empty handler with one table, the default sheet type
    /// (`"CGATS.17"`), the default double formatter and the predefined property /
    /// sample lists installed.
    pub fn new(ctx: &Context) -> Self {
        let mut it8 = It8 {
            lx: Lexer::from_mem("", &[]),
            tables: vec![Table::new()],
            n_table: 0,
            valid_keywords: KeyValueList::new(),
            valid_sample_id: KeyValueList::new(),
            ctx: ctx.clone(),
        };

        it8.tables[0].set_sheet_type("CGATS.17");

        for &(id, as_mode) in PREDEFINED_PROPERTIES {
            it8.valid_keywords
                .add(id, None, None, as_mode)
                .expect("predefined properties are unique");
        }
        for &id in PREDEFINED_SAMPLE_ID {
            it8.valid_sample_id
                .add(id, None, None, WriteMode::Uncooked)
                .expect("predefined sample IDs are unique");
        }

        it8
    }

    // ---- internal table helpers ----

    /// `GetTable` — the currently selected table.
    fn table(&self) -> &Table {
        &self.tables[self.n_table]
    }

    /// Mutable access to the currently selected table.
    fn table_mut(&mut self) -> &mut Table {
        &mut self.tables[self.n_table]
    }

    /// `AllocTable` — append a fresh empty table.
    fn alloc_table(&mut self) -> Result<()> {
        if self.tables.len() >= MAXTABLES - 1 {
            return Err(self.lx.syn_error("Too many tables"));
        }
        self.tables.push(Table::new());
        Ok(())
    }

    /// `AddAvailableProperty`.
    fn add_available_property(&mut self, key: &str, as_mode: WriteMode) -> Result<()> {
        self.valid_keywords.add(key, None, None, as_mode).map(|_| ())
    }

    /// `AddAvailableSampleID`.
    fn add_available_sample_id(&mut self, key: &str) -> Result<()> {
        self.valid_sample_id
            .add(key, None, None, WriteMode::Uncooked)
            .map(|_| ())
    }

    /// `AllocateDataFormat` — size and allocate the column-label array from
    /// `NUMBER_OF_FIELDS`.
    fn allocate_data_format(&mut self) -> Result<()> {
        if self.table().data_format.is_some() {
            return Ok(());
        }
        let n = satoi(self.get_property("NUMBER_OF_FIELDS"));
        if n <= 0 || n > 0x7ffe {
            return Err(self.lx.syn_error("Wrong NUMBER_OF_FIELDS"));
        }
        let t = self.table_mut();
        t.n_samples = n;
        t.data_format = Some(vec![None; n as usize + 1]);
        Ok(())
    }

    /// `AllocateDataSet` — size and allocate the data grid from
    /// `NUMBER_OF_FIELDS` × `NUMBER_OF_SETS`.
    fn allocate_data_set(&mut self) -> Result<()> {
        if self.table().data.is_some() {
            return Ok(());
        }
        let n_samples = satoi(self.get_property("NUMBER_OF_FIELDS"));
        let n_patches = satoi(self.get_property("NUMBER_OF_SETS"));
        if !(0..=0x7ffe).contains(&n_samples)
            || !(0..=0x7ffe).contains(&n_patches)
            || (n_patches as i64 * n_samples as i64) > 200_000
        {
            return Err(self.lx.syn_error("AllocateDataSet: too much data"));
        }
        let size = (n_samples as usize + 1) * (n_patches as usize + 1);
        let t = self.table_mut();
        t.n_samples = n_samples;
        t.n_patches = n_patches;
        t.data = Some(vec![None; size]);
        Ok(())
    }

    /// `SetDataFormat` / `cmsIT8SetDataFormat` — set column `n`'s label,
    /// allocating the format array from `NUMBER_OF_FIELDS` on first use.
    pub fn set_data_format(&mut self, n: i32, label: &str) -> Result<()> {
        // No `debug_assert!(n >= 0)` here: `cmsIT8SetDataFormat` simply returns FALSE for a bad
        // index, and the graceful `n < 0` rejection five lines down already covers it. An
        // assertion would panic in a debug build where C - and this function in release -
        // returns an error.
        if self.table().data_format.is_none() {
            self.allocate_data_format()?;
        }
        let n_samples = self.table().n_samples;
        if n < 0 || n >= n_samples {
            return Err(self
                .lx
                .syn_error("Invalid or more than NUMBER_OF_FIELDS fields."));
        }
        if let Some(df) = self.table_mut().data_format.as_mut() {
            df[n as usize] = Some(label.to_string());
        }
        Ok(())
    }

    /// `SetData` — set the grid cell `(nSet, nField)`.
    fn set_data_idx(&mut self, n_set: i32, n_field: i32, val: &str) -> Result<()> {
        if self.table().data.is_none() {
            self.allocate_data_set()?;
        }
        if self.table().data.is_none() {
            return Err(self.lx.syn_error("Unable to allocate data array"));
        }
        let n_patches = self.table().n_patches;
        let n_samples = self.table().n_samples;
        if n_set > n_patches || n_set < 0 {
            return Err(self.lx.syn_error(format!(
                "Patch {n_set} out of range, there are {n_patches} patches"
            )));
        }
        if n_field > n_samples || n_field < 0 {
            return Err(self.lx.syn_error(format!(
                "Sample {n_field} out of range, there are {n_samples} samples"
            )));
        }
        let idx = n_set as usize * n_samples as usize + n_field as usize;
        if let Some(d) = self.table_mut().data.as_mut() {
            d[idx] = Some(val.to_string());
        }
        Ok(())
    }

    // ---- sheet type / comments (`cmsIT8*SheetType`, `cmsIT8SetComment`) ----

    /// `cmsIT8GetSheetType`.
    pub fn sheet_type(&self) -> &str {
        &self.table().sheet_type
    }

    /// `cmsIT8SetSheetType`.
    pub fn set_sheet_type(&mut self, ty: &str) {
        self.table_mut().set_sheet_type(ty);
    }

    /// `cmsIT8SetComment` — append a `#` comment line.
    pub fn set_comment(&mut self, val: &str) -> Result<()> {
        if val.is_empty() {
            return Err(Error::new(ErrorCode::Null, "empty comment"));
        }
        self.table_mut()
            .header
            .add("# ", None, Some(val), WriteMode::Uncooked)
            .map(|_| ())
    }

    // ---- property getters (`cmsIT8GetProperty*`) ----

    /// `cmsIT8GetProperty` — a header property's value.
    pub fn get_property(&self, key: &str) -> Option<&str> {
        self.table().header.get_value(key, None)
    }

    /// `cmsIT8GetPropertyDbl` — a header property parsed as a float.
    pub fn get_property_dbl(&self, key: &str) -> f64 {
        parse_float_number(self.get_property(key))
    }

    /// `cmsIT8GetPropertyMulti` — a `WRITE_PAIR` dictionary sub-value.
    pub fn get_property_multi(&self, key: &str, subkey: &str) -> Option<&str> {
        self.table().header.get_value(key, Some(subkey))
    }

    // ---- property setters (`cmsIT8SetProperty*`) ----

    /// `cmsIT8SetPropertyStr`.
    pub fn set_property_str(&mut self, key: &str, val: &str) -> Result<()> {
        if val.is_empty() {
            return Err(Error::new(ErrorCode::Null, "empty value"));
        }
        self.table_mut()
            .header
            .add(key, None, Some(val), WriteMode::Stringify)
            .map(|_| ())
    }

    /// `cmsIT8SetPropertyDbl`.
    pub fn set_property_dbl(&mut self, key: &str, val: f64) -> Result<()> {
        let buf = self.lx.format_dbl(val);
        self.table_mut()
            .header
            .add(key, None, Some(&buf), WriteMode::Uncooked)
            .map(|_| ())
    }

    /// `cmsIT8SetPropertyHex`.
    pub fn set_property_hex(&mut self, key: &str, val: u32) -> Result<()> {
        let buf = format!("{val}");
        self.table_mut()
            .header
            .add(key, None, Some(&buf), WriteMode::Hexadecimal)
            .map(|_| ())
    }

    /// `cmsIT8SetPropertyUncooked`.
    pub fn set_property_uncooked(&mut self, key: &str, val: &str) -> Result<()> {
        self.table_mut()
            .header
            .add(key, None, Some(val), WriteMode::Uncooked)
            .map(|_| ())
    }

    /// `cmsIT8SetPropertyMulti` — add a `WRITE_PAIR` dictionary entry.
    pub fn set_property_multi(&mut self, key: &str, subkey: &str, val: &str) -> Result<()> {
        self.table_mut()
            .header
            .add(key, Some(subkey), Some(val), WriteMode::Pair)
            .map(|_| ())
    }

    // ---- data by patch/sample name (`cmsIT8GetData` / `cmsIT8SetData`) ----

    /// `LocateSample` — column index of a named sample, or `-1`.
    fn locate_sample(&self, sample: &str) -> i32 {
        let t = self.table();
        for i in 0..t.n_samples {
            if let Some(f) = t.get_data_format(i) {
                if f.eq_ignore_ascii_case(sample) {
                    return i;
                }
            }
        }
        -1
    }

    /// `LocatePatch` — row index whose `SAMPLE_ID` matches, or `-1`.
    fn locate_patch(&self, patch: &str) -> i32 {
        let t = self.table();
        for i in 0..t.n_patches {
            if let Some(d) = t.get_data(i, t.sample_id) {
                if d.eq_ignore_ascii_case(patch) {
                    return i;
                }
            }
        }
        -1
    }

    /// `LocateEmptyPatch` — first row with no `SAMPLE_ID`, or `-1`.
    fn locate_empty_patch(&self) -> i32 {
        let t = self.table();
        for i in 0..t.n_patches {
            if t.get_data(i, t.sample_id).is_none() {
                return i;
            }
        }
        -1
    }

    /// `cmsIT8FindDataFormat` — column index of a named sample, or `None`.
    pub fn find_data_format(&self, sample: &str) -> Option<usize> {
        let i = self.locate_sample(sample);
        (i >= 0).then_some(i as usize)
    }

    /// `cmsIT8GetData` — grid value by patch and sample name.
    pub fn get_data(&self, patch: &str, sample: &str) -> Option<&str> {
        let i_field = self.locate_sample(sample);
        if i_field < 0 {
            return None;
        }
        let i_set = self.locate_patch(patch);
        if i_set < 0 {
            return None;
        }
        self.table().get_data(i_set, i_field)
    }

    /// `cmsIT8GetDataDbl` — grid value by name parsed as a float.
    pub fn get_data_dbl(&self, patch: &str, sample: &str) -> f64 {
        parse_float_number(self.get_data(patch, sample))
    }

    /// `cmsIT8GetDataRowCol` — grid value by row/column.
    pub fn get_data_row_col(&self, row: i32, col: i32) -> Option<&str> {
        self.table().get_data(row, col)
    }

    /// `cmsIT8GetDataRowColDbl`.
    pub fn get_data_row_col_dbl(&self, row: i32, col: i32) -> f64 {
        parse_float_number(self.get_data_row_col(row, col))
    }

    /// `cmsIT8SetDataRowCol`.
    pub fn set_data_row_col(&mut self, row: i32, col: i32, val: &str) -> Result<()> {
        self.set_data_idx(row, col, val)
    }

    /// `cmsIT8SetDataRowColDbl`.
    pub fn set_data_row_col_dbl(&mut self, row: i32, col: i32, val: f64) -> Result<()> {
        let buf = self.lx.format_dbl(val);
        self.set_data_idx(row, col, &buf)
    }

    /// `cmsIT8SetData` — set a grid value by patch and sample name, growing the
    /// grid / assigning a fresh patch for `SAMPLE_ID` writes as the reference does.
    pub fn set_data(&mut self, patch: &str, sample: &str, val: &str) -> Result<()> {
        let i_field = self.locate_sample(sample);
        if i_field < 0 {
            return Err(Error::new(
                ErrorCode::NotSuitable,
                format!("sample '{sample}' not found"),
            ));
        }

        if self.table().n_patches == 0 {
            self.allocate_data_format()?;
            self.allocate_data_set()?;
            self.cook_pointers();
        }

        let (i_set, i_field) = if sample.eq_ignore_ascii_case("SAMPLE_ID") {
            let e = self.locate_empty_patch();
            if e < 0 {
                return Err(self
                    .lx
                    .syn_error(format!("Couldn't add more patches '{patch}'\n")));
            }
            (e, self.table().sample_id)
        } else {
            let p = self.locate_patch(patch);
            if p < 0 {
                return Err(Error::new(
                    ErrorCode::NotSuitable,
                    format!("patch '{patch}' not found"),
                ));
            }
            (p, i_field)
        };

        self.set_data_idx(i_set, i_field, val)
    }

    /// `cmsIT8SetDataDbl`.
    pub fn set_data_dbl(&mut self, patch: &str, sample: &str, val: f64) -> Result<()> {
        let buf = self.lx.format_dbl(val);
        self.set_data(patch, sample, &buf)
    }

    // ---- enumeration (`cmsIT8Enum*`) ----

    /// `cmsIT8EnumDataFormat` — the column labels and their count.
    pub fn enum_data_format(&self) -> (i32, Vec<Option<&str>>) {
        let t = self.table();
        let labels = match &t.data_format {
            None => Vec::new(),
            Some(df) => (0..t.n_samples as usize)
                .map(|i| df.get(i).and_then(|s| s.as_deref()))
                .collect(),
        };
        (t.n_samples, labels)
    }

    /// `cmsIT8EnumProperties` — the header property keywords, in order.
    pub fn enum_properties(&self) -> Vec<&str> {
        self.table()
            .header
            .nodes
            .iter()
            .map(|p| p.keyword.as_str())
            .collect()
    }

    /// `cmsIT8EnumPropertyMulti` — the sub-keys of a `WRITE_PAIR` property.
    pub fn enum_property_multi(&self, key: &str) -> Vec<&str> {
        let header = &self.table().header;
        let (found, idx) = header.locate(key, None);
        let mut out = Vec::new();
        if !found {
            return out;
        }
        let mut cur = idx;
        while let Some(i) = cur {
            let node = &header.nodes[i];
            if let Some(sk) = &node.subkey {
                out.push(sk.as_str());
            }
            cur = node.next_subkey;
        }
        out
    }

    // ---- patches / tables (`cmsIT8GetPatchName`, `TableCount`, `SetTable`) ----

    /// `cmsIT8GetPatchName` — the `SAMPLE_ID` value of patch `n`.
    pub fn patch_name(&self, n_patch: i32) -> Option<&str> {
        let t = self.table();
        t.get_data(n_patch, t.sample_id)
    }

    /// `cmsIT8GetPatchByName` — the row index of a named patch, or `None`.
    pub fn patch_by_name(&self, patch: &str) -> Option<usize> {
        let i = self.locate_patch(patch);
        (i >= 0).then_some(i as usize)
    }

    /// `cmsIT8TableCount`.
    pub fn table_count(&self) -> u32 {
        self.tables.len() as u32
    }

    /// `cmsIT8SetTable` — select (or append the next) table.
    pub fn set_table(&mut self, n_table: u32) -> Result<u32> {
        let n = n_table as usize;
        if n >= self.tables.len() {
            if n == self.tables.len() {
                self.alloc_table()?;
            } else {
                return Err(self
                    .lx
                    .syn_error(format!("Table {n_table} is out of sequence")));
            }
        }
        self.n_table = n;
        Ok(n_table)
    }

    /// `cmsIT8SetTableByLabel` — follow a `"Label nTable Type"` reference.
    pub fn set_table_by_label(
        &mut self,
        set: &str,
        field: Option<&str>,
        expected_type: Option<&str>,
    ) -> Option<u32> {
        let field = match field {
            Some(f) if !f.is_empty() => f,
            _ => "LABEL",
        };

        let label_fld = self.get_data(set, field)?.to_string();

        // sscanf("%255s %u %255s", ...): exactly three whitespace tokens.
        let mut toks = label_fld.split_whitespace();
        let _label = toks.next()?;
        let n_table: u32 = toks.next()?.parse().ok()?;
        let ty = toks.next()?;
        if toks.next().is_some() {
            // more than three tokens still satisfies sscanf's three conversions,
            // so this is allowed (the trailing text is ignored).
        }

        if let Some(exp) = expected_type {
            if !exp.is_empty() && !ty.eq_ignore_ascii_case(exp) {
                return None;
            }
        }

        self.set_table(n_table).ok()
    }

    /// `cmsIT8SetIndexColumn` — choose which column acts as `SAMPLE_ID`.
    pub fn set_index_column(&mut self, sample: &str) -> Result<()> {
        let pos = self.locate_sample(sample);
        if pos == -1 {
            return Err(Error::new(
                ErrorCode::NotSuitable,
                format!("sample '{sample}' not found"),
            ));
        }
        self.table_mut().sample_id = pos;
        Ok(())
    }

    /// `cmsIT8DefineDblFormat` — set the printf-style double formatter (`None`
    /// restores the default).
    pub fn define_dbl_format(&mut self, formatter: Option<&str>) {
        self.lx.double_formatter = match formatter {
            None => DEFAULT_DBL_FORMAT.to_string(),
            Some(f) => truncate_str(f, MAXID),
        };
    }

    // ---- loading (`cmsIT8LoadFromMem` / `cmsIT8LoadFromFile`) ----

    /// `cmsIT8LoadFromMem` — parse a CGATS/IT8 sheet from a byte buffer.
    pub fn load_from_mem(ctx: &Context, data: &[u8]) -> Result<It8> {
        let type_ = parser::is_my_block(data);
        if type_ == 0 {
            return Err(Error::new(
                ErrorCode::CorruptionDetected,
                "not a CGATS/IT8 stream",
            ));
        }

        let mut it8 = It8::new(ctx);
        it8.lx = Lexer::from_mem("", data);

        let nosheet = (type_ - 1) != 0;
        it8.finish_parse(nosheet)?;
        Ok(it8)
    }

    /// `cmsIT8LoadFromFile` — parse a CGATS/IT8 sheet from a file (with `.include`
    /// resolution relative to the file's directory).
    pub fn load_from_file(ctx: &Context, path: impl AsRef<Path>) -> Result<It8> {
        let path = path.as_ref();
        let name = path.to_string_lossy().into_owned();
        let bytes = std::fs::read(path)
            .map_err(|_| Error::new(ErrorCode::File, format!("File '{name}' not found")))?;

        let type_ = parser::is_my_block(&bytes);
        if type_ == 0 {
            return Err(Error::new(
                ErrorCode::CorruptionDetected,
                "not a CGATS/IT8 stream",
            ));
        }

        let mut it8 = It8::new(ctx);
        it8.lx = Lexer::from_mem(&name, &bytes);

        let nosheet = (type_ - 1) != 0;
        it8.finish_parse(nosheet)?;
        Ok(it8)
    }

    /// Shared tail of the load path: parse, cook pointers, reset to table 0, and
    /// mirror `cmsSignalError` on failure.
    fn finish_parse(&mut self, nosheet: bool) -> Result<()> {
        if let Err(e) = self.parse_it8(nosheet) {
            self.ctx.signal(e.code, &e.message);
            return Err(e);
        }
        self.cook_pointers();
        self.n_table = 0;
        Ok(())
    }

    // ---- saving (`cmsIT8SaveToMem` / `cmsIT8SaveToFile`) ----

    /// `cmsIT8SaveToMem` — serialise every table to a byte buffer.
    pub fn save_to_mem(&mut self) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        for i in 0..self.tables.len() {
            self.n_table = i;
            self.write_header(&mut out);
            self.write_data_format(&mut out);
            self.write_data(&mut out);
        }
        Ok(out)
    }

    /// `cmsIT8SaveToFile` — serialise to a file. Requires every table to carry
    /// both data and a data format, matching the reference's guard.
    pub fn save_to_file(&mut self, path: impl AsRef<Path>) -> Result<()> {
        for t in &self.tables {
            if t.data.is_none() || t.data_format.is_none() {
                return Err(Error::new(
                    ErrorCode::NotSuitable,
                    "table lacks data or data format",
                ));
            }
        }
        let bytes = self.save_to_mem()?;
        std::fs::write(path.as_ref(), bytes)
            .map_err(|e| Error::new(ErrorCode::Write, format!("write error: {e}")))
    }

    /// `WriteHeader` — the sheet type and every header property.
    fn write_header(&mut self, out: &mut Vec<u8>) {
        push_str(out, &self.tables[self.n_table].sheet_type);
        push_str(out, "\n");

        // Collect unknown keywords to register afterwards (mirrors the in-loop
        // `AddAvailableProperty`), avoiding a borrow of `self` during iteration.
        let mut to_register: Vec<String> = Vec::new();

        let node_count = self.tables[self.n_table].header.nodes.len();
        for i in 0..node_count {
            let (keyword, subkey, value, write_as) = {
                let p = &self.tables[self.n_table].header.nodes[i];
                (
                    p.keyword.clone(),
                    p.subkey.clone(),
                    p.value.clone(),
                    p.write_as,
                )
            };

            if keyword.as_bytes().first() == Some(&b'#') {
                push_str(out, "#\n# ");
                if let Some(v) = &value {
                    for ch in v.chars() {
                        push_str(out, &ch.to_string());
                        if ch == '\n' {
                            push_str(out, "# ");
                        }
                    }
                }
                push_str(out, "\n#\n");
                continue;
            }

            if !self.valid_keywords.locate(&keyword, None).0
                && !to_register.iter().any(|k| k.eq_ignore_ascii_case(&keyword))
            {
                to_register.push(keyword.clone());
            }

            push_str(out, &keyword);
            if let Some(v) = &value {
                match write_as {
                    WriteMode::Uncooked => push_str(out, &format!("\t{v}")),
                    WriteMode::Stringify => push_str(out, &format!("\t\"{v}\"")),
                    WriteMode::Hexadecimal => {
                        push_str(out, &format!("\t0x{:X}", satoi(Some(v)) as u32))
                    }
                    WriteMode::Binary => push_str(out, &format!("\t0b{}", satob(Some(v)))),
                    WriteMode::Pair => {
                        let sk = subkey.as_deref().unwrap_or("");
                        push_str(out, &format!("\t\"{sk},{v}\""));
                    }
                }
            }
            push_str(out, "\n");
        }

        for k in to_register {
            let _ = self.add_available_property(&k, WriteMode::Uncooked);
        }
    }

    /// `WriteDataFormat` — the `BEGIN_DATA_FORMAT … END_DATA_FORMAT` block.
    fn write_data_format(&mut self, out: &mut Vec<u8>) {
        let t = &self.tables[self.n_table];
        if t.data_format.is_none() {
            return;
        }

        push_str(out, "BEGIN_DATA_FORMAT\n");
        push_str(out, " ");
        let n_samples = satoi(self.get_property("NUMBER_OF_FIELDS"));
        let t = &self.tables[self.n_table];
        if n_samples <= t.n_samples {
            if let Some(df) = &t.data_format {
                for i in 0..n_samples as usize {
                    let label = df.get(i).and_then(|s| s.as_deref()).unwrap_or(" ");
                    push_str(out, label);
                    push_str(out, if i == (n_samples as usize - 1) { "\n" } else { "\t" });
                }
            }
        }
        push_str(out, "END_DATA_FORMAT\n");
    }

    /// `WriteData` — the `BEGIN_DATA … END_DATA` block, quoting values that
    /// contain spaces.
    fn write_data(&mut self, out: &mut Vec<u8>) {
        if self.tables[self.n_table].data.is_none() {
            return;
        }

        push_str(out, "BEGIN_DATA\n");
        let n_patches = satoi(self.get_property("NUMBER_OF_SETS"));
        let t = &self.tables[self.n_table];
        let n_samples = t.n_samples as usize;

        if n_patches <= t.n_patches {
            if let Some(data) = &t.data {
                for i in 0..n_patches as usize {
                    push_str(out, " ");
                    for j in 0..n_samples {
                        let ptr = data.get(i * n_samples + j).and_then(|s| s.as_deref());
                        match ptr {
                            None => push_str(out, "\"\""),
                            Some(v) if v.contains(' ') => push_str(out, &format!("\"{v}\"")),
                            Some(v) => push_str(out, v),
                        }
                        push_str(out, if j == (n_samples - 1) { "\n" } else { "\t" });
                    }
                }
            }
        }
        push_str(out, "END_DATA\n");
    }

    // ---- `.cube` parsing (`ParseCube`) ----

    /// Parse a `.cube` 3D-LUT file into a [`CubeData`] (`ParseCube` over a file).
    ///
    /// This performs the full lexing/parsing of the `.cube` grammar. Turning the
    /// result into an ICC device-link profile is a separate step; see
    /// [`create_device_link_from_cube_file`].
    pub fn load_cube_from_file(ctx: &Context, path: impl AsRef<Path>) -> Result<CubeData> {
        let path = path.as_ref();
        let name = path.to_string_lossy().into_owned();
        let bytes = std::fs::read(path)
            .map_err(|_| Error::new(ErrorCode::File, format!("File '{name}' not found")))?;
        It8::parse_cube_bytes(ctx, &name, &bytes)
    }

    /// Parse a `.cube` stream held in memory.
    pub fn parse_cube_bytes(ctx: &Context, name: &str, bytes: &[u8]) -> Result<CubeData> {
        let mut cube = It8::new(ctx);
        cube.lx = Lexer::from_mem(name, bytes);
        cube.lx.is_cube = true;
        cube.parse_cube()
    }
}

/// Append a UTF-8 string to the output buffer (`WriteStr`).
fn push_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
}

/// `INTENT_PERCEPTUAL` (lcms2.h) — the rendering intent stamped into the cube device-link.
const INTENT_PERCEPTUAL: u32 = 0;

/// `cmsCreateDeviceLinkFromCubeFileTHR` — parse a `.cube` 3D-LUT and wrap it in an ICC
/// RGB→RGB device-link profile (v4.4).
///
/// Faithful port of the reference builder: parse the file into a shaper (three tabulated
/// tone curves) and/or a 3D CLUT, hang them off a 3→3 [`Pipeline`] (shaper first, CLUT last),
/// and write that pipeline as the `AToB0` tag of a Link-class profile whose device and PCS
/// colour spaces are both RGB. The `TITLE` becomes the profile description (no copyright, since
/// nothing is known about the copyrighted state of the `.cube`).
///
/// The shaper table and CLUT come straight from [`CubeData`]: the shaper is three contiguous
/// per-channel blocks of `shaper_size` floats, and the CLUT is stored **BGR-swapped** exactly
/// as [`cmsStageAllocCLutFloat`](Stage::alloc_clut_float) expects it (the reference performs the
/// R↔B swap during parsing), so both are handed to the stage constructors verbatim.
pub fn create_device_link_from_cube_file_thr(
    context: Context,
    path: impl AsRef<Path>,
) -> Result<Profile> {
    let cube = It8::load_cube_from_file(&context, path)?;

    let mut profile = Profile::create_placeholder(context);
    profile.set_profile_version(4.4);
    profile.set_device_class(class::LINK);
    profile.set_color_space(colorspace::RGB);
    profile.set_pcs(colorspace::RGB);
    profile.set_header_rendering_intent(INTENT_PERCEPTUAL);

    // Pipeline holding shaper + CLUT (`cmsPipelineAlloc(ctx, 3, 3)`).
    let mut pipeline = Pipeline::alloc(3, 3)?;

    // Shaper: three tabulated tone curves, one per per-channel block of the shaper table.
    if let Some(shaper) = &cube.shaper {
        let n = cube.shaper_size as usize;
        let curves = [
            ToneCurve::build_tabulated_float(&shaper[0..n])?,
            ToneCurve::build_tabulated_float(&shaper[n..2 * n])?,
            ToneCurve::build_tabulated_float(&shaper[2 * n..3 * n])?,
        ];
        let stage = Stage::alloc_tone_curves(3, Some(&curves))?;
        if !pipeline.insert_stage(StageLoc::Begin, stage) {
            return Err(Error::new(
                ErrorCode::NotSuitable,
                "cmsCreateDeviceLinkFromCubeFile: shaper stage rejected by pipeline",
            ));
        }
    }

    // CLUT: float 3→3 grid, BGR-swapped storage passed through untouched.
    if let Some(clut) = &cube.clut {
        let stage = Stage::alloc_clut_float(cube.lut_size as u32, 3, 3, Some(clut))?;
        if !pipeline.insert_stage(StageLoc::End, stage) {
            return Err(Error::new(
                ErrorCode::NotSuitable,
                "cmsCreateDeviceLinkFromCubeFile: CLUT stage rejected by pipeline",
            ));
        }
    }

    // Description from TITLE (cmsNoLanguage / cmsNoCountry are the all-zero codes).
    let mut description = Mlu::new();
    description.set_utf8([0, 0], [0, 0], &cube.title)?;

    profile.write_tag(tag_sig::PROFILE_DESCRIPTION, TagValue::Mlu(description))?;
    profile.write_tag(tag_sig::A_TO_B0, TagValue::Pipeline(pipeline))?;

    Ok(profile)
}

/// `cmsCreateDeviceLinkFromCubeFile` — [`create_device_link_from_cube_file_thr`] on a fresh
/// context (C's `NULL` context).
///
/// Callers that only need the raw LUT data should use [`It8::load_cube_from_file`].
pub fn create_device_link_from_cube_file(path: impl AsRef<Path>) -> Result<Profile> {
    create_device_link_from_cube_file_thr(Context::new(), path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHEET: &str = "CGATS.17\n\
ORIGINATOR\t\"lcms-rs\"\n\
NUMBER_OF_FIELDS\t3\n\
NUMBER_OF_SETS\t2\n\
BEGIN_DATA_FORMAT\n\
SAMPLE_ID\tRGB_R\tRGB_G\n\
END_DATA_FORMAT\n\
BEGIN_DATA\n\
1\t10ABC\t0.5\n\
2\t255\t128\n\
END_DATA\n";

    #[test]
    fn round_trip_preserves_values() {
        let ctx = Context::new();
        let it8 = It8::load_from_mem(&ctx, SHEET.as_bytes()).expect("load");

        assert_eq!(it8.sheet_type(), "CGATS.17");
        assert_eq!(it8.get_property("ORIGINATOR"), Some("lcms-rs"));
        assert_eq!(it8.get_property("NUMBER_OF_FIELDS"), Some("3"));
        assert_eq!(it8.get_property("NUMBER_OF_SETS"), Some("2"));

        // Data by patch/sample name.
        assert_eq!(it8.get_data("1", "RGB_R"), Some("10ABC"));
        assert_eq!(it8.get_data("1", "RGB_G"), Some("0.5"));
        assert_eq!(it8.get_data("2", "RGB_R"), Some("255"));
        assert_eq!(it8.get_data("2", "RGB_G"), Some("128"));
        assert!((it8.get_data_dbl("1", "RGB_G") - 0.5).abs() < 1e-12);

        // Save → reload → values preserved.
        let mut it8 = it8;
        let saved = it8.save_to_mem().expect("save");
        let reloaded = It8::load_from_mem(&ctx, &saved).expect("reload");

        assert_eq!(reloaded.sheet_type(), "CGATS.17");
        assert_eq!(reloaded.get_property("ORIGINATOR"), Some("lcms-rs"));
        assert_eq!(reloaded.get_data("1", "RGB_R"), Some("10ABC"));
        assert_eq!(reloaded.get_data("1", "RGB_G"), Some("0.5"));
        assert_eq!(reloaded.get_data("2", "RGB_G"), Some("128"));
    }

    #[test]
    fn number_becomes_identifier_quirk() {
        // A data value of `10ABC` (digits immediately followed by identifier
        // characters) must be tokenised as the identifier "10ABC", not the
        // integer 10.
        let ctx = Context::new();
        let it8 = It8::load_from_mem(&ctx, SHEET.as_bytes()).expect("load");
        assert_eq!(it8.get_data("1", "RGB_R"), Some("10ABC"));
    }

    #[test]
    fn locale_free_float_parse() {
        use super::lexer::parse_float_number as pf;
        assert!((pf(Some("3.5")) - 3.5).abs() < 1e-12);
        assert!((pf(Some("-0.25")) + 0.25).abs() < 1e-12);
        assert!((pf(Some("1.5e2")) - 150.0).abs() < 1e-9);
        assert!((pf(Some("2E-3")) - 0.002).abs() < 1e-12);
        // A comma is never a decimal separator: "3,5" parses as 3.0 (parsing
        // stops at ',').
        assert!((pf(Some("3,5")) - 3.0).abs() < 1e-12);
        assert_eq!(pf(None), 0.0);
    }

    #[test]
    fn g_formatter_matches_c() {
        use super::lexer::fmt_g;
        assert_eq!(fmt_g(1.0, 10), "1");
        assert_eq!(fmt_g(0.5, 10), "0.5");
        assert_eq!(fmt_g(100.0, 10), "100");
        assert_eq!(fmt_g(0.1, 10), "0.1");
        assert_eq!(fmt_g(1e-5, 10), "1e-05");
        assert_eq!(fmt_g(123456789012.0, 10), "1.23456789e+11");
    }

    #[test]
    fn cube_lut_parse_bgr_order() {
        let cube = "TITLE \"test\"\n\
LUT_3D_SIZE 2\n\
0.0 0.0 0.0\n\
1.0 0.0 0.0\n\
0.0 1.0 0.0\n\
1.0 1.0 0.0\n\
0.0 0.0 1.0\n\
1.0 0.0 1.0\n\
0.0 1.0 1.0\n\
1.0 1.0 1.0\n";

        let ctx = Context::new();
        let data = It8::parse_cube_bytes(&ctx, "test.cube", cube.as_bytes()).expect("cube");

        assert_eq!(data.title, "test");
        assert_eq!(data.lut_size, 2);
        assert_eq!(data.shaper_size, 0);
        assert_eq!(data.domain_min, [0.0, 0.0, 0.0]);
        assert_eq!(data.domain_max, [1.0, 1.0, 1.0]);

        let clut = data.clut.expect("clut present");
        assert_eq!(clut.len(), 8 * 3);

        // Node 0 = input (0,0,0) → all zero.
        assert_eq!(&clut[0..3], &[0.0, 0.0, 0.0]);

        // Node 1 = input R=1,G=0,B=0. Stored BGR-swapped: [+0]=B, [+1]=G, [+2]=R.
        assert_eq!(clut[3], 0.0); // B
        assert_eq!(clut[4], 0.0); // G
        assert_eq!(clut[5], 1.0); // R

        // Node 4 = input R=0,G=0,B=1 → [+0]=B=1.
        assert_eq!(clut[12], 1.0); // B
        assert_eq!(clut[13], 0.0); // G
        assert_eq!(clut[14], 0.0); // R
    }

    #[test]
    fn programmatic_build_and_save() {
        // Build a sheet through the setter API and confirm it serialises.
        let ctx = Context::new();
        let mut it8 = It8::new(&ctx);
        it8.set_property_str("ORIGINATOR", "unit-test").unwrap();
        it8.set_property_uncooked("NUMBER_OF_FIELDS", "2").unwrap();
        it8.set_property_uncooked("NUMBER_OF_SETS", "1").unwrap();
        it8.set_data_format(0, "SAMPLE_ID").unwrap();
        it8.set_data_format(1, "RGB_R").unwrap();
        it8.set_data_row_col(0, 0, "patch1").unwrap();
        it8.set_data_row_col(0, 1, "42").unwrap();

        let saved = it8.save_to_mem().expect("save");
        let text = String::from_utf8(saved).unwrap();
        assert!(text.contains("ORIGINATOR\t\"unit-test\""));
        assert!(text.contains("BEGIN_DATA_FORMAT"));
        assert!(text.contains("patch1"));
    }

    #[test]
    fn multi_pair_property_roundtrip() {
        // WRITE_PAIR dictionary property (WEIGHTING_FUNCTION) parsed from a sheet.
        let sheet = "CGATS.17\n\
WEIGHTING_FUNCTION\t\"name, D50; value, 1.0\"\n\
NUMBER_OF_FIELDS\t1\n\
NUMBER_OF_SETS\t1\n\
BEGIN_DATA_FORMAT\n\
SAMPLE_ID\n\
END_DATA_FORMAT\n\
BEGIN_DATA\n\
1\n\
END_DATA\n";
        let ctx = Context::new();
        let it8 = It8::load_from_mem(&ctx, sheet.as_bytes()).expect("load");
        assert_eq!(it8.get_property_multi("WEIGHTING_FUNCTION", "name"), Some("D50"));
        assert_eq!(it8.get_property_multi("WEIGHTING_FUNCTION", "value"), Some("1.0"));
        let subkeys = it8.enum_property_multi("WEIGHTING_FUNCTION");
        assert!(subkeys.contains(&"name"));
        assert!(subkeys.contains(&"value"));
    }

    #[test]
    fn cube_to_device_link_profile() {
        // A .cube with both a shaper (LUT_1D_SIZE) and a 3D CLUT (LUT_3D_SIZE): the
        // parser reads the two shaper rows first, then the eight CLUT nodes.
        let cube = "TITLE \"cube link\"\n\
LUT_1D_SIZE 2\n\
LUT_3D_SIZE 2\n\
0.0 0.0 0.0\n\
1.0 1.0 1.0\n\
0.0 0.0 0.0\n\
1.0 0.0 0.0\n\
0.0 1.0 0.0\n\
1.0 1.0 0.0\n\
0.0 0.0 1.0\n\
1.0 0.0 1.0\n\
0.0 1.0 1.0\n\
1.0 1.0 1.0\n";

        // Write to a uniquely-named temp file so the public file-based entry point is
        // exercised end to end.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("lcms_rs_cube_{nanos}.cube"));
        std::fs::write(&path, cube).expect("write temp .cube");

        let build = create_device_link_from_cube_file(&path);
        std::fs::remove_file(&path).ok();
        let mut profile = build.expect("device link built");

        // Header: Link / RGB / RGB, v4.4, perceptual intent.
        assert_eq!(profile.device_class(), class::LINK);
        assert_eq!(profile.color_space(), colorspace::RGB);
        assert_eq!(profile.pcs(), colorspace::RGB);

        // AToB0 holds the live 3→3 pipeline (shaper curves + CLUT). This is exactly how
        // lcms consumes a cube device-link: the profile is used live as a transform input
        // (see `utils/tificc`), the pipeline read straight back from the tag dictionary.
        //
        // Note: this pipeline shape — [CurveSet, CLUT] with no trailing B-curves — is NOT a
        // valid `LutAToBType` layout (which requires one of B / M·Matrix·B / A·CLUT·B /
        // A·CLUT·M·Matrix·B), so serialising the v4.4 profile to disk is unsupported in lcms
        // itself. We therefore verify the live tag, matching the reference's actual usage.
        match profile.read_tag(tag_sig::A_TO_B0).expect("AToB0 present") {
            TagValue::Pipeline(p) => {
                assert_eq!(p.input_channels(), 3);
                assert_eq!(p.output_channels(), 3);
                // Shaper stage + CLUT stage.
                assert_eq!(p.stage_count(), 2);
                // The pipeline evaluates: identity shaper + corner-exact CLUT maps white→white.
                let mut out = [0f32; 3];
                p.eval_float(&[1.0, 1.0, 1.0], &mut out);
                for v in out {
                    assert!((v - 1.0).abs() < 1e-4, "white should map to white, got {out:?}");
                }
            }
            other => panic!("expected Pipeline for AToB0, got {other:?}"),
        }

        // Description carries the TITLE.
        match profile.read_tag(tag_sig::PROFILE_DESCRIPTION).expect("desc present") {
            TagValue::Mlu(m) => {
                assert_eq!(m.get_utf8([0, 0], [0, 0]).as_deref(), Some("cube link"));
            }
            other => panic!("expected Mlu for description, got {other:?}"),
        }
    }
}
