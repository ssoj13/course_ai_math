//! Port target: `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/codestream/ojph_params_local.h`

use crate::openjph::ojph_arch::{population_count, swap_byte_u16, swap_byte_u32, swap_byte_u64};
use crate::openjph::ojph_base::{Point, Size};
use crate::openjph::ojph_defs::{Si8, Si16, Si32, Ui8, Ui16, Ui32, Ui64, ojph_div_ceil, ojph_min};
use crate::openjph::ojph_file::{InfileBase, OutfileBase};
use crate::openjph::ojph_params::{ALL_COMPS, Nonlinearity};
use std::alloc::{Layout, alloc_zeroed, dealloc, handle_alloc_error};
use std::f64::consts::LN_2;
use std::ptr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum ProgressionOrder {
    OjphPoLrcp = 0,
    OjphPoRlcp = 1,
    OjphPoRpcl = 2,
    OjphPoPcrl = 3,
    OjphPoCprl = 4,
    /// Position - resolution - component - layer (ISO/IEC 15444-2 Table A.49). Decode only: the
    /// encoder never emits it, and it is not a valid POC entry order.
    OjphPoPrcl = 5,
}

/// One progression-order-change entry (ITU-T T.800 §B.12.1). Defines a 3-D volume of packets —
/// layers `[0, lye)`, resolutions `[rs, re)`, components `[cs, ce)` — emitted in progression order
/// `po` (0=LRCP, 1=RLCP, 2=RPCL, 3=PCRL, 4=CPRL). The layer range conceptually starts at 0; the
/// "emit once" rule (a packet belongs to the first entry that contains it) is what lets a later
/// entry with a larger `lye` contribute only its new layers (see the tile packet iterator).
#[derive(Clone, Copy, Debug, Default)]
pub struct PocEntry {
    /// First resolution level (inclusive), `RSpoc`.
    pub rs: Ui32,
    /// Last resolution level (exclusive), `REpoc`.
    pub re: Ui32,
    /// First component (inclusive), `CSpoc`.
    pub cs: Ui32,
    /// Last component (exclusive), `CEpoc`.
    pub ce: Ui32,
    /// Last quality layer (exclusive), `LYEpoc`.
    pub lye: Ui16,
    /// Progression order for this volume, `Ppoc`.
    pub po: Ui8,
}

/// POC marker segment (`0xFF5F`, ITU-T T.800 §B.12). OpenJPH does not implement POC (it skips the
/// marker); this is a native implementation. Holds the ordered list of progression-change entries.
/// A main-header POC applies to every tile; an in-tile (tile-part header) POC overrides it for that
/// tile.
#[derive(Clone, Debug, Default)]
pub struct ParamPoc {
    entries: Vec<PocEntry>,
}

impl ParamPoc {
    /// Clear all entries (decoder reuse / `restart`).
    pub fn restart(&mut self) {
        self.entries.clear();
    }

    /// Whether any progression-change entry was parsed.
    pub fn exists(&self) -> bool {
        !self.entries.is_empty()
    }

    /// The parsed progression-change entries, in marker order.
    pub fn entries(&self) -> &[PocEntry] {
        &self.entries
    }

    /// Hard/soft failure while reading a POC field: `-1` when `resilient`, otherwise a loud error
    /// (never silently continue with a half-read entry).
    fn fail(resilient: bool) -> crate::openjph::ojph_message::OjphResult<i32> {
        if resilient {
            return Ok(-1);
        }
        Err(crate::ojph_error!(0x000300BA, "error reading POC marker segment"))
    }

    /// Parse a POC marker segment. The file cursor must sit right after the `FF5F` marker code (as
    /// left by `find_marker`). `csiz` is the component count: for `csiz < 257` the `CSpoc`/`CEpoc`
    /// fields are one byte each (7-byte entries), otherwise two bytes (9-byte entries). Entries are
    /// appended to any already present (a tile may carry several POC segments). Returns 0 on
    /// success, or -1 on a truncated read when `resilient` (matching `skip_marker`'s contract).
    pub fn read(
        &mut self,
        file: &mut dyn InfileBase,
        csiz: Ui32,
        resilient: bool,
    ) -> crate::openjph::ojph_message::OjphResult<i32> {
        let mut lpoc: Ui16 = 0;
        if !read_u16(file, &mut lpoc) {
            return Self::fail(resilient);
        }
        let lpoc = swap_byte_u16(lpoc) as Ui32;
        let wide = csiz >= 257;
        let entry_size: Ui32 = if wide { 9 } else { 7 };
        if lpoc < 2 + entry_size {
            return Self::fail(resilient);
        }
        let npoc = (lpoc - 2) / entry_size;
        for _ in 0..npoc {
            let mut e = PocEntry::default();
            let mut b: Ui8 = 0;
            let mut w: Ui16 = 0;
            if !read_u8(file, &mut b) {
                return Self::fail(resilient);
            }
            e.rs = b as Ui32;
            if wide {
                if !read_u16(file, &mut w) {
                    return Self::fail(resilient);
                }
                e.cs = swap_byte_u16(w) as Ui32;
            } else {
                if !read_u8(file, &mut b) {
                    return Self::fail(resilient);
                }
                e.cs = b as Ui32;
            }
            if !read_u16(file, &mut w) {
                return Self::fail(resilient);
            }
            e.lye = swap_byte_u16(w);
            if !read_u8(file, &mut b) {
                return Self::fail(resilient);
            }
            e.re = b as Ui32;
            if wide {
                if !read_u16(file, &mut w) {
                    return Self::fail(resilient);
                }
                e.ce = swap_byte_u16(w) as Ui32;
            } else {
                if !read_u8(file, &mut b) {
                    return Self::fail(resilient);
                }
                e.ce = b as Ui32;
            }
            if !read_u8(file, &mut b) {
                return Self::fail(resilient);
            }
            e.po = b;
            if e.po > ProgressionOrder::OjphPoCprl as Ui8 {
                return Err(crate::ojph_error!(
                    0x000300B9,
                    "unsupported POC progression order {} (only 0..=4 are supported)",
                    e.po
                ));
            }
            self.entries.push(e);
        }
        Ok(0)
    }
}

/// Parse an RGN marker segment (`0xFF5E`, ITU-T T.800 §A.6.3 / max-shift ROI §H.1). The cursor must
/// sit right after the `FF5E` marker code (as left by `find_marker`). `csiz` is the component
/// count: the `Crgn` field is one byte for `csiz < 257`, two bytes otherwise. Only the implicit
/// max-shift ROI method (`Srgn == 0`) is defined by Part 1, so any other style is rejected loudly
/// rather than silently mis-decoded. On success the affected component index is written to
/// `out_comp`, the ROI shift to `out_shift`, and 0 is returned; on a truncated read it returns -1
/// when `resilient` (matching `skip_marker`'s contract), otherwise errors loudly.
pub fn read_rgn(
    file: &mut dyn InfileBase,
    csiz: Ui32,
    resilient: bool,
    out_comp: &mut Ui32,
    out_shift: &mut Ui8,
) -> crate::openjph::ojph_message::OjphResult<i32> {
    let fail = |resilient: bool| -> crate::openjph::ojph_message::OjphResult<i32> {
        if resilient {
            return Ok(-1);
        }
        Err(crate::ojph_error!(0x000300BE, "error reading RGN marker segment"))
    };
    let mut lrgn: Ui16 = 0;
    if !read_u16(file, &mut lrgn) {
        return fail(resilient);
    }
    let lrgn = swap_byte_u16(lrgn) as Ui32;
    let wide = csiz >= 257;
    // Lrgn = 2 (its own bytes) + Crgn (1|2) + Srgn (1) + SPrgn (1).
    let expected = if wide { 6 } else { 5 };
    if lrgn != expected {
        return fail(resilient);
    }
    let comp: Ui32 = if wide {
        let mut w: Ui16 = 0;
        if !read_u16(file, &mut w) {
            return fail(resilient);
        }
        swap_byte_u16(w) as Ui32
    } else {
        let mut b: Ui8 = 0;
        if !read_u8(file, &mut b) {
            return fail(resilient);
        }
        b as Ui32
    };
    let mut srgn: Ui8 = 0;
    if !read_u8(file, &mut srgn) {
        return fail(resilient);
    }
    if srgn != 0 {
        return Err(crate::ojph_error!(
            0x000300BF,
            "unsupported RGN ROI style Srgn = {} (only the implicit max-shift method, Srgn = 0, is \
             defined by JPEG 2000 Part 1)",
            srgn
        ));
    }
    let mut sprgn: Ui8 = 0;
    if !read_u8(file, &mut sprgn) {
        return fail(resilient);
    }
    if comp >= csiz {
        return Err(crate::ojph_error!(
            0x000300C0,
            "RGN marker segment references component {}, but the codestream has only {} components",
            comp,
            csiz
        ));
    }
    // Bound the ROI shift: it is applied as `mag << SPrgn` on the (≤32-bit) sign-magnitude
    // code-block buffer, so a hostile/garbage `SPrgn >= 32` would overflow the shift (UB / debug
    // panic) and push background coefficients into the sign bit. Valid max-shift streams keep
    // `SPrgn <= k_max < 31`, so reject anything larger loudly rather than risk corruption.
    if sprgn >= 32 {
        return Err(crate::ojph_error!(
            0x000300C1,
            "RGN ROI shift SPrgn = {} is out of the supported range (must be < 32)",
            sprgn
        ));
    }
    *out_comp = comp;
    *out_shift = sprgn;
    Ok(0)
}

pub const OJPH_PO_STRING_LRCP: &str = "LRCP";
pub const OJPH_PO_STRING_RLCP: &str = "RLCP";
pub const OJPH_PO_STRING_RPCL: &str = "RPCL";
pub const OJPH_PO_STRING_PCRL: &str = "PCRL";
pub const OJPH_PO_STRING_CPRL: &str = "CPRL";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum OjphProfileNum {
    OjphPnUndefined = 0,
    OjphPnProfile0 = 1,
    OjphPnProfile1 = 2,
    OjphPnCinema2k = 3,
    OjphPnCinema4k = 4,
    OjphPnCinemas2k = 5,
    OjphPnCinemas4k = 6,
    OjphPnBroadcast = 7,
    OjphPnImf = 8,
}

pub const OJPH_PN_STRING_PROFILE0: &str = "PROFILE0";
pub const OJPH_PN_STRING_PROFILE1: &str = "PROFILE1";
pub const OJPH_PN_STRING_CINEMA2K: &str = "CINEMA2K";
pub const OJPH_PN_STRING_CINEMA4K: &str = "CINEMA4K";
pub const OJPH_PN_STRING_CINEMAS2K: &str = "CINEMAS2K";
pub const OJPH_PN_STRING_CINEMAS4K: &str = "CINEMAS4K";
pub const OJPH_PN_STRING_BROADCAST: &str = "BROADCAST";
pub const OJPH_PN_STRING_IMF: &str = "IMF";

pub const OJPH_TILEPART_NO_DIVISIONS: Ui32 = 0x0;
pub const OJPH_TILEPART_RESOLUTIONS: Ui32 = 0x1;
pub const OJPH_TILEPART_COMPONENTS: Ui32 = 0x2;
pub const OJPH_TILEPART_LAYERS: Ui32 = 0x4;
pub const OJPH_TILEPART_MASK: Ui32 = 0x3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Jp2kMarker {
    Soc = 0xFF4F,
    Cap = 0xFF50,
    Siz = 0xFF51,
    Cod = 0xFF52,
    Coc = 0xFF53,
    Tlm = 0xFF55,
    Prf = 0xFF56,
    Plm = 0xFF57,
    Plt = 0xFF58,
    Cpf = 0xFF59,
    Qcd = 0xFF5C,
    Qcc = 0xFF5D,
    Rgn = 0xFF5E,
    Poc = 0xFF5F,
    Ppm = 0xFF60,
    Ppt = 0xFF61,
    Crg = 0xFF63,
    Com = 0xFF64,
    Dfs = 0xFF72,
    Ads = 0xFF73,
    Nlt = 0xFF76,
    Atk = 0xFF79,
    Sot = 0xFF90,
    Sop = 0xFF91,
    Eph = 0xFF92,
    Sod = 0xFF93,
    Eoc = 0xFFD9,
}

impl Jp2kMarker {
    pub const fn from_be_word(word: Ui16) -> Option<Self> {
        match word {
            0xFF4F => Some(Self::Soc),
            0xFF50 => Some(Self::Cap),
            0xFF51 => Some(Self::Siz),
            0xFF52 => Some(Self::Cod),
            0xFF53 => Some(Self::Coc),
            0xFF55 => Some(Self::Tlm),
            0xFF56 => Some(Self::Prf),
            0xFF57 => Some(Self::Plm),
            0xFF58 => Some(Self::Plt),
            0xFF59 => Some(Self::Cpf),
            0xFF5C => Some(Self::Qcd),
            0xFF5D => Some(Self::Qcc),
            0xFF5E => Some(Self::Rgn),
            0xFF5F => Some(Self::Poc),
            0xFF60 => Some(Self::Ppm),
            0xFF61 => Some(Self::Ppt),
            0xFF63 => Some(Self::Crg),
            0xFF64 => Some(Self::Com),
            0xFF72 => Some(Self::Dfs),
            0xFF73 => Some(Self::Ads),
            0xFF76 => Some(Self::Nlt),
            0xFF79 => Some(Self::Atk),
            0xFF90 => Some(Self::Sot),
            0xFF91 => Some(Self::Sop),
            0xFF92 => Some(Self::Eph),
            0xFF93 => Some(Self::Sod),
            0xFFD9 => Some(Self::Eoc),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Soc => "SOC",
            Self::Cap => "CAP",
            Self::Siz => "SIZ",
            Self::Cod => "COD",
            Self::Coc => "COC",
            Self::Tlm => "TLM",
            Self::Prf => "PRF",
            Self::Plm => "PLM",
            Self::Plt => "PLT",
            Self::Cpf => "CPF",
            Self::Qcd => "QCD",
            Self::Qcc => "QCC",
            Self::Rgn => "RGN",
            Self::Poc => "POC",
            Self::Ppm => "PPM",
            Self::Ppt => "PPT",
            Self::Crg => "CRG",
            Self::Com => "COM",
            Self::Dfs => "DFS",
            Self::Ads => "ADS",
            Self::Nlt => "NLT",
            Self::Atk => "ATK",
            Self::Sot => "SOT",
            Self::Sop => "SOP",
            Self::Eph => "EPH",
            Self::Sod => "SOD",
            Self::Eoc => "EOC",
        }
    }
}

#[allow(non_snake_case)]
#[derive(Clone, Copy, Debug)]
pub struct ParamCap {
    Lcap: Ui16,
    Pcap: Ui32,
    Ccap: [Ui16; 32],
}

impl Default for ParamCap {
    fn default() -> Self {
        let mut value = Self {
            Lcap: 0,
            Pcap: 0,
            Ccap: [0; 32],
        };
        value.Lcap = 8;
        value.Pcap = 0x0002_0000;
        value
    }
}

impl ParamCap {
    /// `Pcap` bit for ITU-T T.814 (Part 15), the 15th MSB.
    pub const PCAP_PART15: Ui32 = 0x0002_0000;
    /// `Pcap` bit for ITU-T T.801 (Part 2), the 2nd MSB.
    pub const PCAP_PART2: Ui32 = 0x4000_0000;

    /// `Ccap^part` (T.800 A.5.2): the capability words are stored in increasing part order, one per
    /// set `Pcap` bit, i.e. part `i` is bit `32 - i` and its word index is the number of set bits
    /// above it. `None` when the part is not signalled. (Indexing `Ccap[0]` blindly is only right
    /// while Part 15 is the sole part present.)
    pub fn get_ccap_part(&self, part: u32) -> Option<Ui16> {
        let bit = 32 - part;
        if (self.Pcap >> bit) & 1 == 0 {
            return None;
        }
        let idx = population_count(self.Pcap >> bit >> 1) as usize;
        Some(self.Ccap[idx])
    }

    pub fn check_validity(&mut self, cod: &ParamCod, qcd: &ParamQcd) {
        if cod.get_wavelet_kern() == ParamCod::DWT_REV53 {
            self.Ccap[0] &= 0xFFDF;
        } else {
            self.Ccap[0] |= 0x0020;
        }
        self.Ccap[0] &= 0xFFE0;
        let b = qcd.get_MAGB();
        let bp = if b <= 8 {
            0
        } else if b < 28 {
            b - 8
        } else {
            13 + (b >> 2)
        };
        self.Ccap[0] |= bp as Ui16;
    }

    pub fn write(&mut self, file: &mut dyn OutfileBase) -> crate::openjph::ojph_message::OjphResult<bool> {
        self.Lcap = 8;

        let mut result = true;
        result &= write_u16(file, swap_byte_u16(Jp2kMarker::Cap as Ui16))?;
        result &= write_u16(file, swap_byte_u16(self.Lcap))?;
        result &= write_u32(file, swap_byte_u32(self.Pcap))?;
        result &= write_u16(file, swap_byte_u16(self.Ccap[0]))?;
        Ok(result)
    }

    pub fn read(&mut self, file: &mut dyn InfileBase) -> crate::openjph::ojph_message::OjphResult<()> {
        if !read_u16(file, &mut self.Lcap) {
            return Err(crate::ojph_error!(0x00050061, "error reading CAP marker"));
        }
        self.Lcap = swap_byte_u16(self.Lcap);
        if !read_u32(file, &mut self.Pcap) {
            return Err(crate::ojph_error!(0x00050062, "error reading CAP marker"));
        }
        self.Pcap = swap_byte_u32(self.Pcap);
        let count = population_count(self.Pcap);
        // Part 15 (bit 17) is required; Part 2 (bit 30) extensions are accepted: their markers
        // (ATK, DFS, NLT, ...) are parsed on their own and do not need the Ccap^2 word.
        if (self.Pcap & !(Self::PCAP_PART15 | Self::PCAP_PART2)) != 0 {
            return Err(crate::ojph_error!(
                0x00050063,
                "error Pcap in CAP has options that are not supported"
            ));
        }
        if (self.Pcap & 0x0002_0000) == 0 {
            return Err(crate::ojph_error!(
                0x00050064,
                "error Pcap should have its 15th MSB set, Pcap^15.  This is not a JPH file"
            ));
        }
        for i in 0..count as usize {
            if !read_u16(file, &mut self.Ccap[i]) {
                return Err(crate::ojph_error!(0x00050065, "error reading CAP marker"));
            }
            // Big-endian on the wire (like Pcap above and like `write`); without the swap the HT
            // MIXED bits 14-15 of Ccap^15 were read as the low byte and never seen.
            self.Ccap[i] = swap_byte_u16(self.Ccap[i]);
        }
        if self.Lcap != 6 + 2 * count as Ui16 {
            return Err(crate::ojph_error!(0x00050066, "error in CAP marker length"));
        }
        Ok(())
    }

    #[inline]
    pub const fn get_pcap(&self) -> Ui32 {
        self.Pcap
    }

    #[inline]
    pub const fn get_ccap(&self, index: usize) -> Ui16 {
        self.Ccap[index]
    }
}

#[allow(non_snake_case)]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SizCompInfo {
    pub SSiz: Ui8,
    pub XRsiz: Ui8,
    pub YRsiz: Ui8,
}

#[allow(non_snake_case)]
#[derive(Clone, Copy, Debug)]
pub struct CodSpcod {
    pub num_decomp: Ui8,
    pub block_width: Ui8,
    pub block_height: Ui8,
    pub block_style: Ui8,
    pub wavelet_trans: Ui8,
    pub precinct_size: [Ui8; 33],
}

impl Default for CodSpcod {
    fn default() -> Self {
        Self {
            num_decomp: 5,
            block_width: 4,
            block_height: 4,
            block_style: 0x40,
            wavelet_trans: 0,
            precinct_size: [0; 33],
        }
    }
}

impl CodSpcod {
    pub fn get_log_block_dims(&self) -> Size {
        Size::new(self.block_width as Ui32 + 2, self.block_height as Ui32 + 2)
    }

    pub fn get_block_dims(&self) -> Size {
        let t = self.get_log_block_dims();
        Size::new(1u32 << t.w, 1u32 << t.h)
    }

    pub fn get_log_precinct_size(&self, res_num: Ui32) -> Size {
        debug_assert!(res_num <= self.num_decomp as Ui32);
        let v = self.precinct_size[res_num as usize];
        Size::new((v & 0xF) as Ui32, (v >> 4) as Ui32)
    }
}

#[allow(non_snake_case)]
#[derive(Clone, Copy, Debug)]
pub struct CodSGcod {
    pub prog_order: Ui8,
    pub num_layers: Ui16,
    pub mc_trans: Ui8,
}

impl Default for CodSGcod {
    fn default() -> Self {
        Self {
            prog_order: ProgressionOrder::OjphPoRpcl as Ui8,
            num_layers: 1,
            mc_trans: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CodType {
    Undefined = 0,
    CodMain = 1,
    CocMain = 2,
    CodTile = 3,
    CocTile = 4,
}

#[allow(non_snake_case)]
pub struct ParamSiz {
    Lsiz: Ui16,
    Rsiz: Ui16,
    Xsiz: Ui32,
    Ysiz: Ui32,
    XOsiz: Ui32,
    YOsiz: Ui32,
    XTsiz: Ui32,
    YTsiz: Ui32,
    XTOsiz: Ui32,
    YTOsiz: Ui32,
    Csiz: Ui16,
    /// Per-component `SSiz/XRsiz/YRsiz`; always exactly `Csiz` entries (owned, so moving the
    /// struct or calling `set_num_components` repeatedly can never leave a dangling pointer).
    comps: Vec<SizCompInfo>,
    skipped_resolutions: Ui32,
    ws_kern_support_needed: bool,
    dfs_support_needed: bool,
    cod: *const ParamCod,
    dfs: *const ParamDfs,
}

impl Default for ParamSiz {
    fn default() -> Self {
        let mut value = Self {
            Lsiz: 0,
            Rsiz: 0,
            Xsiz: 0,
            Ysiz: 0,
            XOsiz: 0,
            YOsiz: 0,
            XTsiz: 0,
            YTsiz: 0,
            XTOsiz: 0,
            YTOsiz: 0,
            Csiz: 0,
            comps: Vec::new(),
            skipped_resolutions: 0,
            ws_kern_support_needed: false,
            dfs_support_needed: false,
            cod: ptr::null(),
            dfs: ptr::null(),
        };
        value.init();
        value
    }
}

impl ParamSiz {
    /// Create without calling init(); `cod`/`dfs` are wired up later by the owning codestream.
    pub fn new_uninit() -> Self {
        Self {
            Lsiz: 0,
            Rsiz: 0,
            Xsiz: 0,
            Ysiz: 0,
            XOsiz: 0,
            YOsiz: 0,
            XTsiz: 0,
            YTsiz: 0,
            XTOsiz: 0,
            YTOsiz: 0,
            Csiz: 0,
            comps: Vec::new(),
            skipped_resolutions: 0,
            ws_kern_support_needed: false,
            dfs_support_needed: false,
            cod: ptr::null(),
            dfs: ptr::null(),
        }
    }

    pub const RSIZ_NLT_FLAG: Ui16 = 0x0200;
    pub const RSIZ_HT_FLAG: Ui16 = 0x4000;
    pub const RSIZ_EXT_FLAG: Ui16 = 0x8000;

    pub fn init(&mut self) {
        self.Lsiz = 0;
        self.Csiz = 0;
        self.Xsiz = 0;
        self.Ysiz = 0;
        self.XOsiz = 0;
        self.YOsiz = 0;
        self.XTsiz = 0;
        self.YTsiz = 0;
        self.XTOsiz = 0;
        self.YTOsiz = 0;
        self.skipped_resolutions = 0;
        self.comps.clear();
        self.ws_kern_support_needed = false;
        self.dfs_support_needed = false;
        self.cod = ptr::null();
        self.dfs = ptr::null();
        self.Rsiz = Self::RSIZ_HT_FLAG;
    }

    /// Component `comp_num`'s `SSiz/XRsiz/YRsiz` (indexing is bounds-checked: a component number
    /// at or beyond `Csiz` is a caller bug, not something to read out of bounds).
    #[inline]
    fn comp(&self, comp_num: Ui32) -> &SizCompInfo {
        &self.comps[comp_num as usize]
    }

    /// Release the per-component storage (`Vec` also does this on drop).
    pub fn destroy(&mut self) {
        self.comps = Vec::new();
    }

    /// Resize to `num_comps` zeroed components. Safe to call repeatedly with any count.
    pub fn set_num_components(&mut self, num_comps: Ui32) {
        self.Csiz = num_comps as Ui16;
        self.comps.clear();
        self.comps.resize(self.Csiz as usize, SizCompInfo::default());
    }

    pub fn set_comp_info(
        &mut self,
        comp_num: Ui32,
        downsampling: Point,
        bit_depth: Ui32,
        is_signed: bool,
    ) {
        debug_assert!(comp_num < self.Csiz as Ui32);
        debug_assert!(downsampling.x != 0 && downsampling.y != 0);
        debug_assert!((1..=38).contains(&bit_depth), "SIZ bit depth must be 1..=38, got {bit_depth}");
        let c = &mut self.comps[comp_num as usize];
        c.SSiz = (bit_depth.saturating_sub(1) + if is_signed { 0x80 } else { 0 }) as Ui8;
        c.XRsiz = downsampling.x as Ui8;
        c.YRsiz = downsampling.y as Ui8;
    }

    #[inline]
    pub fn set_image_extent(&mut self, dims: Point) {
        self.Xsiz = dims.x;
        self.Ysiz = dims.y;
    }
    #[inline]
    pub const fn get_image_extent(&self) -> Point {
        Point::new(self.Xsiz, self.Ysiz)
    }
    #[inline]
    pub fn set_tile_size(&mut self, s: Size) {
        self.XTsiz = s.w;
        self.YTsiz = s.h;
    }
    #[inline]
    pub const fn get_tile_size(&self) -> Size {
        Size::new(self.XTsiz, self.YTsiz)
    }
    #[inline]
    pub fn set_image_offset(&mut self, offset: Point) {
        self.XOsiz = offset.x;
        self.YOsiz = offset.y;
    }
    #[inline]
    pub const fn get_image_offset(&self) -> Point {
        Point::new(self.XOsiz, self.YOsiz)
    }
    #[inline]
    pub fn set_tile_offset(&mut self, offset: Point) {
        self.XTOsiz = offset.x;
        self.YTOsiz = offset.y;
    }
    #[inline]
    pub const fn get_tile_offset(&self) -> Point {
        Point::new(self.XTOsiz, self.YTOsiz)
    }
    #[inline]
    pub fn set_cod(&mut self, cod: &ParamCod) {
        self.cod = cod;
    }
    #[inline]
    pub fn link_cod(&mut self, cod: *const ParamCod) {
        self.cod = cod;
    }
    #[inline]
    pub fn link_dfs(&mut self, dfs: *const ParamDfs) {
        self.dfs = dfs;
    }
    #[inline]
    pub fn set_skipped_resolutions(&mut self, skipped_resolutions: Ui32) {
        self.skipped_resolutions = skipped_resolutions;
    }

    pub fn check_validity(&self) -> crate::openjph::ojph_message::OjphResult<()> {
        if self.Xsiz == 0 || self.Ysiz == 0 || self.XTsiz == 0 || self.YTsiz == 0 {
            return Err(crate::ojph_error!(0x00040001, "Image extent and/or tile size cannot be zero"));
        }
        if self.XTOsiz > self.XOsiz || self.YTOsiz > self.YOsiz {
            return Err(crate::ojph_error!(
                0x00040002,
                "Tile offset has to be smaller than the image offset"
            ));
        }
        // Widened: tile size + offset straight from the file can exceed u32.
        if u64::from(self.XTsiz) + u64::from(self.XTOsiz) <= u64::from(self.XOsiz)
            || u64::from(self.YTsiz) + u64::from(self.YTOsiz) <= u64::from(self.YOsiz)
        {
            return Err(crate::ojph_error!(
                0x00040003,
                "The top left tile must intersect with the image"
            ));
        }
        if self.Xsiz <= self.XOsiz || self.Ysiz <= self.YOsiz {
            return Err(crate::ojph_error!(
                0x00040004,
                "The image extent must be larger than the image offset"
            ));
        }
        Ok(())
    }

    #[inline]
    pub const fn get_num_components(&self) -> Ui16 {
        self.Csiz
    }
    pub fn get_bit_depth(&self, comp_num: Ui32) -> Ui32 {
        (self.comp(comp_num).SSiz & 0x7F) as Ui32 + 1
    }
    pub fn is_signed(&self, comp_num: Ui32) -> bool {
        (self.comp(comp_num).SSiz & 0x80) != 0
    }
    pub fn get_downsampling(&self, comp_num: Ui32) -> Point {
        let c = self.comp(comp_num);
        Point::new(c.XRsiz as Ui32, c.YRsiz as Ui32)
    }

    pub fn get_width(&self, comp_num: Ui32) -> Ui32 {
        let ds = self.comp(comp_num).XRsiz as Ui32;
        ojph_div_ceil(self.Xsiz, ds) - ojph_div_ceil(self.XOsiz, ds)
    }

    pub fn get_height(&self, comp_num: Ui32) -> Ui32 {
        let ds = self.comp(comp_num).YRsiz as Ui32;
        ojph_div_ceil(self.Ysiz, ds) - ojph_div_ceil(self.YOsiz, ds)
    }

    pub fn write(&mut self, file: &mut dyn OutfileBase) -> crate::openjph::ojph_message::OjphResult<bool> {
        self.Lsiz = 38 + 3 * self.Csiz;
        let mut result = true;
        result &= write_u16(file, swap_byte_u16(Jp2kMarker::Siz as Ui16))?;
        result &= write_u16(file, swap_byte_u16(self.Lsiz))?;
        result &= write_u16(file, swap_byte_u16(self.Rsiz))?;
        result &= write_u32(file, swap_byte_u32(self.Xsiz))?;
        result &= write_u32(file, swap_byte_u32(self.Ysiz))?;
        result &= write_u32(file, swap_byte_u32(self.XOsiz))?;
        result &= write_u32(file, swap_byte_u32(self.YOsiz))?;
        result &= write_u32(file, swap_byte_u32(self.XTsiz))?;
        result &= write_u32(file, swap_byte_u32(self.YTsiz))?;
        result &= write_u32(file, swap_byte_u32(self.XTOsiz))?;
        result &= write_u32(file, swap_byte_u32(self.YTOsiz))?;
        result &= write_u16(file, swap_byte_u16(self.Csiz))?;
        for entry in &self.comps {
            result &= file.write(&[entry.SSiz, entry.XRsiz, entry.YRsiz])? == 3;
        }
        Ok(result)
    }

    pub fn read(&mut self, file: &mut dyn InfileBase) -> crate::openjph::ojph_message::OjphResult<()> {
        if !read_u16(file, &mut self.Lsiz) {
            return Err(crate::ojph_error!(0x00050041, "error reading SIZ marker"));
        }
        self.Lsiz = swap_byte_u16(self.Lsiz);
        // 38 fixed bytes + 3 per component; a shorter Lsiz would underflow the subtraction below.
        if self.Lsiz < 38 {
            return Err(crate::ojph_error!(0x00050042, "error in SIZ marker length"));
        }
        let num_comps = ((self.Lsiz - 38) / 3) as i32;
        if self.Lsiz != 38 + 3 * num_comps as Ui16 {
            return Err(crate::ojph_error!(0x00050042, "error in SIZ marker length"));
        }
        if !read_u16(file, &mut self.Rsiz) {
            return Err(crate::ojph_error!(0x00050043, "error reading SIZ marker"));
        }
        self.Rsiz = swap_byte_u16(self.Rsiz);
        // Rsiz bit 14 (0x4000) marks an HTJ2K (Part-15) codestream. We no longer hard-reject when it
        // is clear: a plain Part-1 (legacy MQ/EBCOT) codestream is now decodable too. Whether each
        // component is HT or Part-1 is determined per-COD/COC from the code-block style HT bit.
        if (self.Rsiz & 0x8000) != 0 && (self.Rsiz & 0xD5F) != 0 {
            crate::ojph_warn!(0x00050001, "Rsiz in SIZ has unimplemented fields");
        }
        if !read_u32(file, &mut self.Xsiz) {
            return Err(crate::ojph_error!(0x00050045, "error reading SIZ marker"));
        }
        self.Xsiz = swap_byte_u32(self.Xsiz);
        if !read_u32(file, &mut self.Ysiz) {
            return Err(crate::ojph_error!(0x00050046, "error reading SIZ marker"));
        }
        self.Ysiz = swap_byte_u32(self.Ysiz);

        let mut t_xosiz = 0u32;
        let mut t_yosiz = 0u32;
        if !read_u32(file, &mut t_xosiz) {
            return Err(crate::ojph_error!(0x00050047, "error reading SIZ marker"));
        }
        if !read_u32(file, &mut t_yosiz) {
            return Err(crate::ojph_error!(0x00050048, "error reading SIZ marker"));
        }
        self.set_image_offset(Point::new(swap_byte_u32(t_xosiz), swap_byte_u32(t_yosiz)));

        let mut t_xtsiz = 0u32;
        let mut t_ytsiz = 0u32;
        if !read_u32(file, &mut t_xtsiz) {
            return Err(crate::ojph_error!(0x00050049, "error reading SIZ marker"));
        }
        if !read_u32(file, &mut t_ytsiz) {
            return Err(crate::ojph_error!(0x0005004A, "error reading SIZ marker"));
        }
        self.set_tile_size(Size::new(swap_byte_u32(t_xtsiz), swap_byte_u32(t_ytsiz)));

        let mut t_xtosiz = 0u32;
        let mut t_ytosiz = 0u32;
        if !read_u32(file, &mut t_xtosiz) {
            return Err(crate::ojph_error!(0x0005004B, "error reading SIZ marker"));
        }
        if !read_u32(file, &mut t_ytosiz) {
            return Err(crate::ojph_error!(0x0005004C, "error reading SIZ marker"));
        }
        self.set_tile_offset(Point::new(swap_byte_u32(t_xtosiz), swap_byte_u32(t_ytosiz)));

        if !read_u16(file, &mut self.Csiz) {
            return Err(crate::ojph_error!(0x0005004D, "error reading SIZ marker"));
        }
        self.Csiz = swap_byte_u16(self.Csiz);
        if self.Csiz as i32 != num_comps {
            return Err(crate::ojph_error!(0x0005004E, "Csiz does not match the SIZ marker size"));
        }
        // OpenJPH v0.31.0 hardening: reject Csiz == 0 explicitly, rather than letting a
        // zero-component image slip through to set_num_components / the per-component loop.
        if self.Csiz == 0 {
            return Err(crate::ojph_error!(0x0005004F, "Wrong Csiz value of 0 in SIZ marker segment"));
        }
        self.set_num_components(self.Csiz as Ui32);
        for entry in &mut self.comps {
            if !read_u8(file, &mut entry.SSiz) {
                return Err(crate::ojph_error!(0x00050051, "error reading SIZ marker"));
            }
            if !read_u8(file, &mut entry.XRsiz) {
                return Err(crate::ojph_error!(0x00050052, "error reading SIZ marker"));
            }
            if !read_u8(file, &mut entry.YRsiz) {
                return Err(crate::ojph_error!(0x00050053, "error reading SIZ marker"));
            }
            if (entry.SSiz & 0x7F) > 37 {
                return Err(crate::ojph_error!(0x00050054, "Wrong SIZ-SSiz value of {}", entry.SSiz));
            }
            if entry.XRsiz == 0 {
                return Err(crate::ojph_error!(0x00050055, "Wrong SIZ-XRsiz value of {}", entry.XRsiz));
            }
            if entry.YRsiz == 0 {
                return Err(crate::ojph_error!(0x00050056, "Wrong SIZ-YRsiz value of {}", entry.YRsiz));
            }
        }

        self.ws_kern_support_needed = (self.Rsiz & 0x20) != 0;
        self.dfs_support_needed = (self.Rsiz & 0x80) != 0;
        self.check_validity()
    }

    pub fn get_recon_downsampling(&self, comp_num: Ui32) -> Point {
        debug_assert!(comp_num < self.get_num_components() as Ui32);

        let mut factor = Point::new(
            1u32 << self.skipped_resolutions,
            1u32 << self.skipped_resolutions,
        );
        unsafe {
            let cdp = if self.cod.is_null() {
                ptr::null_mut()
            } else {
                (*self.cod).get_coc(comp_num)
            };
            if !self.dfs.is_null() && !cdp.is_null() && (*cdp).is_dfs_defined() {
                if let Some(d) = (*self.dfs).get_dfs((*cdp).get_dfs_index() as i32) {
                    factor = d.get_res_downsamp(self.skipped_resolutions);
                }
            }
        }
        let c = self.comp(comp_num);
        factor.x *= c.XRsiz as Ui32;
        factor.y *= c.YRsiz as Ui32;
        factor
    }

    pub fn get_recon_size(&self, comp_num: Ui32) -> Point {
        debug_assert!(comp_num < self.get_num_components() as Ui32);
        let factor = self.get_recon_downsampling(comp_num);
        Point::new(
            ojph_div_ceil(self.Xsiz, factor.x) - ojph_div_ceil(self.XOsiz, factor.x),
            ojph_div_ceil(self.Ysiz, factor.y) - ojph_div_ceil(self.YOsiz, factor.y),
        )
    }

    #[inline]
    pub fn get_recon_width(&self, comp_num: Ui32) -> Ui32 {
        self.get_recon_size(comp_num).x
    }

    #[inline]
    pub fn get_recon_height(&self, comp_num: Ui32) -> Ui32 {
        self.get_recon_size(comp_num).y
    }

    #[inline]
    pub const fn is_ws_kern_support_needed(&self) -> bool {
        self.ws_kern_support_needed
    }

    #[inline]
    pub const fn is_dfs_support_needed(&self) -> bool {
        self.dfs_support_needed
    }

    #[inline]
    pub fn set_rsiz_flag(&mut self, flag: Ui16) {
        self.Rsiz |= flag;
    }

    #[inline]
    pub fn reset_rsiz_flag(&mut self, flag: Ui16) {
        self.Rsiz &= !flag;
    }
}

#[allow(non_snake_case)]
pub struct ParamCod {
    type_: CodType,
    Lcod: Ui16,
    Scod: Ui8,
    SGCod: CodSGcod,
    SPcod: CodSpcod,
    next: *mut ParamCod,
    atk: *const ParamAtk,
    top_cod: *mut ParamCod,
    comp_idx: Ui16,
    avail: *mut ParamCod,
}

impl Default for ParamCod {
    fn default() -> Self {
        let mut value = Self {
            type_: CodType::Undefined,
            Lcod: 0,
            Scod: 0,
            SGCod: CodSGcod::default(),
            SPcod: CodSpcod::default(),
            next: ptr::null_mut(),
            atk: ptr::null(),
            top_cod: ptr::null_mut(),
            comp_idx: Self::OJPH_COD_DEFAULT,
            avail: ptr::null_mut(),
        };
        value.init(ptr::null_mut(), Self::OJPH_COD_DEFAULT);
        value
    }
}

impl Drop for ParamCod {
    fn drop(&mut self) {
        unsafe {
            if !self.avail.is_null() {
                drop(Box::from_raw(self.avail));
                self.avail = ptr::null_mut();
            }
            if !self.next.is_null() {
                drop(Box::from_raw(self.next));
                self.next = ptr::null_mut();
            }
        }
    }
}

impl ParamCod {
    /// Create without calling init() — safe to move. Call restart() at final address.
    pub fn new_uninit() -> Self {
        Self {
            type_: CodType::Undefined,
            Lcod: 0,
            Scod: 0,
            SGCod: CodSGcod::default(),
            SPcod: CodSpcod::default(),
            next: ptr::null_mut(),
            atk: ptr::null(),
            top_cod: ptr::null_mut(),
            comp_idx: Self::OJPH_COD_DEFAULT,
            avail: ptr::null_mut(),
        }
    }

    pub const OJPH_COD_UNKNOWN: Ui16 = 65_534;
    pub const OJPH_COD_DEFAULT: Ui16 = 65_535;
    pub const VERT_CAUSAL_MODE: Ui8 = crate::coding::ojph_cmodes::CAUSAL as Ui8;
    pub const HT_MODE: Ui8 = crate::coding::ojph_cmodes::HT as Ui8;
    /// With `HT_MODE`: HT and original (Part-1) code-blocks may be mixed in one component (T.814).
    pub const HT_MIXED_MODE: Ui8 = crate::coding::ojph_cmodes::HT_MIXED as Ui8;

    /// Whether a code-block style byte (SPcod/SPcoc) is decodable; see
    /// [`crate::coding::ojph_cmodes::style_ok`] (the single source of truth).
    pub const fn block_style_ok(bs: Ui8) -> bool {
        crate::coding::ojph_cmodes::style_ok(bs)
    }
    pub const DWT_IRV97: Ui8 = 0;
    pub const DWT_REV53: Ui8 = 1;

    pub fn init(&mut self, top_cod: *mut ParamCod, comp_idx: Ui16) {
        self.type_ = if top_cod.is_null() {
            CodType::CodMain
        } else {
            CodType::CocMain
        };
        self.Lcod = 0;
        self.Scod = 0;
        self.next = ptr::null_mut();
        self.atk = ptr::null();
        self.top_cod = top_cod;
        self.comp_idx = comp_idx;
    }

    pub fn restart(&mut self) {
        unsafe {
            let mut p = &mut self.avail as *mut *mut ParamCod;
            while !(*p).is_null() {
                p = &mut (**p).next as *mut *mut ParamCod;
            }
            *p = self.next;
        }
        self.init(self.top_cod, Self::OJPH_COD_DEFAULT);
    }

    pub fn set_reversible(&mut self, reversible: bool) {
        debug_assert!(
            self.type_ == CodType::Undefined
                || self.type_ == CodType::CodMain
                || self.type_ == CodType::CocMain
        );
        self.SPcod.wavelet_trans = if reversible {
            Self::DWT_REV53
        } else {
            Self::DWT_IRV97
        };
    }

    pub fn set_num_decompositions(&mut self, num: Ui8) -> crate::openjph::ojph_message::OjphResult<()> {
        if num > 32 {
            return Err(crate::ojph_error!(
                0x00050001,
                "maximum number of decompositions cannot exceed 32"
            ));
        }
        self.SPcod.num_decomp = num;
        Ok(())
    }

    /// Set code-block dimensions as pixel sizes (must be powers of 2, min 4).
    pub fn set_block_dims(&mut self, width: Ui32, height: Ui32) -> crate::openjph::ojph_message::OjphResult<()> {
        let log_w = 31 - crate::openjph::ojph_arch::count_leading_zeros_u32(width);
        let log_h = 31 - crate::openjph::ojph_arch::count_leading_zeros_u32(height);
        if width == 0
            || width != (1 << log_w)
            || height == 0
            || height != (1 << log_h)
            || log_w < 2
            || log_h < 2
            || log_w + log_h > 12
        {
            return Err(crate::ojph_error!(0x00050011, "incorrect code block dimensions"));
        }
        self.SPcod.block_width = (log_w - 2) as Ui8;
        self.SPcod.block_height = (log_h - 2) as Ui8;
        Ok(())
    }

    pub fn set_progression_order(&mut self, name: &str) -> crate::openjph::ojph_message::OjphResult<()> {
        let prog_order = match name {
            "LRCP" => ProgressionOrder::OjphPoLrcp as Ui8,
            "RLCP" => ProgressionOrder::OjphPoRlcp as Ui8,
            "RPCL" => ProgressionOrder::OjphPoRpcl as Ui8,
            "PCRL" => ProgressionOrder::OjphPoPcrl as Ui8,
            "CPRL" => ProgressionOrder::OjphPoCprl as Ui8,
            _ => {
                return Err(crate::ojph_error!(0x00050031, "unknown progression order"));
            }
        };
        self.SGCod.prog_order = prog_order;
        Ok(())
    }

    pub fn set_precinct_size(&mut self, sizes: &[Size]) -> crate::openjph::ojph_message::OjphResult<()> {
        if sizes.is_empty() {
            self.Scod &= 0xFE;
        } else {
            self.Scod |= 1;
            for i in 0..=self.SPcod.num_decomp as usize {
                let t = sizes[if i < sizes.len() { i } else { sizes.len() - 1 }];
                let ppx = 31 - crate::openjph::ojph_arch::count_leading_zeros_u32(t.w);
                let ppy = 31 - crate::openjph::ojph_arch::count_leading_zeros_u32(t.h);
                if t.w == 0 || t.h == 0 {
                    return Err(crate::ojph_error!(0x00050021, "precinct width or height cannot be 0"));
                }
                if t.w != (1 << ppx) || t.h != (1 << ppy) {
                    return Err(crate::ojph_error!(
                        0x00050022,
                        "precinct width and height should be a power of 2"
                    ));
                }
                if ppx > 15 || ppy > 15 {
                    return Err(crate::ojph_error!(0x00050023, "precinct size is too large"));
                }
                if i > 0 && (ppx == 0 || ppy == 0) {
                    return Err(crate::ojph_error!(0x00050024, "precinct size is too small"));
                }
                self.SPcod.precinct_size[i] = (ppx | (ppy << 4)) as Ui8;
            }
        }
        Ok(())
    }

    pub fn employ_color_transform(&mut self, val: Ui8) {
        debug_assert!(val == 0 || val == 1);
        debug_assert!(self.type_ == CodType::Undefined || self.type_ == CodType::CodMain);
        self.SGCod.mc_trans = val;
    }

    pub fn check_validity(&self, siz: &ParamSiz) -> crate::openjph::ojph_message::OjphResult<()> {
        debug_assert!(self.type_ == CodType::CodMain);

        let num_comps = siz.get_num_components() as i32;
        if self.SGCod.mc_trans == 1 && num_comps < 3 {
            return Err(crate::ojph_error!(
                0x00040011,
                "color transform can only be employed when the image has 3 or more color components"
            ));
        }

        if self.SGCod.mc_trans == 1 {
            let mut test_signedness = false;
            let mut test_bit_depth = false;
            let mut test_downsampling = false;
            let p = siz.get_downsampling(0);
            let bit_depth = siz.get_bit_depth(0);
            let is_signed = siz.is_signed(0);
            for i in 1..3 {
                let p1 = siz.get_downsampling(i);
                test_downsampling = test_downsampling || (p.x != p1.x || p.y != p1.y);
                test_bit_depth = test_bit_depth || (bit_depth != siz.get_bit_depth(i));
                test_signedness = test_signedness || (is_signed != siz.is_signed(i));
            }
            if test_downsampling {
                return Err(crate::ojph_error!(
                    0x00040012,
                    "when color transform is used, the first 3 colour components must have the same downsampling factor."
                ));
            }
            if test_bit_depth {
                return Err(crate::ojph_error!(
                    0x00040014,
                    "when color transform is used, the first 3 colour components must have the same bit depth."
                ));
            }
            if test_signedness {
                return Err(crate::ojph_error!(
                    0x00040015,
                    "when color transform is used, the first 3 colour components must have the same signedness (signed or unsigned)."
                ));
            }
        }

        if self.SGCod.prog_order == ProgressionOrder::OjphPoRpcl as Ui8
            || self.SGCod.prog_order == ProgressionOrder::OjphPoPcrl as Ui8
            || self.SGCod.prog_order == ProgressionOrder::OjphPoPrcl as Ui8
        {
            let num_comps = siz.get_num_components();
            for i in 0..num_comps as Ui32 {
                let r = siz.get_downsampling(i);
                if (r.x & (r.x - 1)) != 0 || (r.y & (r.y - 1)) != 0 {
                    return Err(crate::ojph_error!(
                        0x00040013,
                        "For RPCL and PCRL progression orders,component downsampling factors have to be powers of 2"
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn get_num_decompositions(&self) -> Ui8 {
        if self.type_ == CodType::CodMain {
            self.SPcod.num_decomp
        } else if self.type_ == CodType::CocMain {
            if self.is_dfs_defined() {
                unsafe { (*self.top_cod).get_num_decompositions() }
            } else {
                self.SPcod.num_decomp
            }
        } else {
            debug_assert!(false);
            0
        }
    }

    #[inline]
    pub fn get_progression_order(&self) -> Ui8 {
        if self.type_ == CodType::CodMain || self.type_ == CodType::CodTile {
            self.SGCod.prog_order
        } else {
            unsafe { (*self.top_cod).get_progression_order() }
        }
    }

    pub fn get_progression_order_as_str(&self) -> &'static str {
        match self.get_progression_order() {
            x if x == ProgressionOrder::OjphPoLrcp as Ui8 => "LRCP",
            x if x == ProgressionOrder::OjphPoRlcp as Ui8 => "RLCP",
            x if x == ProgressionOrder::OjphPoRpcl as Ui8 => "RPCL",
            x if x == ProgressionOrder::OjphPoPcrl as Ui8 => "PCRL",
            x if x == ProgressionOrder::OjphPoCprl as Ui8 => "CPRL",
            x if x == ProgressionOrder::OjphPoPrcl as Ui8 => "PRCL",
            _ => "UNKNOWN",
        }
    }

    #[inline]
    pub fn get_num_layers(&self) -> Ui16 {
        if self.type_ == CodType::CodMain || self.type_ == CodType::CodTile {
            self.SGCod.num_layers
        } else {
            unsafe { (*self.top_cod).get_num_layers() }
        }
    }

    #[inline]
    pub fn get_block_dims(&self) -> Size {
        self.SPcod.get_block_dims()
    }

    #[inline]
    pub fn get_log_block_dims(&self) -> Size {
        self.SPcod.get_log_block_dims()
    }

    #[inline]
    pub const fn get_wavelet_kern(&self) -> Ui8 {
        self.SPcod.wavelet_trans
    }

    pub fn is_reversible(&self) -> bool {
        if self.SPcod.wavelet_trans <= 1 {
            self.get_wavelet_kern() == Self::DWT_REV53
        } else {
            debug_assert!(!self.atk.is_null());
            unsafe { (*self.atk).is_reversible() }
        }
    }

    pub fn is_employing_color_transform(&self) -> bool {
        if self.type_ == CodType::CodMain || self.type_ == CodType::CodTile {
            self.SGCod.mc_trans == 1
        } else {
            unsafe { (*self.top_cod).is_employing_color_transform() }
        }
    }

    pub fn get_precinct_size(&self, res_num: Ui32) -> Size {
        let t = self.get_log_precinct_size(res_num);
        Size::new(1u32 << t.w, 1u32 << t.h)
    }

    pub fn get_log_precinct_size(&self, res_num: Ui32) -> Size {
        if (self.Scod & 1) != 0 {
            self.SPcod.get_log_precinct_size(res_num)
        } else {
            Size::new(15, 15)
        }
    }

    pub fn packets_may_use_sop(&self) -> bool {
        if self.type_ == CodType::CodMain || self.type_ == CodType::CodTile {
            (self.Scod & 2) == 2
        } else {
            false
        }
    }

    pub fn packets_use_eph(&self) -> bool {
        if self.type_ == CodType::CodMain || self.type_ == CodType::CodTile {
            (self.Scod & 4) == 4
        } else {
            false
        }
    }

    pub fn get_block_vertical_causality(&self) -> bool {
        (self.SPcod.block_style & Self::VERT_CAUSAL_MODE) != 0
    }

    /// True when this component uses HT (HTJ2K) block coding — the `HT_MODE` (0x40) bit of the
    /// code-block style. When false the component uses Part-1 (legacy MQ/EBCOT) block coding.
    pub fn is_ht(&self) -> bool {
        (self.SPcod.block_style & Self::HT_MODE) != 0
    }

    /// The raw code-block style byte (SPcod/SPcoc). Its low 6 bits are the Part-1 coding modes
    /// (BYPASS/RESET/RESTART/CAUSAL/ERTERM/SEGMARK); bit 6 (`HT_MODE`) selects HT.
    pub fn get_block_style(&self) -> Ui8 {
        self.SPcod.block_style
    }

    pub fn write(&mut self, file: &mut dyn OutfileBase) -> crate::openjph::ojph_message::OjphResult<bool> {
        debug_assert!(self.type_ == CodType::CodMain);
        self.Lcod = 12;
        self.Lcod = (self.Lcod
            + if (self.Scod & 1) != 0 {
                1 + self.SPcod.num_decomp as Ui16
            } else {
                0
            }) as Ui16;

        let mut result = true;
        result &= write_u16(file, swap_byte_u16(Jp2kMarker::Cod as Ui16))?;
        result &= write_u16(file, swap_byte_u16(self.Lcod))?;
        result &= file.write(&[self.Scod])? == 1;
        result &= file.write(&[self.SGCod.prog_order])? == 1;
        result &= write_u16(file, swap_byte_u16(self.SGCod.num_layers))?;
        result &= file.write(&[self.SGCod.mc_trans])? == 1;
        result &= file.write(&[
            self.SPcod.num_decomp,
            self.SPcod.block_width,
            self.SPcod.block_height,
            self.SPcod.block_style,
        ])? == 4;
        result &= file.write(&[self.SPcod.wavelet_trans])? == 1;
        if (self.Scod & 1) != 0 {
            for i in 0..=self.SPcod.num_decomp as usize {
                result &= file.write(&[self.SPcod.precinct_size[i]])? == 1;
            }
        }
        Ok(result)
    }

    pub fn write_coc(
        &mut self,
        file: &mut dyn OutfileBase,
        num_comps: Ui32,
    ) -> crate::openjph::ojph_message::OjphResult<bool> {
        debug_assert!(self.type_ == CodType::CodMain);
        let mut result = true;
        let mut p = self.next;
        unsafe {
            while !p.is_null() {
                if (*p).comp_idx < num_comps as Ui16 {
                    result &= (*p).internal_write_coc(file, num_comps)?;
                }
                p = (*p).next;
            }
        }
        Ok(result)
    }

    pub fn read(&mut self, file: &mut dyn InfileBase) -> crate::openjph::ojph_message::OjphResult<()> {
        debug_assert!(self.type_ == CodType::CodMain);
        if !read_u16(file, &mut self.Lcod) {
            return Err(crate::ojph_error!(0x00050071, "error reading COD segment"));
        }
        self.Lcod = swap_byte_u16(self.Lcod);
        if !read_u8(file, &mut self.Scod) {
            return Err(crate::ojph_error!(0x00050072, "error reading COD segment"));
        }
        if !read_u8(file, &mut self.SGCod.prog_order) {
            return Err(crate::ojph_error!(0x00050073, "error reading COD segment"));
        }
        if !read_u16(file, &mut self.SGCod.num_layers) {
            return Err(crate::ojph_error!(0x00050074, "error reading COD segment"));
        } else {
            self.SGCod.num_layers = swap_byte_u16(self.SGCod.num_layers);
        }
        if !read_u8(file, &mut self.SGCod.mc_trans) {
            return Err(crate::ojph_error!(0x00050075, "error reading COD segment"));
        }
        // Reject values the tile parser has no iteration for, instead of decoding a blank tile
        // (an unknown progression order used to fall through to "no packets, Ok"), or treating
        // zero layers / a reserved MCT value as if they were valid.
        if self.SGCod.prog_order > ProgressionOrder::OjphPoPrcl as Ui8 {
            return Err(crate::ojph_error!(
                0x000300E6,
                "COD progression order {} is not defined",
                self.SGCod.prog_order
            ));
        }
        if self.SGCod.num_layers == 0 {
            return Err(crate::ojph_error!(0x000300E7, "COD declares zero quality layers"));
        }
        if self.SGCod.mc_trans > 1 {
            return Err(crate::ojph_error!(
                0x000300E8,
                "COD multiple component transform {} is reserved",
                self.SGCod.mc_trans
            ));
        }
        if !read_u8(file, &mut self.SPcod.num_decomp) {
            return Err(crate::ojph_error!(0x00050076, "error reading COD segment"));
        }
        if !read_u8(file, &mut self.SPcod.block_width) {
            return Err(crate::ojph_error!(0x00050077, "error reading COD segment"));
        }
        if !read_u8(file, &mut self.SPcod.block_height) {
            return Err(crate::ojph_error!(0x00050078, "error reading COD segment"));
        }
        if !read_u8(file, &mut self.SPcod.block_style) {
            return Err(crate::ojph_error!(0x00050079, "error reading COD segment"));
        }
        if !read_u8(file, &mut self.SPcod.wavelet_trans) {
            return Err(crate::ojph_error!(0x0005007A, "error reading COD segment"));
        }

        if self.get_num_decompositions() > 32
            || self.SPcod.block_width > 8
            || self.SPcod.block_height > 8
            || self.SPcod.block_width + self.SPcod.block_height > 8
            || !Self::block_style_ok(self.SPcod.block_style)
        {
            return Err(crate::ojph_error!(0x0005007D, "wrong settings in a COD-SPcod parameter"));
        }
        let num_decompositions = self.get_num_decompositions();
        if (self.Scod & 1) != 0 {
            for i in 0..=num_decompositions as usize {
                if !read_u8(file, &mut self.SPcod.precinct_size[i]) {
                    return Err(crate::ojph_error!(0x0005007B, "error reading COD segment"));
                }
                // OpenJPH v0.31.0 hardening: for resolutions other than the coarsest
                // (i > 0), a precinct width or height of 1 (nibble == 0) is invalid.
                if i != 0
                    && ((self.SPcod.precinct_size[i] & 0x0F) == 0
                        || (self.SPcod.precinct_size[i] >> 4) == 0)
                {
                    return Err(crate::ojph_error!(
                        0x0005007F,
                        "Precinct width or height for resolutions other than the coarsest must be larger than 1; here, they are {} and {}, respectively.",
                        1u32 << (self.SPcod.precinct_size[i] & 0x0F),
                        1u32 << (self.SPcod.precinct_size[i] >> 4)
                    ));
                }
            }
        }
        if self.Lcod
            != 12
                + if (self.Scod & 1) != 0 {
                    1 + self.SPcod.num_decomp as Ui16
                } else {
                    0
                }
        {
            return Err(crate::ojph_error!(0x0005007C, "error in COD segment length"));
        }
        Ok(())
    }

    pub fn read_coc(
        &mut self,
        file: &mut dyn InfileBase,
        num_comps: Ui32,
        top_cod: *mut ParamCod,
    ) -> crate::openjph::ojph_message::OjphResult<()> {
        debug_assert!(self.type_ == CodType::CocMain);
        debug_assert!(!top_cod.is_null());
        unsafe {
            self.SGCod = (*top_cod).SGCod;
        }
        self.top_cod = top_cod;
        if !read_u16(file, &mut self.Lcod) {
            return Err(crate::ojph_error!(0x00050121, "error reading COC segment"));
        }
        self.Lcod = swap_byte_u16(self.Lcod);
        if num_comps < 257 {
            let mut t = 0u8;
            if !read_u8(file, &mut t) {
                return Err(crate::ojph_error!(0x00050122, "error reading COC segment"));
            }
            self.comp_idx = t as Ui16;
        } else {
            if !read_u16(file, &mut self.comp_idx) {
                return Err(crate::ojph_error!(0x00050123, "error reading COC segment"));
            }
            self.comp_idx = swap_byte_u16(self.comp_idx);
        }
        if !read_u8(file, &mut self.Scod) {
            return Err(crate::ojph_error!(0x00050124, "error reading COC segment"));
        }
        if (self.Scod & 0xF8) != 0 {
            crate::ojph_warn!(
                0x00050011,
                "Unsupported options in Scoc field of the COC segment"
            );
        }
        if !read_u8(file, &mut self.SPcod.num_decomp) {
            return Err(crate::ojph_error!(0x00050125, "error reading COC segment"));
        }
        if !read_u8(file, &mut self.SPcod.block_width) {
            return Err(crate::ojph_error!(0x00050126, "error reading COC segment"));
        }
        if !read_u8(file, &mut self.SPcod.block_height) {
            return Err(crate::ojph_error!(0x00050127, "error reading COC segment"));
        }
        if !read_u8(file, &mut self.SPcod.block_style) {
            return Err(crate::ojph_error!(0x00050128, "error reading COC segment"));
        }
        if !read_u8(file, &mut self.SPcod.wavelet_trans) {
            return Err(crate::ojph_error!(0x00050129, "error reading COC segment"));
        }
        if self.get_num_decompositions() > 32
            || self.SPcod.block_width > 8
            || self.SPcod.block_height > 8
            || self.SPcod.block_width + self.SPcod.block_height > 8
            || !Self::block_style_ok(self.SPcod.block_style)
        {
            return Err(crate::ojph_error!(0x0005012C, "wrong settings in a COC-SPcoc parameter"));
        }
        let num_decompositions = self.get_num_decompositions();
        if (self.Scod & 1) != 0 {
            for i in 0..=num_decompositions as usize {
                if !read_u8(file, &mut self.SPcod.precinct_size[i]) {
                    return Err(crate::ojph_error!(0x0005012A, "error reading COC segment"));
                }
                // OpenJPH v0.31.0 hardening: see the matching COD-read check above.
                if i != 0
                    && ((self.SPcod.precinct_size[i] & 0x0F) == 0
                        || (self.SPcod.precinct_size[i] >> 4) == 0)
                {
                    return Err(crate::ojph_error!(
                        0x0005012E,
                        "Precinct width or height for resolutions other than the coarsest must be larger than 1; here, they are {} and {}, respectively.",
                        1u32 << (self.SPcod.precinct_size[i] & 0x0F),
                        1u32 << (self.SPcod.precinct_size[i] >> 4)
                    ));
                }
            }
        }
        let mut t = 9u32;
        t += if num_comps < 257 { 0 } else { 1 };
        t += if (self.Scod & 1) != 0 {
            1 + num_decompositions as Ui32
        } else {
            0
        };
        if self.Lcod as Ui32 != t {
            return Err(crate::ojph_error!(0x0005012B, "error in COC segment length"));
        }
        Ok(())
    }

    pub fn update_atk(&mut self, atk: &mut ParamAtk) -> crate::openjph::ojph_message::OjphResult<()> {
        debug_assert!(self.type_ == CodType::CodMain);
        self.atk = atk.get_atk(self.SPcod.wavelet_trans as i32);
        if self.atk.is_null() {
            return Err(crate::ojph_error!(
                0x00050131,
                "A COD segment employs the DWT kernel atk = {}, but a corresponding ATK segment cannot be found.",
                self.SPcod.wavelet_trans
            ));
        }
        let mut p = self.next;
        unsafe {
            while !p.is_null() {
                (*p).atk = atk.get_atk((*p).SPcod.wavelet_trans as i32);
                if (*p).atk.is_null() {
                    return Err(crate::ojph_error!(
                        0x00050132,
                        "A COC segment employs the DWT kernel atk = {}, but a corresponding ATK segment cannot be found",
                        self.SPcod.wavelet_trans
                    ));
                }
                p = (*p).next;
            }
        }
        Ok(())
    }

    pub fn get_coc(&self, comp_idx: Ui32) -> *mut ParamCod {
        let (mut p, q) = if self.type_ == CodType::CodMain {
            (
                self as *const ParamCod as *mut ParamCod,
                self as *const ParamCod as *mut ParamCod,
            )
        } else {
            (self.top_cod, self.top_cod)
        };
        unsafe {
            while !p.is_null() && (*p).comp_idx != comp_idx as Ui16 {
                p = (*p).next;
            }
        }
        if p.is_null() { q } else { p }
    }

    #[inline]
    pub const fn is_dfs_defined(&self) -> bool {
        (self.SPcod.num_decomp & 0x80) != 0
    }

    #[inline]
    pub const fn get_dfs_index(&self) -> Ui16 {
        (self.SPcod.num_decomp & 0xF) as Ui16
    }

    pub fn add_coc_object(&mut self, comp_idx: Ui32) -> *mut ParamCod {
        debug_assert!(self.type_ == CodType::CodMain);
        let mut p = self as *mut ParamCod;
        unsafe {
            while !(*p).next.is_null() {
                p = (*p).next;
            }
            if !self.avail.is_null() {
                (*p).next = self.avail;
                self.avail = (*self.avail).next;
                (*(*p).next).init(self, comp_idx as Ui16);
            } else {
                let mut node = Box::new(ParamCod::default());
                node.init(self, comp_idx as Ui16);
                (*p).next = Box::into_raw(node);
            }
            (*p).next
        }
    }

    pub const fn access_atk(&self) -> *const ParamAtk {
        self.atk
    }

    pub fn get_comp_idx(&self) -> Ui32 {
        debug_assert!(
            (self.type_ == CodType::CocMain && self.comp_idx != Self::OJPH_COD_DEFAULT)
                || (self.type_ == CodType::CodMain && self.comp_idx == Self::OJPH_COD_DEFAULT)
        );
        self.comp_idx as Ui32
    }

    fn internal_write_coc(
        &mut self,
        file: &mut dyn OutfileBase,
        num_comps: Ui32,
    ) -> crate::openjph::ojph_message::OjphResult<bool> {
        debug_assert!(self.type_ == CodType::CocMain);
        self.Lcod = if num_comps < 257 { 9 } else { 10 };
        self.Lcod = (self.Lcod
            + if (self.Scod & 1) != 0 {
                1 + self.SPcod.num_decomp as Ui16
            } else {
                0
            }) as Ui16;

        let mut result = true;
        result &= write_u16(file, swap_byte_u16(Jp2kMarker::Coc as Ui16))?;
        result &= write_u16(file, swap_byte_u16(self.Lcod))?;
        if num_comps < 257 {
            result &= file.write(&[self.comp_idx as Ui8])? == 1;
        } else {
            result &= write_u16(file, swap_byte_u16(self.comp_idx))?;
        }
        result &= file.write(&[self.Scod])? == 1;
        result &= file.write(&[
            self.SPcod.num_decomp,
            self.SPcod.block_width,
            self.SPcod.block_height,
            self.SPcod.block_style,
        ])? == 4;
        result &= file.write(&[self.SPcod.wavelet_trans])? == 1;
        if (self.Scod & 1) != 0 {
            for i in 0..=self.SPcod.num_decomp as usize {
                result &= file.write(&[self.SPcod.precinct_size[i]])? == 1;
            }
        }
        Ok(result)
    }
}

const SQRT_ENERGY_GAIN_9X7_L: [f32; 34] = [
    1.0000e+00f32,
    1.4021e+00f32,
    2.0304e+00f32,
    2.9012e+00f32,
    4.1153e+00f32,
    5.8245e+00f32,
    8.2388e+00f32,
    1.1652e+01f32,
    1.6479e+01f32,
    2.3304e+01f32,
    3.2957e+01f32,
    4.6609e+01f32,
    6.5915e+01f32,
    9.3217e+01f32,
    1.3183e+02f32,
    1.8643e+02f32,
    2.6366e+02f32,
    3.7287e+02f32,
    5.2732e+02f32,
    7.4574e+02f32,
    1.0546e+03f32,
    1.4915e+03f32,
    2.1093e+03f32,
    2.9830e+03f32,
    4.2185e+03f32,
    5.9659e+03f32,
    8.4371e+03f32,
    1.1932e+04f32,
    1.6874e+04f32,
    2.3864e+04f32,
    3.3748e+04f32,
    4.7727e+04f32,
    6.7496e+04f32,
    9.5454e+04f32,
];

const SQRT_ENERGY_GAIN_9X7_H: [f32; 34] = [
    1.4425e+00f32,
    1.9669e+00f32,
    2.8839e+00f32,
    4.1475e+00f32,
    5.8946e+00f32,
    8.3472e+00f32,
    1.1809e+01f32,
    1.6701e+01f32,
    2.3620e+01f32,
    3.3403e+01f32,
    4.7240e+01f32,
    6.6807e+01f32,
    9.4479e+01f32,
    1.3361e+02f32,
    1.8896e+02f32,
    2.6723e+02f32,
    3.7792e+02f32,
    5.3446e+02f32,
    7.5583e+02f32,
    1.0689e+03f32,
    1.5117e+03f32,
    2.1378e+03f32,
    3.0233e+03f32,
    4.2756e+03f32,
    6.0467e+03f32,
    8.5513e+03f32,
    1.2093e+04f32,
    1.7103e+04f32,
    2.4187e+04f32,
    3.4205e+04f32,
    4.8373e+04f32,
    6.8410e+04f32,
    9.6747e+04f32,
    1.3682e+05f32,
];

const SQRT_ENERGY_GAIN_5X3_L: [f32; 34] = [
    1.0000e+00f32,
    1.2247e+00f32,
    1.3229e+00f32,
    1.5411e+00f32,
    1.7139e+00f32,
    1.9605e+00f32,
    2.2044e+00f32,
    2.5047e+00f32,
    2.8277e+00f32,
    3.2049e+00f32,
    3.6238e+00f32,
    4.1033e+00f32,
    4.6423e+00f32,
    5.2548e+00f32,
    5.9462e+00f32,
    6.7299e+00f32,
    7.6159e+00f32,
    8.6193e+00f32,
    9.7544e+00f32,
    1.1039e+01f32,
    1.2493e+01f32,
    1.4139e+01f32,
    1.6001e+01f32,
    1.8108e+01f32,
    2.0493e+01f32,
    2.3192e+01f32,
    2.6246e+01f32,
    2.9702e+01f32,
    3.3614e+01f32,
    3.8041e+01f32,
    4.3051e+01f32,
    4.8721e+01f32,
    5.5138e+01f32,
    6.2399e+01f32,
];

const SQRT_ENERGY_GAIN_5X3_H: [f32; 34] = [
    1.0458e+00f32,
    1.3975e+00f32,
    1.4389e+00f32,
    1.7287e+00f32,
    1.8880e+00f32,
    2.1841e+00f32,
    2.4392e+00f32,
    2.7830e+00f32,
    3.1341e+00f32,
    3.5576e+00f32,
    4.0188e+00f32,
    4.5532e+00f32,
    5.1494e+00f32,
    5.8301e+00f32,
    6.5963e+00f32,
    7.4663e+00f32,
    8.4489e+00f32,
    9.5623e+00f32,
    1.0821e+01f32,
    1.2247e+01f32,
    1.3860e+01f32,
    1.5685e+01f32,
    1.7751e+01f32,
    2.0089e+01f32,
    2.2735e+01f32,
    2.5729e+01f32,
    2.9117e+01f32,
    3.2952e+01f32,
    3.7292e+01f32,
    4.2203e+01f32,
    4.7761e+01f32,
    5.4051e+01f32,
    6.1170e+01f32,
    6.9226e+01f32,
];

const BIBO_GAIN_9X7_L: [f32; 34] = [
    1.0000e+00f32,
    1.3803e+00f32,
    1.3328e+00f32,
    1.3067e+00f32,
    1.3028e+00f32,
    1.3001e+00f32,
    1.2993e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
    1.2992e+00f32,
];

const BIBO_GAIN_9X7_H: [f32; 34] = [
    1.2976e+00f32,
    1.3126e+00f32,
    1.2757e+00f32,
    1.2352e+00f32,
    1.2312e+00f32,
    1.2285e+00f32,
    1.2280e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
    1.2278e+00f32,
];

const BIBO_GAIN_5X3_L: [f32; 34] = [
    1.0000e+00f32,
    1.5000e+00f32,
    1.6250e+00f32,
    1.6875e+00f32,
    1.6963e+00f32,
    1.7067e+00f32,
    1.7116e+00f32,
    1.7129e+00f32,
    1.7141e+00f32,
    1.7145e+00f32,
    1.7151e+00f32,
    1.7152e+00f32,
    1.7155e+00f32,
    1.7155e+00f32,
    1.7156e+00f32,
    1.7156e+00f32,
    1.7156e+00f32,
    1.7156e+00f32,
    1.7156e+00f32,
    1.7156e+00f32,
    1.7156e+00f32,
    1.7156e+00f32,
    1.7156e+00f32,
    1.7156e+00f32,
    1.7156e+00f32,
    1.7156e+00f32,
    1.7156e+00f32,
    1.7156e+00f32,
    1.7156e+00f32,
    1.7156e+00f32,
    1.7156e+00f32,
    1.7156e+00f32,
    1.7156e+00f32,
    1.7156e+00f32,
];

const BIBO_GAIN_5X3_H: [f32; 34] = [
    2.0000e+00f32,
    2.5000e+00f32,
    2.7500e+00f32,
    2.8047e+00f32,
    2.8198e+00f32,
    2.8410e+00f32,
    2.8558e+00f32,
    2.8601e+00f32,
    2.8628e+00f32,
    2.8656e+00f32,
    2.8662e+00f32,
    2.8667e+00f32,
    2.8669e+00f32,
    2.8670e+00f32,
    2.8671e+00f32,
    2.8671e+00f32,
    2.8671e+00f32,
    2.8671e+00f32,
    2.8671e+00f32,
    2.8671e+00f32,
    2.8671e+00f32,
    2.8671e+00f32,
    2.8671e+00f32,
    2.8671e+00f32,
    2.8671e+00f32,
    2.8671e+00f32,
    2.8671e+00f32,
    2.8671e+00f32,
    2.8671e+00f32,
    2.8671e+00f32,
    2.8671e+00f32,
    2.8671e+00f32,
    2.8671e+00f32,
    2.8671e+00f32,
];

#[inline]
fn sqrt_energy_gain_l(num_decomp: Ui32, reversible: bool) -> f32 {
    if reversible {
        SQRT_ENERGY_GAIN_5X3_L[num_decomp as usize]
    } else {
        SQRT_ENERGY_GAIN_9X7_L[num_decomp as usize]
    }
}

#[inline]
fn sqrt_energy_gain_h(num_decomp: Ui32, reversible: bool) -> f32 {
    if reversible {
        SQRT_ENERGY_GAIN_5X3_H[num_decomp as usize]
    } else {
        SQRT_ENERGY_GAIN_9X7_H[num_decomp as usize]
    }
}

#[inline]
fn bibo_gain_l(num_decomp: Ui32, reversible: bool) -> f32 {
    if reversible {
        BIBO_GAIN_5X3_L[num_decomp as usize]
    } else {
        BIBO_GAIN_9X7_L[num_decomp as usize]
    }
}

#[inline]
fn bibo_gain_h(num_decomp: Ui32, reversible: bool) -> f32 {
    if reversible {
        BIBO_GAIN_5X3_H[num_decomp as usize]
    } else {
        BIBO_GAIN_9X7_H[num_decomp as usize]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum QcdType {
    Undefined = 0,
    QcdMain = 1,
    QccMain = 2,
    QcdTile = 3,
    QccTile = 4,
}

/// Port target: `https://github.com/aous72/OpenJPH/blob/v0.31.0/src/core/openjph/ojph_params.h`
/// (`param_qcd::comp_type`, added in v0.31.0 alongside the qfactor feature). Identifies which
/// visually-weighted role a component plays after the colour transform, for `set_qfactor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CompType {
    Y = 0,
    Cb = 1,
    Cr = 2,
    Undefined = 0xFF,
}

impl CompType {
    /// Port of `param_qcd::ui8_2_comp_type`.
    pub fn from_u8(c: Ui8) -> Self {
        match c {
            0 => CompType::Y,
            1 => CompType::Cb,
            2 => CompType::Cr,
            _ => CompType::Undefined,
        }
    }
}

/// Sentinel for "no qfactor set" (`param_qcd::qfactor_const::QFACTOR_UNSET` in v0.31.0).
const QFACTOR_UNSET: Ui8 = 0;

/// Port target: `https://github.com/aous72/OpenJPH/blob/v0.31.0/src/core/codestream/ojph_params.cpp`
/// (`class visual_weights`, added in v0.31.0 to support `param_qcd::set_qfactor`). Not a 1:1-port
/// exception to the "idiomatic Rust only in api/ facade" rule — it is itself a direct, structure-
/// preserving port of the C++ `visual_weights` class (same tables, same formulas), just as a free
/// module instead of a nested class (Rust has no nested nominal classes with static methods).
#[allow(clippy::approx_constant)] // literal tables ported verbatim from the C++ source
mod visual_weights {
    use super::CompType;
    use crate::openjph::ojph_base::Point;
    use crate::openjph::ojph_defs::Ui32;

    #[derive(Clone, Copy, PartialEq, Eq)]
    #[allow(dead_code)] // Format400 mirrors C++'s enum value; unreachable from get_format's inputs
    pub enum ColourFormat {
        Format400,
        Format420,
        Format422,
        Format444,
        FormatError,
    }

    pub fn get_format(component_subsampling: Point) -> ColourFormat {
        if component_subsampling.x == 2 && component_subsampling.y == 2 {
            ColourFormat::Format420
        } else if component_subsampling.x == 2 && component_subsampling.y == 1 {
            ColourFormat::Format422
        } else if component_subsampling.x == 1 && component_subsampling.y == 1 {
            ColourFormat::Format444
        } else {
            ColourFormat::FormatError
        }
    }

    #[rustfmt::skip]
    const CB420: [f32; 19] = [
        0.2724, 0.5128, 0.5128,           // level 1
        0.6692, 0.9382, 0.9382,           // level 2
        1.0888, 1.3046, 1.3046,           // level 3
        1.4156, 1.5594, 1.5594,           // level 4
        2.0,    2.0,    2.0,              // level 5
        2.0,    2.0,    2.0,    2.0,      // level 6 + LL
    ];
    #[rustfmt::skip]
    const CR420: [f32; 19] = [
        0.5196, 0.8260, 0.8260,
        1.0080, 1.2928, 1.2928,
        1.4440, 1.6508, 1.6508,
        1.7538, 1.8848, 1.8848,
        2.0,    2.0,    2.0,
        2.0,    2.0,    2.0,    2.0,
    ];
    #[rustfmt::skip]
    const CB422: [f32; 19] = [
        0.1220, 0.1220, 0.3626,
        0.3626, 0.3626, 0.6634,
        0.6634, 0.6634, 0.9225,
        0.9225, 0.9225, 1.1027,
        1.1027, 1.1027, 1.4142,
        1.4142, 1.4142, 1.4142, 1.4142,
    ];
    #[rustfmt::skip]
    const CR422: [f32; 19] = [
        0.2595, 0.2595, 0.5841,
        0.5841, 0.5841, 0.9141,
        0.9141, 0.9141, 1.1673,
        1.1673, 1.1673, 1.3328,
        1.3328, 1.3328, 1.4142,
        1.4142, 1.4142, 1.4142, 1.4142,
    ];
    #[rustfmt::skip]
    const CB444: [f32; 19] = [
        0.0263, 0.0863, 0.0863,
        0.1362, 0.2564, 0.2564,
        0.3346, 0.4691, 0.4691,
        0.5444, 0.6523, 0.6523,
        0.7078, 0.7797, 0.7797,
        1.0,    1.0,    1.0,    1.0,
    ];
    #[rustfmt::skip]
    const CR444: [f32; 19] = [
        0.0773, 0.1835, 0.1835,
        0.2598, 0.4130, 0.4130,
        0.5040, 0.6464, 0.6464,
        0.7220, 0.8254, 0.8254,
        0.8769, 0.9424, 0.9424,
        1.0,    1.0,    1.0,    1.0,
    ];
    #[rustfmt::skip]
    const Y: [f32; 19] = [
        0.0901, 0.2758, 0.2758,
        0.7018, 0.8378, 0.8378,
        1.0,    1.0,    1.0,
        1.0,    1.0,    1.0,
        1.0,    1.0,    1.0,
        1.0,    1.0,    1.0,    1.0,
    ];
    #[rustfmt::skip]
    const NO_WEIGHTS: [f32; 19] = [
        1.0, 1.0, 1.0,
        1.0, 1.0, 1.0,
        1.0, 1.0, 1.0,
        1.0, 1.0, 1.0,
        1.0, 1.0, 1.0,
        1.0, 1.0, 1.0, 1.0,
    ];

    pub fn get_weights(format: ColourFormat, comp_type: CompType) -> &'static [f32; 19] {
        match comp_type {
            CompType::Y => &Y,
            CompType::Cb => match format {
                ColourFormat::Format420 => &CB420,
                ColourFormat::Format422 => &CB422,
                ColourFormat::Format444 => &CB444,
                _ => {
                    debug_assert!(false);
                    &Y
                }
            },
            CompType::Cr => match format {
                ColourFormat::Format420 => &CR420,
                ColourFormat::Format422 => &CR422,
                ColourFormat::Format444 => &CR444,
                _ => {
                    debug_assert!(false);
                    &Y
                }
            },
            CompType::Undefined => {
                debug_assert!(false);
                &Y
            }
        }
    }

    pub fn get_no_weights() -> &'static [f32; 19] {
        &NO_WEIGHTS
    }

    pub fn get_weight(v: &[f32; 19], decomposition_level: Ui32, subband_idx: Ui32) -> f32 {
        if subband_idx == 0 {
            v[18]
        } else {
            debug_assert!((1..=3).contains(&subband_idx));
            debug_assert!(decomposition_level > 0);
            let decomposition_level = decomposition_level.min(6);
            let index = (decomposition_level - 1) * 3 + (3 - subband_idx);
            v[index as usize]
        }
    }

    pub fn get_gain(comp_type: CompType) -> f32 {
        match comp_type {
            CompType::Y => 1.0,
            CompType::Cb => 1.8051 / 1.7321,
            CompType::Cr => 1.5734 / 1.7321,
            CompType::Undefined => {
                debug_assert!(false);
                0.0
            }
        }
    }

    /// Returns `delta_ref` and `power` to be used with visual weights.
    pub fn get_delta_ref(qfactor: Ui32, bit_depth: Ui32, power: &mut f32) -> f32 {
        const T0: f32 = 65.0;
        const T1: f32 = 97.0;
        const ALPHA_T0: f32 = 0.04;
        const ALPHA_T1: f32 = 0.10;
        let m_t0 = 2.0 * (1.0 - T0 / 100.0);
        let m_t1 = 2.0 * (1.0 - T1 / 100.0);

        let m_q = if qfactor < 50 {
            50.0 / qfactor as f32
        } else {
            2.0 * (1.0 - qfactor as f32 / 100.0)
        };

        let alpha_q;
        if (qfactor as f32) <= T0 {
            *power = 1.0;
            alpha_q = ALPHA_T0;
        } else if (qfactor as f32) < T1 {
            *power = (m_q.ln() - m_t1.ln()) / (m_t0.ln() - m_t1.ln());
            alpha_q = ALPHA_T1 * (ALPHA_T0 / ALPHA_T1).powf(*power);
        } else {
            *power = 0.0;
            alpha_q = ALPHA_T1;
        }
        let eps = 0.5f32.sqrt() * (2.0f32).powi(-(bit_depth as i32));
        alpha_q * m_q + eps
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
union QcdSpqcd {
    u8_: [Ui8; 97],
    u16_: [Ui16; 97],
}

impl Default for QcdSpqcd {
    fn default() -> Self {
        Self { u8_: [0; 97] }
    }
}

#[allow(non_snake_case)]
pub struct ParamQcd {
    type_: QcdType,
    Lqcd: Ui16,
    Sqcd: Ui8,
    SPqcd: QcdSpqcd,
    num_subbands: Ui32,
    base_delta: f32,
    enabled: bool,
    next: *mut ParamQcd,
    top_qcd: *mut ParamQcd,
    comp_idx: Ui16,
    avail: *mut ParamQcd,
    // v0.31.0 qfactor extension (`param_qcd::make_quant_steps`/`is_qcc_needed`/`set_qfactor`).
    /// Have the quantization step sizes been generated for this object (`is_init` in C++).
    is_init: bool,
    qfactor: Ui8,
    ctype: CompType,
    is_color_trans: bool,
    num_decomps: Ui32,
    bit_depth: Ui32,
    is_signed: bool,
    wavelet_kern: Ui32,
    sampling: Point,
}

impl Default for ParamQcd {
    fn default() -> Self {
        let mut value = Self {
            type_: QcdType::Undefined,
            Lqcd: 0,
            Sqcd: 0,
            SPqcd: QcdSpqcd::default(),
            num_subbands: 0,
            base_delta: -1.0,
            enabled: true,
            next: ptr::null_mut(),
            top_qcd: ptr::null_mut(),
            comp_idx: Self::OJPH_QCD_DEFAULT,
            avail: ptr::null_mut(),
            is_init: false,
            qfactor: QFACTOR_UNSET,
            ctype: CompType::Y,
            is_color_trans: false,
            num_decomps: 0,
            bit_depth: 0,
            is_signed: false,
            wavelet_kern: 0,
            sampling: Point::new(1, 1),
        };
        value.init(ptr::null_mut(), Self::OJPH_QCD_DEFAULT);
        value
    }
}

impl Drop for ParamQcd {
    fn drop(&mut self) {
        unsafe {
            if !self.avail.is_null() {
                drop(Box::from_raw(self.avail));
                self.avail = ptr::null_mut();
            }
            if !self.next.is_null() {
                drop(Box::from_raw(self.next));
                self.next = ptr::null_mut();
            }
        }
    }
}

#[allow(non_snake_case)]
impl ParamQcd {
    pub const OJPH_QCD_UNKNOWN: Ui16 = 65_534;
    pub const OJPH_QCD_DEFAULT: Ui16 = 65_535;

    fn init(&mut self, top_qcd: *mut ParamQcd, comp_idx: Ui16) {
        self.type_ = if top_qcd.is_null() {
            QcdType::QcdMain
        } else {
            QcdType::QccMain
        };
        self.Lqcd = 0;
        self.Sqcd = 0;
        self.SPqcd = QcdSpqcd::default();
        self.num_subbands = 0;
        self.enabled = true;
        self.next = ptr::null_mut();
        self.top_qcd = top_qcd;
        self.comp_idx = comp_idx;
        // v0.31.0 qfactor extension: `init()` also resets is_init/qfactor/ctype/sampling (but not
        // is_color_trans/num_decomps/bit_depth/is_signed/wavelet_kern, which are only meaningful
        // once `is_init` is true again via `make_quant_steps`).
        self.base_delta = -1.0;
        self.is_init = false;
        self.qfactor = QFACTOR_UNSET;
        self.ctype = CompType::Y;
        self.sampling = Point::new(1, 1);
    }

    pub fn restart(&mut self) {
        unsafe {
            let mut p = &mut self.avail as *mut *mut ParamQcd;
            while !(*p).is_null() {
                p = &mut (**p).next as *mut *mut ParamQcd;
            }
            *p = self.next;
        }
        self.init(self.top_qcd, Self::OJPH_QCD_DEFAULT);
    }

    #[inline]
    pub fn set_delta(&mut self, delta: f32) {
        self.base_delta = delta;
    }

    /// Copy the quantization payload (`Lqcd`/`Sqcd`/`SPqcd`/subband count/base delta) from `src`
    /// into `self`, leaving the linked-list bookkeeping (`type_`, `next`, `top_qcd`, `comp_idx`,
    /// `avail`) untouched. Used to seed a tile-scoped QCD from the main QCD before applying an
    /// in-tile QCD/QCC override, so components without their own in-tile entry keep the main values.
    ///
    /// When both are main QCDs the per-component QCC chain is cloned too: a tile override of ONE
    /// component must not make the others lose their main-header QCC.
    pub fn copy_quant_from(&mut self, src: &ParamQcd) {
        self.Lqcd = src.Lqcd;
        self.Sqcd = src.Sqcd;
        self.SPqcd = src.SPqcd;
        self.num_subbands = src.num_subbands;
        self.base_delta = src.base_delta;
        if self.type_ == QcdType::QcdMain && src.type_ == QcdType::QcdMain {
            let mut n = src.next;
            while !n.is_null() {
                unsafe {
                    let d = self.add_qcc_object((*n).comp_idx as Ui32);
                    (*d).copy_quant_from(&*n);
                    n = (*n).next;
                }
            }
        }
    }

    /// Settle the freshly parsed QCC node `node` (last in the chain): if an earlier node already
    /// targets the same component (copied from the main header), the new payload replaces it and
    /// `node` goes back to the free list — `get_qcc` returns the FIRST match, so a duplicate would
    /// leave the stale main-header values in force.
    pub fn commit_qcc(&mut self, node: *mut ParamQcd) {
        debug_assert!(self.type_ == QcdType::QcdMain);
        unsafe {
            let mut p = self as *mut ParamQcd;
            while (*p).next != node {
                if (*p).next.is_null() {
                    return;
                }
                p = (*p).next;
            }
            let mut q = self.next;
            while q != node {
                if (*q).comp_idx == (*node).comp_idx {
                    (*q).copy_quant_from(&*node);
                    (*p).next = ptr::null_mut();
                    (*node).next = self.avail;
                    self.avail = node;
                    return;
                }
                q = (*q).next;
            }
        }
    }

    pub fn set_delta_for_comp(&mut self, comp_idx: Ui32, delta: f32) {
        debug_assert!(self.type_ == QcdType::QcdMain);
        let mut p = self.get_qcc(comp_idx);
        if p.is_null() {
            p = self.add_qcc_object(comp_idx);
        }
        unsafe {
            (*p).set_delta(delta);
        }
    }

    /// Port target: `https://github.com/aous72/OpenJPH/blob/v0.31.0/src/core/codestream/ojph_params.cpp`
    /// `param_qcd::check_validity`. Replaces the 0.26.3 `all_same`/`first_comp` logic with the
    /// `make_quant_steps`/`is_qcc_needed` refactor that also drives the qfactor feature.
    ///
    /// For the default configuration (no COC/QCC overrides, no qfactor) this is algebraically
    /// identical to the previous logic: both pick component 0, use the main COD's decomposition
    /// count/wavelet kernel and component 0's bit depth/signedness, and `is_color_trans` (new in
    /// this comparison) is the same for every component under a single COD. Verified by the
    /// existing byte-identity tests, which pass both before and after this port.
    pub fn check_validity(
        &mut self,
        siz: &ParamSiz,
        cod: &ParamCod,
    ) -> crate::openjph::ojph_message::OjphResult<()> {
        debug_assert!(self.type_ == QcdType::QcdMain);

        let num_comps = siz.get_num_components() as Ui32;
        self.trim_non_existing_components(num_comps);

        // Initialize QCD based on the first component that is (a) associated with COD and (b)
        // does not have a COC, or the first component otherwise.
        let mut qcd_comp = 0u32;
        for c in 0..num_comps {
            let coc = cod.get_coc(c);
            let qcc = self.get_qcc(c);
            if core::ptr::eq(coc as *const ParamCod, cod as *const ParamCod)
                && core::ptr::eq(qcc as *const ParamQcd, self as *const ParamQcd)
            {
                qcd_comp = c;
                break;
            }
        }

        // Check if only the top QCD has qfactor set, if so, check if any of the first component
        // has COC and qfactor set properly.
        if self.qfactor != QFACTOR_UNSET {
            if num_comps < 3 {
                // one or two components
                for i in 0..num_comps {
                    let mut q = self.get_qcc(i);
                    if core::ptr::eq(q as *const ParamQcd, self as *const ParamQcd) {
                        q = self.add_qcc_object(i);
                        let _ = q;
                        self.set_qfactor_comp(i, CompType::Y, self.qfactor)?;
                    }
                }
            } else {
                for i in 0..num_comps {
                    let mut q = self.get_qcc(i);
                    if core::ptr::eq(q as *const ParamQcd, self as *const ParamQcd) {
                        q = self.add_qcc_object(i);
                        let _ = q;
                        let ci = if i < 3 { i as Ui8 } else { 0u8 };
                        let t = CompType::from_u8(ci);
                        self.set_qfactor_comp(i, t, self.qfactor)?;
                    }
                }
            }
        }

        self.make_quant_steps(qcd_comp, cod, siz)?;

        // Initialize every QCC, creating one for every component that (a) cannot use QCD and
        // (b) does not already have a QCC. NOTE: Qfactor always creates a QCC and QCD cannot be
        // reused.
        for c in 0..num_comps {
            let mut qcc = self.get_qcc(c);
            let coc = cod.get_coc(c);

            if core::ptr::eq(qcc as *const ParamQcd, self as *const ParamQcd) {
                let needed = unsafe { self.is_qcc_needed(c, &*coc, siz)? };
                if !needed {
                    continue;
                }
                qcc = self.add_qcc_object(c);
                unsafe {
                    (*qcc).set_delta(self.base_delta);
                }
            }

            unsafe {
                (*qcc).make_quant_steps(c, &*coc, siz)?;
            }
        }
        Ok(())
    }

    /// Port of `param_qcd::make_quant_steps`.
    fn make_quant_steps(
        &mut self,
        comp_num: Ui32,
        cod: &ParamCod,
        siz: &ParamSiz,
    ) -> crate::openjph::ojph_message::OjphResult<()> {
        if self.is_init {
            return Err(crate::ojph_error!(0x00040001, "Quantization step sizes already initialized."));
        }
        self.is_init = true;

        self.num_decomps = cod.get_num_decompositions() as Ui32;
        self.bit_depth = siz.get_bit_depth(comp_num);
        self.is_signed = siz.is_signed(comp_num);
        self.is_color_trans = cod.is_employing_color_transform();
        self.wavelet_kern = cod.get_wavelet_kern() as Ui32;
        self.sampling = siz.get_downsampling(comp_num);
        self.num_subbands = 1 + 3 * self.num_decomps;

        if self.wavelet_kern == ParamCod::DWT_REV53 as Ui32 {
            self.set_rev_quant(
                self.num_decomps,
                self.bit_depth,
                if comp_num < 3 {
                    self.is_color_trans
                } else {
                    false
                },
            )?;
        } else if self.wavelet_kern == ParamCod::DWT_IRV97 as Ui32 {
            if self.base_delta == -1.0 {
                let t = ojph_min(16, self.bit_depth);
                self.base_delta = 1.0 / ((1u32 << t) as f32);
            } else if self.qfactor != QFACTOR_UNSET {
                crate::ojph_warn!(
                    0x00040002,
                    "qstep for component {} is ignored, because qfactor is set.",
                    comp_num
                );
            }
            self.set_irrev_quant(self.num_decomps)?;
        }
        Ok(())
    }

    /// Port of `param_qcd::is_qcc_needed`.
    fn is_qcc_needed(
        &self,
        comp_num: Ui32,
        cod: &ParamCod,
        siz: &ParamSiz,
    ) -> crate::openjph::ojph_message::OjphResult<bool> {
        if !self.is_init {
            return Err(crate::ojph_error!(0x00040001, "Quantization step sizes not initialized."));
        }
        Ok(self.num_decomps != cod.get_num_decompositions() as Ui32
            || self.bit_depth != siz.get_bit_depth(comp_num)
            || self.is_signed != siz.is_signed(comp_num)
            || self.is_color_trans != cod.is_employing_color_transform()
            || self.wavelet_kern != cod.get_wavelet_kern() as Ui32)
    }

    /// Port of `ojph::param_qcd::set_qfactor(ui8)` / `local::param_qcd::set_qfactor(ui8)`. Sets
    /// the top-level qfactor; component-level QCC expansion happens later, in `check_validity`
    /// (matching upstream: this call alone does not create any QCCs).
    pub fn set_qfactor(&mut self, qfactor: Ui8) -> crate::openjph::ojph_message::OjphResult<()> {
        debug_assert!(self.type_ == QcdType::QcdMain);
        if !(1..=100).contains(&qfactor) {
            return Err(crate::ojph_error!(
                0x00050181,
                "Qfactor must be between 1 and 100, but was set to {}.",
                qfactor
            ));
        }
        self.qfactor = qfactor;
        Ok(())
    }

    /// Port of `param_qcd::set_qfactor(ui32 comp_idx, comp_type ctype, ui8 qfactor)`. Named
    /// distinctly from `set_qfactor` above since Rust has no overloading.
    pub fn set_qfactor_comp(
        &mut self,
        comp_idx: Ui32,
        ctype: CompType,
        qfactor: Ui8,
    ) -> crate::openjph::ojph_message::OjphResult<()> {
        debug_assert!(self.type_ == QcdType::QcdMain);
        if !(1..=100).contains(&qfactor) {
            return Err(crate::ojph_error!(
                0x00050191,
                "Qfactor must be between 1 and 100, but was set to {}.",
                qfactor
            ));
        }
        let mut p = self.get_qcc(comp_idx);
        if core::ptr::eq(p as *const ParamQcd, self as *const ParamQcd) {
            p = self.add_qcc_object(comp_idx);
        }
        unsafe {
            (*p).qfactor = qfactor;
            (*p).ctype = ctype;
        }
        Ok(())
    }

    pub fn get_num_guard_bits(&self) -> Ui32 {
        (self.Sqcd >> 5) as Ui32
    }

    pub fn get_MAGB(&self) -> Ui32 {
        let mut b = 0u32;
        let mut p = self as *const ParamQcd;
        unsafe {
            while !p.is_null() {
                let num_decomps = ((*p).num_subbands - 1) / 3;
                let irrev = (*p).Sqcd & 0x1F;
                if irrev == 0 {
                    for i in 0..(*p).num_subbands as usize {
                        let mut t = (*p).decode_SPqcd((*p).SPqcd.u8_[i]) as Ui32;
                        t += (*p).get_num_guard_bits() - 1;
                        if t > b {
                            b = t;
                        }
                    }
                } else if irrev == 2 {
                    for i in 0..(*p).num_subbands {
                        let nb = num_decomps - if i != 0 { (i - 1) / 3 } else { 0 };
                        let t = ((*p).SPqcd.u16_[i as usize] >> 11) as Ui32
                            + (*p).get_num_guard_bits()
                            - nb;
                        if t > b {
                            b = t;
                        }
                    }
                } else if irrev == 1 {
                    // Scalar-derived: only the LL band (idx 0) is signalled; the per-band exponent
                    // only shrinks for deeper bands, so the LL bounds MAGB. nb = num_decomps here.
                    let t = ((*p).SPqcd.u16_[0] >> 11) as Ui32 + (*p).get_num_guard_bits()
                        - num_decomps;
                    if t > b {
                        b = t;
                    }
                } else {
                    debug_assert!(false);
                }
                p = (*p).next;
            }
        }
        b
    }

    pub fn get_comp_idx(&self) -> Ui16 {
        self.comp_idx
    }

    pub fn get_irrev_delta(
        &self,
        dfs: *const ParamDfs,
        num_decompositions: Ui32,
        resolution: Ui32,
        subband: Ui32,
    ) -> crate::openjph::ojph_message::OjphResult<f32> {
        let arr = [1.0f32, 2.0f32, 2.0f32, 4.0f32];
        // OpenJPH v0.31.0 hardens this from a bare assert to a hard runtime error (it only ever
        // supports expounded, mode 2). jph-rs also supports derived (mode 1) as its own extension
        // beyond OpenJPH (`derived_SPqcd` below) — keep that, but promote the invalid-mode case
        // (neither 1 nor 2, i.e. a codestream claiming irreversible with reversible/no-quant step
        // sizes) from a debug-only assert to a real error, matching upstream's hardening intent.
        if !matches!(self.Sqcd & 0x1F, 1 | 2) {
            return Err(crate::ojph_error!(
                0x00050101,
                "There is something wrong in the configuration of the codestream; the codestream defines an irreversible transform, for which the codestream provides reversible (no quantization) step sizes in Sqcd/Sqcc."
            ));
        }

        let idx = if !dfs.is_null() && unsafe { (*dfs).exists() } {
            unsafe { (*dfs).get_subband_idx(num_decompositions, resolution, subband) }
        } else if resolution != 0 {
            (resolution - 1) * 3 + subband
        } else {
            0
        };
        // Scalar-derived quantization synthesizes the SPqcd word for this band from the LL value;
        // expounded reads it directly (clamping to the signalled count, as OpenJPH does).
        let sp = if (self.Sqcd & 0x1F) == 1 {
            self.derived_SPqcd(idx)
        } else {
            let mut i = idx;
            if i >= self.num_subbands {
                crate::ojph_info!(
                    0x00050102,
                    "Trying to access quantization step size for subband {} when the QCD/QCC marker segment specifies quantization step sizes for {} subbands only.  To continue decoding, we are using the step size for subband {}, which can produce incorrect results",
                    i + 1,
                    self.num_subbands,
                    self.num_subbands - 1
                );
                i = self.num_subbands - 1;
            }
            unsafe { self.SPqcd.u16_[i as usize] }
        };
        let eps = sp >> 11;
        let mut mantissa = (((sp & 0x7FF) | 0x800) as f32) * arr[subband as usize];
        mantissa /= (1 << 11) as f32;
        mantissa /= (1u32 << eps) as f32;
        Ok(mantissa)
    }

    pub fn propose_precision(&self, cod: *const ParamCod) -> Ui32 {
        let comp_idx = unsafe { (*cod).get_comp_idx() };
        let main = unsafe { (*cod).get_coc(ParamCod::OJPH_COD_DEFAULT as Ui32) };
        let mut precision = 0u32;
        unsafe {
            if (*main).is_employing_color_transform() && comp_idx < 3 {
                for i in 0..3u32 {
                    let p = self.get_qcc(i);
                    let t = (*p).get_largest_kmax();
                    if t > precision {
                        precision = t;
                    }
                }
            } else {
                precision = self.get_largest_kmax();
            }
        }
        precision + 1 + 1
    }

    /// Effective (expounded) SPqcd word for subband index `idx` under scalar-derived quantization
    /// (`Sqcd & 0x1F == 1`): only `SPqcd[0]` (the LL band) is signalled and the rest derive per
    /// ITU-T T.800 E.1.1 / OpenJPEG `j2k.c`: `expn_idx = max(0, expn_0 - (idx-1)/3)`, mantissa
    /// unchanged. `idx` is the band index `(resolution-1)*3 + subband` (0 = LL).
    fn derived_SPqcd(&self, idx: Ui32) -> Ui16 {
        let v0 = unsafe { self.SPqcd.u16_[0] };
        let e0 = (v0 >> 11) as Ui32;
        let reduction = if idx > 0 { (idx - 1) / 3 } else { 0 };
        let eb = e0.saturating_sub(reduction) as Ui16;
        (eb << 11) | (v0 & 0x7FF)
    }

    pub fn get_kmax(
        &self,
        dfs: *const ParamDfs,
        num_decompositions: Ui32,
        resolution: Ui32,
        subband: Ui32,
    ) -> Ui32 {
        let mut idx = if !dfs.is_null() && unsafe { (*dfs).exists() } {
            unsafe { (*dfs).get_subband_idx(num_decompositions, resolution, subband) }
        } else if resolution != 0 {
            (resolution - 1) * 3 + subband
        } else {
            0
        };
        let irrev = self.Sqcd & 0x1F;
        if irrev == 1 {
            // Derived: synthesize this band's exponent from the LL value; the single signalled
            // SPqcd entry has no per-band bound to clamp against.
            let e = (self.derived_SPqcd(idx) >> 11) as Ui32;
            let num_bits = if e == 0 { 0 } else { e - 1 };
            return num_bits + self.get_num_guard_bits();
        }
        if idx >= self.num_subbands {
            crate::ojph_info!(
                0x00050111,
                "Trying to access quantization step size for subband {} when the QCD/QCC marker segment specifies quantization step sizes for {} subbands only.  To continue decoding, we are using the step size for subband {}, which can produce incorrect results",
                idx + 1,
                self.num_subbands,
                self.num_subbands - 1
            );
            idx = self.num_subbands - 1;
        }

        let num_bits = if irrev == 0 {
            let t = self.decode_SPqcd(unsafe { self.SPqcd.u8_[idx as usize] }) as Ui32;
            if t == 0 { 0 } else { t - 1 }
        } else if irrev == 2 {
            // Exponent 0 is legal (step >= 1, e.g. Qfactor high bands). C++ computes `e - 1 + G` in
            // u32 and relies on wraparound; add the guard bits first so the result is identical
            // whenever `e + G >= 1` without an underflow panic in debug builds.
            let e = (unsafe { self.SPqcd.u16_[idx as usize] } >> 11) as Ui32;
            return (e + self.get_num_guard_bits()).saturating_sub(1);
        } else {
            debug_assert!(false);
            0
        };
        num_bits + self.get_num_guard_bits()
    }

    pub fn write(&mut self, file: &mut dyn OutfileBase) -> crate::openjph::ojph_message::OjphResult<bool> {
        let irrev = self.Sqcd & 0x1F;
        self.Lqcd = 3;
        if irrev == 0 {
            self.Lqcd = (self.Lqcd + self.num_subbands as Ui16) as Ui16;
        } else if irrev == 2 {
            self.Lqcd = (self.Lqcd + 2 * self.num_subbands as Ui16) as Ui16;
        } else {
            debug_assert!(false);
        }

        let mut result = true;
        result &= write_u16(file, swap_byte_u16(Jp2kMarker::Qcd as Ui16))?;
        result &= write_u16(file, swap_byte_u16(self.Lqcd))?;
        result &= file.write(&[self.Sqcd])? == 1;
        if irrev == 0 {
            for i in 0..self.num_subbands as usize {
                result &= file.write(&[unsafe { self.SPqcd.u8_[i] }])? == 1;
            }
        } else if irrev == 2 {
            for i in 0..self.num_subbands as usize {
                result &= write_u16(file, swap_byte_u16(unsafe { self.SPqcd.u16_[i] }))?;
            }
        } else {
            debug_assert!(false);
        }
        Ok(result)
    }

    pub fn write_qcc(
        &mut self,
        file: &mut dyn OutfileBase,
        num_comps: Ui32,
    ) -> crate::openjph::ojph_message::OjphResult<bool> {
        debug_assert!(self.type_ == QcdType::QcdMain);
        let mut result = true;
        let mut p = self.next;
        unsafe {
            while !p.is_null() {
                if (*p).enabled {
                    result &= (*p).internal_write_qcc(file, num_comps)?;
                }
                p = (*p).next;
            }
        }
        Ok(result)
    }

    pub fn read(&mut self, file: &mut dyn InfileBase) -> crate::openjph::ojph_message::OjphResult<()> {
        if !read_u16(file, &mut self.Lqcd) {
            return Err(crate::ojph_error!(0x00050081, "error reading QCD marker"));
        }
        self.Lqcd = swap_byte_u16(self.Lqcd);
        if !read_u8(file, &mut self.Sqcd) {
            return Err(crate::ojph_error!(0x00050082, "error reading QCD marker"));
        }

        if (self.Sqcd & 0x1F) == 0 {
            self.num_subbands = (self.Lqcd - 3) as Ui32;
            if self.num_subbands == 0 {
                return Err(crate::ojph_error!(
                    0x0005008A,
                    "QCD marker segment that specifies no quantization informtion"
                ));
            }
            if self.num_subbands > 97 || self.Lqcd as Ui32 != 3 + self.num_subbands {
                return Err(crate::ojph_error!(
                    0x00050083,
                    "wrong Lqcd value of {} in QCD marker",
                    self.Lqcd
                ));
            }
            for i in 0..self.num_subbands as usize {
                if !read_u8(file, unsafe { &mut self.SPqcd.u8_[i] }) {
                    return Err(crate::ojph_error!(0x00050084, "error reading QCD marker"));
                }
            }
        } else if (self.Sqcd & 0x1F) == 1 {
            // Scalar derived quantization (ITU-T T.800 E.1.1): a single SPqcd word is signalled for
            // the LL (NLLL) band; every other subband's step size is derived on the fly from it
            // (see `derived_SPqcd`, `get_kmax`, `get_irrev_delta`). Lqcd = 2 + 1 + 2.
            if self.Lqcd != 5 {
                return Err(crate::ojph_error!(0x00050085, "wrong Lqcd value in QCD marker"));
            }
            self.num_subbands = 1;
            if !read_u16(file, unsafe { &mut self.SPqcd.u16_[0] }) {
                return Err(crate::ojph_error!(0x00050084, "error reading QCD marker"));
            }
            unsafe {
                self.SPqcd.u16_[0] = swap_byte_u16(self.SPqcd.u16_[0]);
            }
        } else if (self.Sqcd & 0x1F) == 2 {
            self.num_subbands = ((self.Lqcd - 3) / 2) as Ui32;
            if self.num_subbands == 0 {
                return Err(crate::ojph_error!(
                    0x0005008B,
                    "QCD marker segment that specifies no quantization informtion"
                ));
            }
            if self.num_subbands > 97 || self.Lqcd as Ui32 != 3 + 2 * self.num_subbands {
                return Err(crate::ojph_error!(
                    0x00050086,
                    "wrong Lqcd value of {} in QCD marker",
                    self.Lqcd
                ));
            }
            for i in 0..self.num_subbands as usize {
                if !read_u16(file, unsafe { &mut self.SPqcd.u16_[i] }) {
                    return Err(crate::ojph_error!(0x00050087, "error reading QCD marker"));
                }
                unsafe {
                    self.SPqcd.u16_[i] = swap_byte_u16(self.SPqcd.u16_[i]);
                }
            }
        } else {
            return Err(crate::ojph_error!(0x00050088, "wrong Sqcd value in QCD marker"));
        }
        Ok(())
    }

    pub fn read_qcc(
        &mut self,
        file: &mut dyn InfileBase,
        num_comps: Ui32,
    ) -> crate::openjph::ojph_message::OjphResult<()> {
        if !read_u16(file, &mut self.Lqcd) {
            return Err(crate::ojph_error!(0x000500A1, "error reading QCC marker"));
        }
        self.Lqcd = swap_byte_u16(self.Lqcd);
        if num_comps < 257 {
            let mut v = 0u8;
            if !read_u8(file, &mut v) {
                return Err(crate::ojph_error!(0x000500A2, "error reading QCC marker"));
            }
            self.comp_idx = v as Ui16;
        } else {
            if !read_u16(file, &mut self.comp_idx) {
                return Err(crate::ojph_error!(0x000500A3, "error reading QCC marker"));
            }
            self.comp_idx = swap_byte_u16(self.comp_idx);
        }
        if u32::from(self.comp_idx) >= num_comps {
            return Err(crate::ojph_error!(
                0x00050200,
                "QCC marker for component {} but the image has only {} components",
                self.comp_idx,
                num_comps
            ));
        }
        if !read_u8(file, &mut self.Sqcd) {
            return Err(crate::ojph_error!(0x000500A4, "error reading QCC marker"));
        }
        let offset = if num_comps < 257 { 4 } else { 5 };
        if (self.Sqcd & 0x1F) == 0 {
            self.num_subbands = (self.Lqcd as Ui32).wrapping_sub(offset);
            if self.num_subbands == 0 {
                return Err(crate::ojph_error!(
                    0x000500AC,
                    "QCC marker segment that specifies no quantization informtion"
                ));
            }
            if self.num_subbands > 97 || self.Lqcd as Ui32 != offset + self.num_subbands {
                return Err(crate::ojph_error!(
                    0x000500A5,
                    "wrong Lqcd value of {} in QCC marker",
                    self.Lqcd
                ));
            }
            for i in 0..self.num_subbands as usize {
                if !read_u8(file, unsafe { &mut self.SPqcd.u8_[i] }) {
                    return Err(crate::ojph_error!(0x000500A6, "error reading QCC marker"));
                }
            }
        } else if (self.Sqcd & 0x1F) == 1 {
            // Scalar derived quantization (see the QCD path above): one SPqcd word for the LL band.
            // Lqcd = offset (length + component + Sqcd) + 2 (the single SPqcd u16).
            if self.Lqcd as Ui32 != offset + 2 {
                return Err(crate::ojph_error!(0x000500A7, "wrong Lqcc value in QCC marker"));
            }
            self.num_subbands = 1;
            if !read_u16(file, unsafe { &mut self.SPqcd.u16_[0] }) {
                return Err(crate::ojph_error!(0x000500A6, "error reading QCC marker"));
            }
            unsafe {
                self.SPqcd.u16_[0] = swap_byte_u16(self.SPqcd.u16_[0]);
            }
        } else if (self.Sqcd & 0x1F) == 2 {
            self.num_subbands = ((self.Lqcd as Ui32).wrapping_sub(offset)) / 2;
            if self.num_subbands == 0 {
                return Err(crate::ojph_error!(
                    0x000500AD,
                    "QCC marker segment that specifies no quantization informtion"
                ));
            }
            if self.num_subbands > 97 || self.Lqcd as Ui32 != offset + 2 * self.num_subbands {
                return Err(crate::ojph_error!(
                    0x000500A8,
                    "wrong Lqcc value of {} in QCC marker",
                    self.Lqcd
                ));
            }
            for i in 0..self.num_subbands as usize {
                if !read_u16(file, unsafe { &mut self.SPqcd.u16_[i] }) {
                    return Err(crate::ojph_error!(0x000500A9, "error reading QCC marker"));
                }
                unsafe {
                    self.SPqcd.u16_[i] = swap_byte_u16(self.SPqcd.u16_[i]);
                }
            }
        } else {
            return Err(crate::ojph_error!(0x000500AA, "wrong Sqcc value in QCC marker"));
        }
        Ok(())
    }

    pub fn get_qcc(&self, comp_idx: Ui32) -> *mut ParamQcd {
        let (mut p, q) = if self.type_ == QcdType::QcdMain {
            (
                self as *const ParamQcd as *mut ParamQcd,
                self as *const ParamQcd as *mut ParamQcd,
            )
        } else {
            (self.top_qcd, self.top_qcd)
        };
        unsafe {
            while !p.is_null() && (*p).comp_idx != comp_idx as Ui16 {
                p = (*p).next;
            }
        }
        if p.is_null() { q } else { p }
    }

    pub fn add_qcc_object(&mut self, comp_idx: Ui32) -> *mut ParamQcd {
        debug_assert!(self.type_ == QcdType::QcdMain);
        let mut p = self as *mut ParamQcd;
        unsafe {
            while !(*p).next.is_null() {
                p = (*p).next;
            }
            if !self.avail.is_null() {
                (*p).next = self.avail;
                self.avail = (*self.avail).next;
                (*(*p).next).init(self, comp_idx as Ui16);
            } else {
                let mut node = Box::new(ParamQcd::default());
                node.init(self, comp_idx as Ui16);
                (*p).next = Box::into_raw(node);
            }
            (*p).next
        }
    }

    fn set_rev_quant(
        &mut self,
        num_decomps: Ui32,
        bit_depth: Ui32,
        is_employing_color_transform: bool,
    ) -> crate::openjph::ojph_message::OjphResult<()> {
        let mut b = bit_depth;
        if is_employing_color_transform {
            b += 1;
        }
        let mut s = 0usize;
        let bibo_l = bibo_gain_l(num_decomps, true) as f64;
        let mut x = ((bibo_l * bibo_l).ln() / LN_2).ceil() as Ui32;
        unsafe {
            self.SPqcd.u8_[s] = (b + x) as Ui8;
        }
        s += 1;
        let mut max_b_plus_x = b + x;
        for d in (1..=num_decomps).rev() {
            let bibo_l = bibo_gain_l(d, true) as f64;
            let bibo_h = bibo_gain_h(d - 1, true) as f64;
            x = ((bibo_h * bibo_l).ln() / LN_2).ceil() as Ui32;
            unsafe {
                self.SPqcd.u8_[s] = (b + x) as Ui8;
            }
            if b + x > max_b_plus_x {
                max_b_plus_x = b + x;
            }
            s += 1;
            unsafe {
                self.SPqcd.u8_[s] = (b + x) as Ui8;
            }
            if b + x > max_b_plus_x {
                max_b_plus_x = b + x;
            }
            s += 1;
            x = ((bibo_h * bibo_h).ln() / LN_2).ceil() as Ui32;
            unsafe {
                self.SPqcd.u8_[s] = (b + x) as Ui8;
            }
            if b + x > max_b_plus_x {
                max_b_plus_x = b + x;
            }
            s += 1;
        }

        if max_b_plus_x > 38 {
            return Err(crate::ojph_error!(
                0x00050151,
                "The specified combination of bit_depth, colour transform, and type of wavelet transform requires more than 38 bits; it requires {} bits. This is beyond what is allowed in the JPEG2000 image coding format.",
                max_b_plus_x
            ));
        }

        let guard_bits = if max_b_plus_x > 32 {
            (max_b_plus_x as Si32) - 31
        } else {
            1
        };
        self.Sqcd = (guard_bits as Ui8) << 5;
        s = 0;
        unsafe {
            self.SPqcd.u8_[s] = self.encode_SPqcd(self.SPqcd.u8_[s] - guard_bits as Ui8);
        }
        s += 1;
        for _ in (1..=num_decomps).rev() {
            unsafe {
                self.SPqcd.u8_[s] = self.encode_SPqcd(self.SPqcd.u8_[s] - guard_bits as Ui8);
            }
            s += 1;
            unsafe {
                self.SPqcd.u8_[s] = self.encode_SPqcd(self.SPqcd.u8_[s] - guard_bits as Ui8);
            }
            s += 1;
            unsafe {
                self.SPqcd.u8_[s] = self.encode_SPqcd(self.SPqcd.u8_[s] - guard_bits as Ui8);
            }
            s += 1;
        }
        Ok(())
    }

    /// Port target: `https://github.com/aous72/OpenJPH/blob/v0.31.0/src/core/codestream/ojph_params.cpp`
    /// `param_qcd::set_irrev_quant`. When `qfactor == QFACTOR_UNSET` (the only path exercised
    /// before this port existed), `weights` is all-1.0 and `g_c`/`power` are both `1.0`, so
    /// `w_b.powf(power) == 1.0` always and this produces byte-identical `SPqcd` words to the
    /// pre-0.31.0 inlined exp/mantissa loop it replaces (verified: this is exactly why the golden
    /// HTJ2K encode tests already passed byte-identical before qfactor was ported at all).
    fn set_irrev_quant(&mut self, num_decomps: Ui32) -> crate::openjph::ojph_message::OjphResult<()> {
        let guard_bits = 1u8;
        self.Sqcd = (guard_bits << 5) | 0x2; // one guard bit, scalar quantization

        let mut g_c = 1.0f32;
        let mut delta_ref = self.base_delta;
        let mut power = 1.0f32;
        let mut weights: &[f32; 19] = visual_weights::get_no_weights();

        if self.qfactor != QFACTOR_UNSET {
            let format = visual_weights::get_format(self.sampling);
            if format == visual_weights::ColourFormat::FormatError {
                return Err(crate::ojph_error!(
                    0x00050161,
                    "Qfactor can only be used on components with 4:4:4, 4:2:2 or 4:2:0 sampling"
                ));
            }
            if self.ctype == CompType::Y && self.sampling.x != 1 && self.sampling.y != 1 {
                return Err(crate::ojph_error!(
                    0x00050162,
                    "Qfactor can only be used for a Y or luminance component when it is not downsampled."
                ));
            }

            g_c = visual_weights::get_gain(self.ctype);
            delta_ref = visual_weights::get_delta_ref(self.qfactor as Ui32, self.bit_depth, &mut power);
            weights = visual_weights::get_weights(format, self.ctype);
        }

        // LL band
        let mut b: Ui32 = 0;
        let gain_l = sqrt_energy_gain_l(num_decomps, false);
        let mut w_b = visual_weights::get_weight(weights, num_decomps, b);
        w_b = w_b.powf(power);
        self.encode_irv_spqcd(b, delta_ref / (gain_l * gain_l * g_c * w_b));
        b += 1;

        // LL, HL, LH, HH, HL, LH, HH...
        for d in (1..=num_decomps).rev() {
            let gain_l = sqrt_energy_gain_l(d, false);
            let gain_h = sqrt_energy_gain_h(d - 1, false);

            let mut w_b = visual_weights::get_weight(weights, d, 1);
            w_b = w_b.powf(power);
            self.encode_irv_spqcd(b, delta_ref / (gain_h * gain_l * g_c * w_b));
            b += 1;
            let mut w_b = visual_weights::get_weight(weights, d, 2);
            w_b = w_b.powf(power);
            self.encode_irv_spqcd(b, delta_ref / (gain_l * gain_h * g_c * w_b));
            b += 1;
            let mut w_b = visual_weights::get_weight(weights, d, 3);
            w_b = w_b.powf(power);
            self.encode_irv_spqcd(b, delta_ref / (gain_h * gain_h * g_c * w_b));
            b += 1;
        }
        Ok(())
    }

    /// Port of `param_qcd::encode_SPqcd(ui32 subband_index, float delta)`. Named distinctly from
    /// the existing `encode_SPqcd`/`decode_SPqcd` (reversible, `ui8 -> ui8`) since Rust has no
    /// function overloading; C++ genuinely has both under the same name.
    fn encode_irv_spqcd(&mut self, subband_index: Ui32, mut delta: f32) {
        let mut exp = 0i32;
        while delta < 1.0 {
            exp += 1;
            delta *= 2.0;
        }
        let mut mantissa = (delta * (1 << 11) as f32).round() as i32 - (1 << 11);
        // With rounding, there is a risk that the mantissa becomes equal to 1<<11.
        mantissa = if mantissa < (1 << 11) { mantissa } else { 0x7FF };
        unsafe {
            self.SPqcd.u16_[subband_index as usize] = ((exp << 11) | mantissa) as Ui16;
        }
    }

    fn get_largest_kmax(&self) -> Ui32 {
        let irrev = self.Sqcd & 0x1F;
        let mut num_bits = 0u32;
        if irrev == 0 {
            for i in 0..self.num_subbands as usize {
                let t = self.decode_SPqcd(unsafe { self.SPqcd.u8_[i] }) as Ui32;
                let t = if t == 0 { 0 } else { t - 1 };
                if t > num_bits {
                    num_bits = t;
                }
            }
        } else if irrev == 1 {
            // Scalar-derived: the LL band (idx 0) carries the largest exponent.
            let e = (unsafe { self.SPqcd.u16_[0] } >> 11) as Ui32;
            num_bits = if e == 0 { 0 } else { e - 1 };
        } else if irrev == 2 {
            // Largest exponent over all bands, then `e - 1 + G` (see `get_kmax`). C++ maxes the
            // u32-wrapped `e - 1`, so a single exponent-0 band would hide every larger band and
            // understate the precision; taking the max of `e` first is correct for all inputs.
            let max_e = (0..self.num_subbands as usize)
                .map(|i| (unsafe { self.SPqcd.u16_[i] } >> 11) as Ui32)
                .max()
                .unwrap_or(0);
            return (max_e + self.get_num_guard_bits()).saturating_sub(1);
        } else {
            debug_assert!(false);
        }
        num_bits + self.get_num_guard_bits()
    }

    fn internal_write_qcc(
        &mut self,
        file: &mut dyn OutfileBase,
        num_comps: Ui32,
    ) -> crate::openjph::ojph_message::OjphResult<bool> {
        let irrev = self.Sqcd & 0x1F;
        self.Lqcd = (4 + if num_comps < 257 { 0 } else { 1 }) as Ui16;
        if irrev == 0 {
            self.Lqcd = (self.Lqcd + self.num_subbands as Ui16) as Ui16;
        } else if irrev == 2 {
            self.Lqcd = (self.Lqcd + 2 * self.num_subbands as Ui16) as Ui16;
        } else {
            debug_assert!(false);
        }

        let mut result = true;
        result &= write_u16(file, swap_byte_u16(Jp2kMarker::Qcc as Ui16))?;
        result &= write_u16(file, swap_byte_u16(self.Lqcd))?;
        if num_comps < 257 {
            result &= file.write(&[self.comp_idx as Ui8])? == 1;
        } else {
            result &= write_u16(file, swap_byte_u16(self.comp_idx))?;
        }
        result &= file.write(&[self.Sqcd])? == 1;
        if irrev == 0 {
            for i in 0..self.num_subbands as usize {
                result &= file.write(&[unsafe { self.SPqcd.u8_[i] }])? == 1;
            }
        } else if irrev == 2 {
            for i in 0..self.num_subbands as usize {
                result &= write_u16(file, swap_byte_u16(unsafe { self.SPqcd.u16_[i] }))?;
            }
        } else {
            debug_assert!(false);
        }
        Ok(result)
    }

    fn trim_non_existing_components(&mut self, num_comps: Ui32) {
        debug_assert!(self.type_ == QcdType::QcdMain && self.comp_idx == Self::OJPH_QCD_DEFAULT);
        let mut p = self.next;
        unsafe {
            while !p.is_null() {
                debug_assert!((*p).type_ == QcdType::QccMain);
                (*p).enabled = (*p).comp_idx < num_comps as Ui16;
                p = (*p).next;
            }
        }
    }

    #[inline]
    fn decode_SPqcd(&self, v: Ui8) -> Ui8 {
        v >> 3
    }

    #[inline]
    fn encode_SPqcd(&self, v: Ui8) -> Ui8 {
        v << 3
    }
}

#[allow(non_snake_case)]
pub struct ParamNlt {
    Lnlt: Ui16,
    Cnlt: Ui16,
    BDnlt: Ui8,
    Tnlt: Ui8,
    enabled: bool,
    next: *mut ParamNlt,
    avail: *mut ParamNlt,
}

impl Default for ParamNlt {
    fn default() -> Self {
        let mut value = Self {
            Lnlt: 0,
            Cnlt: 0,
            BDnlt: 0,
            Tnlt: Nonlinearity::OjphNltUndefined as Ui8,
            enabled: false,
            next: ptr::null_mut(),
            avail: ptr::null_mut(),
        };
        value.init();
        value
    }
}

impl Drop for ParamNlt {
    fn drop(&mut self) {
        unsafe {
            if !self.avail.is_null() {
                drop(Box::from_raw(self.avail));
                self.avail = ptr::null_mut();
            }
            if !self.next.is_null() {
                drop(Box::from_raw(self.next));
                self.next = ptr::null_mut();
            }
        }
    }
}

#[allow(non_snake_case)]
impl ParamNlt {
    fn init(&mut self) {
        self.Lnlt = 6;
        self.Cnlt = ALL_COMPS;
        self.BDnlt = 0;
        self.Tnlt = Nonlinearity::OjphNltUndefined as Ui8;
        self.enabled = false;
        self.next = ptr::null_mut();
    }

    pub fn restart(&mut self) {
        unsafe {
            let mut p = &mut self.avail as *mut *mut ParamNlt;
            while !(*p).is_null() {
                p = &mut (**p).next as *mut *mut ParamNlt;
            }
            *p = self.next;
        }
        self.init();
    }

    pub fn check_validity(&mut self, siz: &mut ParamSiz) {
        if !self.is_any_enabled() {
            return;
        }

        if self.enabled && self.Tnlt == Nonlinearity::OjphNltNoNlt as Ui8 {
            self.enabled = false;
        }

        if self.enabled && self.Tnlt == Nonlinearity::OjphNltBinaryComplementNlt as Ui8 {
            let mut all_same = true;
            let num_comps = siz.get_num_components() as Ui32;
            let mut bit_depth = 0u32;
            let mut is_signed = false;
            for c in 0..num_comps {
                let p = self.get_nlt_object(c);
                if p.is_null() || unsafe { !(*p).enabled } {
                    if bit_depth != 0 {
                        all_same = all_same && (bit_depth == siz.get_bit_depth(c));
                        all_same = all_same && (is_signed == siz.is_signed(c));
                    } else {
                        bit_depth = siz.get_bit_depth(c);
                        is_signed = siz.is_signed(c);
                    }
                } else {
                    unsafe {
                        (*p).BDnlt = (siz.get_bit_depth(c) - 1) as Ui8;
                        (*p).BDnlt |= if siz.is_signed(c) { 0x80 } else { 0 };
                    }
                }
            }

            if all_same && bit_depth != 0 {
                self.BDnlt = (bit_depth - 1) as Ui8;
                self.BDnlt |= if is_signed { 0x80 } else { 0 };
            } else if !all_same {
                self.enabled = false;
                for c in 0..num_comps {
                    let mut p = self.get_nlt_object(c);
                    if p.is_null() || unsafe { !(*p).enabled } {
                        if p.is_null() {
                            p = self.add_object(c);
                        }
                        unsafe {
                            (*p).enabled = true;
                            (*p).Tnlt = Nonlinearity::OjphNltBinaryComplementNlt as Ui8;
                            (*p).BDnlt = (siz.get_bit_depth(c) - 1) as Ui8;
                            (*p).BDnlt |= if siz.is_signed(c) { 0x80 } else { 0 };
                        }
                    }
                }
            }
        } else {
            let num_comps = siz.get_num_components() as Ui32;
            for c in 0..num_comps {
                let p = self.get_nlt_object(c);
                if !p.is_null() && unsafe { (*p).enabled } {
                    unsafe {
                        (*p).BDnlt = (siz.get_bit_depth(c) - 1) as Ui8;
                        (*p).BDnlt |= if siz.is_signed(c) { 0x80 } else { 0 };
                    }
                }
            }
        }

        self.trim_non_existing_components(siz.get_num_components() as Ui32);

        if self.is_any_enabled() {
            siz.set_rsiz_flag(ParamSiz::RSIZ_EXT_FLAG | ParamSiz::RSIZ_NLT_FLAG);
        }
    }

    pub fn set_nonlinear_transform(
        &mut self,
        comp_num: Ui32,
        nl_type: Ui8,
    ) -> crate::openjph::ojph_message::OjphResult<()> {
        if nl_type != Nonlinearity::OjphNltNoNlt as Ui8
            && nl_type != Nonlinearity::OjphNltBinaryComplementNlt as Ui8
        {
            return Err(crate::ojph_error!(
                0x00050171,
                "Nonliearities other than type 0 (No Nonlinearity) or type  3 (Binary Binary Complement to Sign Magnitude Conversion) are not supported yet"
            ));
        }
        let mut p = self.get_nlt_object(comp_num);
        if p.is_null() {
            p = self.add_object(comp_num);
        }
        unsafe {
            (*p).Tnlt = nl_type;
            (*p).enabled = true;
        }
        Ok(())
    }

    pub fn get_nonlinear_transform(
        &self,
        comp_num: Ui32,
        bit_depth: &mut Ui8,
        is_signed: &mut bool,
        nl_type: &mut Ui8,
    ) -> bool {
        debug_assert!(self.Cnlt == ALL_COMPS);
        let mut p = self.get_nlt_object(comp_num);
        if p.is_null() || unsafe { !(*p).enabled } {
            p = self as *const ParamNlt as *mut ParamNlt;
        }
        if unsafe { (*p).enabled } {
            *bit_depth = unsafe { ((*p).BDnlt & 0x7F) + 1 };
            *bit_depth = if *bit_depth <= 38 { *bit_depth } else { 38 };
            *is_signed = unsafe { ((*p).BDnlt & 0x80) == 0x80 };
            *nl_type = unsafe { (*p).Tnlt };
            return true;
        }
        false
    }

    pub fn write(&self, file: &mut dyn OutfileBase) -> crate::openjph::ojph_message::OjphResult<bool> {
        if !self.is_any_enabled() {
            return Ok(true);
        }

        let mut result = true;
        let mut p = self as *const ParamNlt;
        unsafe {
            while !p.is_null() {
                if (*p).enabled {
                    result &= write_u16(file, swap_byte_u16(Jp2kMarker::Nlt as Ui16))?;
                    result &= write_u16(file, swap_byte_u16((*p).Lnlt))?;
                    result &= write_u16(file, swap_byte_u16((*p).Cnlt))?;
                    result &= file.write(&[(*p).BDnlt])? == 1;
                    result &= file.write(&[(*p).Tnlt])? == 1;
                }
                p = (*p).next;
            }
        }
        Ok(result)
    }

    pub fn read(&mut self, file: &mut dyn InfileBase) -> crate::openjph::ojph_message::OjphResult<()> {
        let mut buf = [0u8; 6];
        if file.read(&mut buf) != 6 {
            return Err(crate::ojph_error!(0x00050141, "error reading NLT marker segment"));
        }

        let length = swap_byte_u16(Ui16::from_ne_bytes([buf[0], buf[1]]));
        if length != 6 || (buf[5] != 3 && buf[5] != 0) {
            return Err(crate::ojph_error!(0x00050142, "Unsupported NLT type {}", buf[5]));
        }

        let comp = swap_byte_u16(Ui16::from_ne_bytes([buf[2], buf[3]]));
        let mut p = self.get_nlt_object(comp as Ui32);
        if p.is_null() {
            p = self.add_object(comp as Ui32);
        }
        unsafe {
            (*p).enabled = true;
            (*p).Cnlt = comp;
            (*p).BDnlt = buf[4];
            (*p).Tnlt = buf[5];
        }
        Ok(())
    }

    fn get_nlt_object(&self, comp_num: Ui32) -> *mut ParamNlt {
        let mut p = self as *const ParamNlt as *mut ParamNlt;
        unsafe {
            while !p.is_null() && (*p).Cnlt != comp_num as Ui16 {
                p = (*p).next;
            }
        }
        p
    }

    fn add_object(&mut self, comp_num: Ui32) -> *mut ParamNlt {
        debug_assert!(comp_num != ALL_COMPS as Ui32);
        debug_assert!(self.Cnlt == ALL_COMPS);
        let mut p = self as *mut ParamNlt;
        unsafe {
            while !(*p).next.is_null() {
                debug_assert!((*p).Cnlt != comp_num as Ui16);
                p = (*p).next;
            }
            if !self.avail.is_null() {
                (*p).next = self.avail;
                self.avail = (*self.avail).next;
                (*(*p).next).init();
            } else {
                (*p).next = Box::into_raw(Box::new(ParamNlt::default()));
            }
            p = (*p).next;
            (*p).Cnlt = comp_num as Ui16;
            p
        }
    }

    fn is_any_enabled(&self) -> bool {
        let mut p = self as *const ParamNlt;
        unsafe {
            while !p.is_null() && !(*p).enabled {
                p = (*p).next;
            }
        }
        !p.is_null()
    }

    fn trim_non_existing_components(&mut self, num_comps: Ui32) {
        let mut p = self.next;
        unsafe {
            while !p.is_null() {
                if (*p).enabled && (*p).Cnlt >= num_comps as Ui16 {
                    (*p).enabled = false;
                    crate::ojph_info!(
                        0x00050161,
                        "The NLT marker segment for the non-existing component {} has been removed.",
                        (*p).Cnlt
                    );
                }
                p = (*p).next;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ParamSot {
    lsot: Ui16,
    isot: Ui16,
    psot: Ui32,
    tpsot: Ui8,
    tnsot: Ui8,
}

impl ParamSot {
    #[inline]
    pub const fn init(
        &mut self,
        payload_length: Ui32,
        tile_idx: Ui16,
        tile_part_index: Ui8,
        num_tile_parts: Ui8,
    ) {
        self.lsot = 10;
        self.psot = payload_length + 12;
        self.isot = tile_idx;
        self.tpsot = tile_part_index;
        self.tnsot = num_tile_parts;
    }

    pub fn write(
        &mut self,
        file: &mut dyn OutfileBase,
        payload_len: Ui32,
    ) -> crate::openjph::ojph_message::OjphResult<bool> {
        self.psot = payload_len + 14;
        self.write_marker_segment(file, self.psot, self.tpsot, self.tnsot)
    }

    pub fn write_with_tile_part(
        &mut self,
        file: &mut dyn OutfileBase,
        payload_len: Ui32,
        tpsot: Ui8,
        tnsot: Ui8,
    ) -> crate::openjph::ojph_message::OjphResult<bool> {
        self.write_marker_segment(file, payload_len + 14, tpsot, tnsot)
    }

    fn write_marker_segment(
        &self,
        file: &mut dyn OutfileBase,
        psot: Ui32,
        tpsot: Ui8,
        tnsot: Ui8,
    ) -> crate::openjph::ojph_message::OjphResult<bool> {
        let mut result = true;
        result &= write_u16(file, swap_byte_u16(Jp2kMarker::Sot as Ui16))?;
        result &= write_u16(file, swap_byte_u16(self.lsot))?;
        result &= write_u16(file, swap_byte_u16(self.isot))?;
        result &= write_u32(file, swap_byte_u32(psot))?;
        result &= file.write(&[tpsot])? == 1;
        result &= file.write(&[tnsot])? == 1;
        Ok(result)
    }

    pub fn read(
        &mut self,
        file: &mut dyn InfileBase,
        resilient: bool,
    ) -> crate::openjph::ojph_message::OjphResult<bool> {
        if resilient {
            if !read_u16(file, &mut self.lsot) {
                crate::ojph_info!(0x00050091, "error reading SOT marker");
                self.clear();
                return Ok(false);
            }
            self.lsot = swap_byte_u16(self.lsot);
            if self.lsot != 10 {
                crate::ojph_info!(0x00050092, "error in SOT length");
                self.clear();
                return Ok(false);
            }
            if !read_u16(file, &mut self.isot) {
                crate::ojph_info!(0x00050093, "error reading tile index");
                self.clear();
                return Ok(false);
            }
            self.isot = swap_byte_u16(self.isot);
            if self.isot == 0xFFFF {
                crate::ojph_info!(0x00050094, "tile index in SOT marker cannot be 0xFFFF");
                self.clear();
                return Ok(false);
            }
            if !read_u32(file, &mut self.psot) {
                crate::ojph_info!(0x00050095, "error reading SOT marker");
                self.clear();
                return Ok(false);
            }
            self.psot = swap_byte_u32(self.psot);
            if !read_u8(file, &mut self.tpsot) {
                crate::ojph_info!(0x00050096, "error reading SOT marker");
                self.clear();
                return Ok(false);
            }
            if !read_u8(file, &mut self.tnsot) {
                crate::ojph_info!(0x00050097, "error reading SOT marker");
                self.clear();
                return Ok(false);
            }
        } else {
            if !read_u16(file, &mut self.lsot) {
                return Err(crate::ojph_error!(0x00050091, "error reading SOT marker"));
            }
            self.lsot = swap_byte_u16(self.lsot);
            if self.lsot != 10 {
                return Err(crate::ojph_error!(0x00050092, "error in SOT length"));
            }
            if !read_u16(file, &mut self.isot) {
                return Err(crate::ojph_error!(0x00050093, "error reading SOT tile index"));
            }
            self.isot = swap_byte_u16(self.isot);
            if self.isot == 0xFFFF {
                return Err(crate::ojph_error!(0x00050094, "tile index in SOT marker cannot be 0xFFFF"));
            }
            if !read_u32(file, &mut self.psot) {
                return Err(crate::ojph_error!(0x00050095, "error reading SOT marker"));
            }
            self.psot = swap_byte_u32(self.psot);
            if !read_u8(file, &mut self.tpsot) {
                return Err(crate::ojph_error!(0x00050096, "error reading SOT marker"));
            }
            if !read_u8(file, &mut self.tnsot) {
                return Err(crate::ojph_error!(0x00050097, "error reading SOT marker"));
            }
        }
        Ok(true)
    }

    #[inline]
    pub const fn get_tile_index(&self) -> Ui16 {
        self.isot
    }

    /// Bytes of the tile-part after the 12-byte SOT segment (`Psot - 12`), or `None` for `Psot == 0`
    /// which T.800 defines as "this last tile-part runs to the EOC marker". A nonzero `Psot`
    /// below 12 is corrupt; it yields `Some(0)`, which the tile parser rejects because the SOD
    /// and tile-part header it has already read exceed it.
    #[inline]
    pub const fn get_payload_length(&self) -> Option<Ui32> {
        if self.psot == 0 { None } else { Some(self.psot.saturating_sub(12)) }
    }

    #[inline]
    pub const fn get_tile_part_index(&self) -> Ui8 {
        self.tpsot
    }

    #[inline]
    pub const fn get_num_tile_parts(&self) -> Ui8 {
        self.tnsot
    }

    #[inline]
    fn clear(&mut self) {
        self.lsot = 0;
        self.isot = 0;
        self.psot = 0;
        self.tpsot = 0;
        self.tnsot = 0;
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TtlmPtlmPair {
    pub ttlm: Ui16,
    pub ptlm: Ui32,
}

#[derive(Debug, Default)]
pub struct ParamTlm {
    ltlm: Ui16,
    ztlm: Ui8,
    stlm: Ui8,
    pairs: Vec<TtlmPtlmPair>,
    num_pairs: Ui32,
    next_pair_index: Ui32,
}

impl ParamTlm {
    pub fn init(
        &mut self,
        num_pairs: Ui32,
        store: Vec<TtlmPtlmPair>,
    ) -> crate::openjph::ojph_message::OjphResult<()> {
        if 4 + 6 * num_pairs > 65_535 {
            return Err(crate::ojph_error!(
                0x000500B1,
                "Trying to allocate more than 65535 bytes for a TLM marker; this can be resolved by having more than one TLM marker, but the code does not support this. In any case, this limit means that we have 10922 tileparts or more, which is a huge number."
            ));
        }
        self.num_pairs = num_pairs;
        self.pairs = store;
        if self.pairs.len() < num_pairs as usize {
            self.pairs
                .resize(num_pairs as usize, TtlmPtlmPair::default());
        }
        self.ltlm = (4 + 6 * num_pairs) as Ui16;
        self.ztlm = 0;
        self.stlm = 0x60;
        self.next_pair_index = 0;
        Ok(())
    }

    pub fn set_next_pair(&mut self, ttlm: Ui16, ptlm: Ui32) {
        debug_assert!(self.next_pair_index < self.num_pairs);
        let index = self.next_pair_index as usize;
        self.pairs[index].ttlm = ttlm;
        self.pairs[index].ptlm = ptlm + 14;
        self.next_pair_index += 1;
    }

    pub fn write(&self, file: &mut dyn OutfileBase) -> crate::openjph::ojph_message::OjphResult<bool> {
        debug_assert!(self.next_pair_index == self.num_pairs);

        let mut result = true;
        result &= write_u16(file, swap_byte_u16(Jp2kMarker::Tlm as Ui16))?;
        result &= write_u16(file, swap_byte_u16(self.ltlm))?;
        result &= file.write(&[self.ztlm])? == 1;
        result &= file.write(&[self.stlm])? == 1;
        for i in 0..self.num_pairs as usize {
            result &= write_u16(file, swap_byte_u16(self.pairs[i].ttlm))?;
            result &= write_u32(file, swap_byte_u32(self.pairs[i].ptlm))?;
        }
        Ok(result)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DfsDwtType {
    NoDwt = 0,
    BidirDwt = 1,
    HorzDwt = 2,
    VertDwt = 3,
}

#[allow(non_snake_case)]
pub struct ParamDfs {
    Ldfs: Ui16,
    Sdfs: Ui16,
    Ids: Ui8,
    Ddfs: [Ui8; 8],
    next: *mut ParamDfs,
    avail: *mut ParamDfs,
}

impl Default for ParamDfs {
    fn default() -> Self {
        let mut value = Self {
            Ldfs: 0,
            Sdfs: 0,
            Ids: 0,
            Ddfs: [0; 8],
            next: ptr::null_mut(),
            avail: ptr::null_mut(),
        };
        value.init();
        value
    }
}

impl Drop for ParamDfs {
    fn drop(&mut self) {
        unsafe {
            if !self.avail.is_null() {
                drop(Box::from_raw(self.avail));
                self.avail = ptr::null_mut();
            }
            if !self.next.is_null() {
                drop(Box::from_raw(self.next));
                self.next = ptr::null_mut();
            }
        }
    }
}

impl ParamDfs {
    #[inline]
    pub fn restart(&mut self) {
        unsafe {
            let mut p = &mut self.avail as *mut *mut ParamDfs;
            while !(*p).is_null() {
                p = &mut (**p).next as *mut *mut ParamDfs;
            }
            *p = self.next;
        }
        self.init();
    }

    #[inline]
    pub const fn exists(&self) -> bool {
        self.Ldfs != 0
    }

    pub fn get_dfs(&self, index: i32) -> Option<&ParamDfs> {
        let mut p = self as *const ParamDfs;
        unsafe {
            while !p.is_null() && (*p).Sdfs != index as Ui16 {
                p = (*p).next;
            }
            p.as_ref()
        }
    }

    pub fn get_dwt_type(&self, decomp_level: Ui32) -> DfsDwtType {
        let decomp_level = ojph_min(decomp_level, self.Ids as Ui32);
        let d = decomp_level - 1;
        let idx = d >> 2;
        let bits = d & 0x3;
        let val = (self.Ddfs[idx as usize] >> (6 - 2 * bits)) & 0x3;
        match val {
            0 => DfsDwtType::NoDwt,
            1 => DfsDwtType::BidirDwt,
            2 => DfsDwtType::HorzDwt,
            _ => DfsDwtType::VertDwt,
        }
    }

    pub fn get_subband_idx(
        &self,
        num_decompositions: Ui32,
        resolution: Ui32,
        subband: Ui32,
    ) -> Ui32 {
        debug_assert!(
            (resolution == 0 && subband == 0) || (resolution > 0 && subband > 0 && subband < 4)
        );

        let ns = [0u32, 3, 1, 1];

        let mut idx = 0;
        if resolution > 0 {
            idx = 0;
            let mut i = 1;
            while i < resolution {
                idx += ns[self.get_dwt_type(num_decompositions - i + 1) as usize];
                i += 1;
            }
            let t = self.get_dwt_type(num_decompositions - i + 1);
            idx += subband;
            if t == DfsDwtType::VertDwt && subband == 2 {
                idx -= 1;
            }
        }

        idx
    }

    pub fn get_res_downsamp(&self, mut skipped_resolutions: Ui32) -> Point {
        let mut factor = Point::new(1, 1);
        let mut decomp_level = 1;
        while skipped_resolutions > 0 {
            let ty = self.get_dwt_type(decomp_level);
            if ty == DfsDwtType::BidirDwt {
                factor.x *= 2;
                factor.y *= 2;
            } else if ty == DfsDwtType::HorzDwt {
                factor.x *= 2;
            } else if ty == DfsDwtType::VertDwt {
                factor.y *= 2;
            }

            decomp_level += 1;
            skipped_resolutions -= 1;
        }
        factor
    }

    pub fn read(&mut self, file: &mut dyn InfileBase) -> crate::openjph::ojph_message::OjphResult<()> {
        unsafe {
            if self.Ldfs != 0 {
                let mut p = self as *mut ParamDfs;
                while !(*p).next.is_null() {
                    p = (*p).next;
                }
                if !self.avail.is_null() {
                    (*p).next = self.avail;
                    self.avail = (*self.avail).next;
                    (*(*p).next).init();
                } else {
                    (*p).next = Box::into_raw(Box::new(ParamDfs::default()));
                }
                p = (*p).next;
                return (*p).read(file);
            }
        }

        if !read_u16(file, &mut self.Ldfs) {
            return Err(crate::ojph_error!(0x000500D1, "error reading DFS-Ldfs parameter"));
        }
        self.Ldfs = swap_byte_u16(self.Ldfs);
        if !read_u16(file, &mut self.Sdfs) {
            return Err(crate::ojph_error!(0x000500D2, "error reading DFS-Sdfs parameter"));
        }
        self.Sdfs = swap_byte_u16(self.Sdfs);
        if self.Sdfs > 15 {
            return Err(crate::ojph_error!(
                0x000500D3,
                "The DFS-Sdfs parameter is {}, which is larger than the permissible 15",
                self.Sdfs
            ));
        }

        let mut t = 0u8;
        let mut l_ids = 0u8;
        if !read_u8(file, &mut l_ids) {
            return Err(crate::ojph_error!(0x000500D4, "error reading DFS-Ids parameter"));
        }
        // OpenJPH v0.31.0 hardening: reject Ids == 0 (a DFS marker with zero decomposition
        // styles is meaningless and would leave Ddfs entirely unpopulated).
        if l_ids == 0 {
            return Err(crate::ojph_error!(
                0x000500D8,
                "The value of the Ids member in the DFS marker segment cannot be 0"
            ));
        }
        let max_ddfs = (self.Ddfs.len() * 4) as u8;
        if l_ids > max_ddfs {
            crate::ojph_info!(
                0x000500D5,
                "The DFS-Ids parameter is {}; while this is valid, the number is unnessarily large -- you do not need more than {}.  Please contact me regarding this issue.",
                l_ids,
                max_ddfs
            );
        }
        self.Ids = if l_ids < max_ddfs { l_ids } else { max_ddfs };
        let mut i = 0;
        while i < self.Ids {
            if file.read(&mut self.Ddfs[(i / 4) as usize..(i / 4) as usize + 1]) != 1 {
                return Err(crate::ojph_error!(0x000500D6, "error reading DFS-Ddfs parameters"));
            }
            i += 4;
        }
        let mut i = self.Ids;
        while i < l_ids {
            if !read_u8(file, &mut t) {
                return Err(crate::ojph_error!(0x000500D7, "error reading DFS-Ddfs parameters"));
            }
            i += 4;
        }
        Ok(())
    }

    fn init(&mut self) {
        self.Ldfs = 0;
        self.Sdfs = 0;
        self.Ids = 0;
        self.Ddfs = [0; 8];
        self.next = ptr::null_mut();
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct IrvData {
    pub aatk: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RevData {
    pub eatk: Ui8,
    pub batk: Si16,
    pub aatk: Si16,
}

#[allow(non_snake_case)]
#[repr(C)]
#[derive(Clone, Copy)]
pub union LiftingStep {
    pub irv: IrvData,
    pub rev: RevData,
}

impl Default for LiftingStep {
    fn default() -> Self {
        Self {
            irv: IrvData::default(),
        }
    }
}

#[allow(non_snake_case)]
pub struct ParamAtk {
    Latk: Ui16,
    Satk: Ui16,
    Katk: f32,
    Natk: Ui8,
    d: *mut LiftingStep,
    max_steps: Ui32,
    d_store: [LiftingStep; 6],
    next: *mut ParamAtk,
    top_atk: *mut ParamAtk,
    avail: *mut ParamAtk,
}

impl Default for ParamAtk {
    fn default() -> Self {
        let mut value = Self {
            Latk: 0,
            Satk: 0,
            Katk: 0.0,
            Natk: 0,
            d: ptr::null_mut(),
            max_steps: 0,
            d_store: [LiftingStep::default(); 6],
            next: ptr::null_mut(),
            top_atk: ptr::null_mut(),
            avail: ptr::null_mut(),
        };
        value.d = value.d_store.as_mut_ptr();
        value.max_steps = value.d_store.len() as Ui32;
        value.init(ptr::null_mut());
        value
    }
}

impl Drop for ParamAtk {
    fn drop(&mut self) {
        unsafe {
            if !self.avail.is_null() {
                drop(Box::from_raw(self.avail));
                self.avail = ptr::null_mut();
            }
            if !self.next.is_null() {
                drop(Box::from_raw(self.next));
                self.next = ptr::null_mut();
            }
            if !self.d.is_null() && self.max_steps > self.d_store.len() as Ui32 {
                free_lifting_steps(self.d, self.max_steps as usize);
                self.d = self.d_store.as_mut_ptr();
                self.max_steps = self.d_store.len() as Ui32;
            }
        }
    }
}

impl ParamAtk {
    #[inline]
    fn using_inline_steps(&self) -> bool {
        self.max_steps <= self.d_store.len() as Ui32
    }

    #[inline]
    fn step_ptr(&self) -> *const LiftingStep {
        if self.using_inline_steps() {
            // `d_store` lives inline inside the struct, so any move of `ParamAtk`
            // changes its address. C++ never sees this because objects are born in
            // their final storage, but Rust freely moves values after `Default`.
            // Reading through `d_store` for the small inline case preserves the
            // intended storage model without depending on a stale raw pointer.
            self.d_store.as_ptr()
        } else {
            self.d as *const LiftingStep
        }
    }

    #[inline]
    fn step_ptr_mut(&mut self) -> *mut LiftingStep {
        if self.using_inline_steps() {
            // Keep writes pinned to the inline buffer when we have not spilled to
            // heap storage. This is the actual parity fix for default ATK filters:
            // `init_irv97`/`init_rev53` may run after the struct has moved.
            self.d_store.as_mut_ptr()
        } else {
            self.d
        }
    }

    #[inline]
    pub fn restart(&mut self) {
        debug_assert!(self.top_atk.is_null());

        self.Latk = 0;
        self.Satk = 0;
        self.Katk = 0.0;
        self.Natk = 0;
        if self.d.is_null() || self.max_steps <= self.d_store.len() as Ui32 {
            // Always refresh d to current d_store address (guards against
            // struct relocation, e.g. after Box::new move).
            self.d = self.d_store.as_mut_ptr();
            self.max_steps = self.d_store.len() as Ui32;
        }
        unsafe {
            ptr::write_bytes(
                self.step_ptr_mut() as *mut u8,
                0,
                self.max_steps as usize * core::mem::size_of::<LiftingStep>(),
            );
            let mut p = &mut self.avail as *mut *mut ParamAtk;
            while !(*p).is_null() {
                p = &mut (**p).next as *mut *mut ParamAtk;
            }
            *p = self.next;
        }

        self.next = ptr::null_mut();
    }

    pub fn read(&mut self, file: &mut dyn InfileBase) -> crate::openjph::ojph_message::OjphResult<()> {
        if self.Latk != 0 {
            let p = self.add_object();
            return unsafe { (*p).read(file) };
        }

        if !read_u16(file, &mut self.Latk) {
            return Err(crate::ojph_error!(0x000500E1, "error reading ATK-Latk parameter"));
        }
        self.Latk = swap_byte_u16(self.Latk);
        let mut bytes = self.Latk as Si32 - 2;
        let mut temp_satk = 0u16;
        if !read_u16(file, &mut temp_satk) {
            return Err(crate::ojph_error!(0x000500E2, "error reading ATK-Satk parameter"));
        }
        bytes -= 2;
        temp_satk = swap_byte_u16(temp_satk);
        let tmp_idx = temp_satk & 0xFF;
        if (!self.top_atk.is_null()
            && unsafe { !(*self.top_atk).get_atk(tmp_idx as i32).is_null() })
            || tmp_idx == 0
            || tmp_idx == 1
        {
            return Err(crate::ojph_error!(
                0x000500F3,
                "ATK-Satk parameter sets ATK marker index to the illegal value of {}. ATK-Satk should be in (2-255) and, I believe, must not be repeated; otherwise, it would be unclear what marker segment must be employed when an index is repeated.",
                tmp_idx
            ));
        }
        self.Satk = temp_satk;
        if !self.is_m_init0() {
            return Err(crate::ojph_error!(
                0x000500E3,
                "ATK-Satk parameter sets m_init to 1, requiring odd-indexed subsequence in first reconstruction step, which is not supported yet."
            ));
        }
        if !self.is_whole_sample() {
            return Err(crate::ojph_error!(
                0x000500E4,
                "ATK-Satk parameter specified ARB filter, which is not supported yet."
            ));
        }
        if self.is_reversible() && self.get_coeff_type() >= 2 {
            return Err(crate::ojph_error!(
                0x000500E5,
                "ATK-Satk parameter does not make sense. It employs floats with reversible filtering."
            ));
        }
        if !self.is_using_ws_extension() {
            return Err(crate::ojph_error!(
                0x000500E6,
                "ATK-Satk parameter requires constant boundary extension, which is not supported yet."
            ));
        }
        let coeff_type = self.get_coeff_type();
        if !self.is_reversible()
            && !Self::read_coefficient_f32(coeff_type, file, &mut bytes, &mut self.Katk)
        {
            return Err(crate::ojph_error!(0x000500E7, "error reading ATK-Katk parameter"));
        }
        if !read_u8(file, &mut self.Natk) {
            return Err(crate::ojph_error!(0x000500E8, "error reading ATK-Natk parameter"));
        }
        bytes -= 1;
        if self.Natk as Ui32 > self.max_steps {
            unsafe {
                if self.max_steps > self.d_store.len() as Ui32 {
                    free_lifting_steps(self.d, self.max_steps as usize);
                }
                self.d = alloc_lifting_steps(self.Natk as usize);
            }
            self.max_steps = self.Natk as Ui32;
        }

        if self.is_reversible() {
            for s in 0..self.Natk as usize {
                let step = unsafe { self.step_ptr_mut().add(s) };
                let mut eatk = 0u8;
                if !read_u8(file, &mut eatk) {
                    return Err(crate::ojph_error!(0x000500E9, "error reading ATK-Eatk parameter"));
                }
                bytes -= 1;

                let mut batk = 0i16;
                if !read_i16_be(file, &mut batk) {
                    return Err(crate::ojph_error!(0x000500EA, "error reading ATK-Batk parameter"));
                }
                bytes -= 2;

                let mut lcatk = 0u8;
                if !read_u8(file, &mut lcatk) {
                    return Err(crate::ojph_error!(0x000500EB, "error reading ATK-LCatk parameter"));
                }
                bytes -= 1;
                if lcatk == 0 {
                    return Err(crate::ojph_error!(
                        0x000500EC,
                        "Encountered a ATK-LCatk value of zero; something is wrong."
                    ));
                }
                if lcatk > 1 {
                    return Err(crate::ojph_error!(
                        0x000500ED,
                        "ATK-LCatk value greater than 1; that is, a multitap filter is not supported"
                    ));
                }

                let mut aatk = 0i16;
                if !Self::read_coefficient_i16(coeff_type, file, &mut bytes, &mut aatk) {
                    return Err(crate::ojph_error!(0x000500EE, "Error reding ATK-Aatk parameter"));
                }

                unsafe {
                    ptr::write(
                        step,
                        LiftingStep {
                            rev: RevData { eatk, batk, aatk },
                        },
                    );
                }
            }
        } else {
            for s in 0..self.Natk as usize {
                let step = unsafe { self.step_ptr_mut().add(s) };
                let mut lcatk = 0u8;
                if !read_u8(file, &mut lcatk) {
                    return Err(crate::ojph_error!(0x000500EF, "error reading ATK-LCatk parameter"));
                }
                bytes -= 1;
                if lcatk == 0 {
                    return Err(crate::ojph_error!(
                        0x000500F0,
                        "Encountered a ATK-LCatk value of zero; something is wrong."
                    ));
                }
                if lcatk > 1 {
                    return Err(crate::ojph_error!(
                        0x000500F1,
                        "ATK-LCatk value greater than 1; that is, a multitap filter is not supported."
                    ));
                }

                let mut aatk = 0.0f32;
                if !Self::read_coefficient_f32(coeff_type, file, &mut bytes, &mut aatk) {
                    return Err(crate::ojph_error!(0x000500F2, "Error reding ATK-Aatk parameter"));
                }

                unsafe {
                    ptr::write(
                        step,
                        LiftingStep {
                            irv: IrvData { aatk },
                        },
                    );
                }
            }
        }

        if bytes != 0 {
            return Err(crate::ojph_error!(
                0x000500F3,
                "The length of an ATK marker segment (ATK-Latk) is not correct"
            ));
        }

        Ok(())
    }

    #[inline]
    pub const fn get_index(&self) -> Ui8 {
        (self.Satk & 0xFF) as Ui8
    }

    #[inline]
    pub const fn get_coeff_type(&self) -> i32 {
        ((self.Satk >> 8) & 0x7) as i32
    }

    #[inline]
    pub const fn is_whole_sample(&self) -> bool {
        (self.Satk & 0x800) != 0
    }

    #[inline]
    pub const fn is_reversible(&self) -> bool {
        (self.Satk & 0x1000) != 0
    }

    #[inline]
    pub const fn is_m_init0(&self) -> bool {
        (self.Satk & 0x2000) == 0
    }

    #[inline]
    pub const fn is_using_ws_extension(&self) -> bool {
        (self.Satk & 0x4000) != 0
    }

    pub fn get_atk(&mut self, index: i32) -> *mut ParamAtk {
        debug_assert!(self.top_atk.is_null());

        if self.Latk == 0 {
            if index == 0 {
                self.init_irv97();
                return self;
            } else if index == 1 {
                self.init_rev53();
                return self;
            }
        }

        let mut p = self as *mut ParamAtk;
        unsafe {
            while !p.is_null() && (*p).get_index() != index as Ui8 {
                p = (*p).next;
            }

            if p.is_null() && (index == 0 || index == 1) {
                p = self.add_object();
                if index == 0 {
                    (*p).init_irv97();
                } else if index == 1 {
                    (*p).init_rev53();
                }
            }
        }

        p
    }

    #[inline]
    pub fn get_step(&self, s: Ui32) -> *const LiftingStep {
        debug_assert!(s < self.Natk as Ui32);
        unsafe { self.step_ptr().add(s as usize) }
    }

    #[inline]
    pub const fn get_num_steps(&self) -> Ui32 {
        self.Natk as Ui32
    }

    #[inline]
    pub const fn get_k(&self) -> f32 {
        self.Katk
    }

    fn init(&mut self, top_atk: *mut ParamAtk) {
        self.Latk = 0;
        self.Satk = 0;
        self.Katk = 0.0;
        self.Natk = 0;
        if self.d.is_null() || self.max_steps <= self.d_store.len() as Ui32 {
            self.d = self.d_store.as_mut_ptr();
            self.max_steps = self.d_store.len() as Ui32;
        }
        unsafe {
            ptr::write_bytes(
                self.step_ptr_mut() as *mut u8,
                0,
                self.max_steps as usize * core::mem::size_of::<LiftingStep>(),
            );
        }
        self.next = ptr::null_mut();
        self.top_atk = top_atk;
        self.avail = ptr::null_mut();
    }

    fn read_coefficient_f32(
        coeff_type: i32,
        file: &mut dyn InfileBase,
        bytes: &mut Si32,
        k: &mut f32,
    ) -> bool {
        if coeff_type == 0 {
            let mut v = 0u8;
            if !read_u8(file, &mut v) {
                return false;
            }
            *bytes -= 1;
            *k = v as f32;
        } else if coeff_type == 1 {
            let mut v = 0u16;
            if !read_u16(file, &mut v) {
                return false;
            }
            *bytes -= 2;
            *k = swap_byte_u16(v) as f32;
        } else if coeff_type == 2 {
            let mut v = 0u32;
            if !read_u32(file, &mut v) {
                return false;
            }
            *bytes -= 4;
            *k = f32::from_bits(swap_byte_u32(v));
        } else if coeff_type == 3 {
            let mut v = 0u64;
            if !read_u64(file, &mut v) {
                return false;
            }
            *bytes -= 8;
            *k = f64::from_bits(swap_byte_u64(v)) as f32;
        } else if coeff_type == 4 {
            let mut v = 0u64;
            let mut v1 = 0u64;
            if !read_u64(file, &mut v) {
                return false;
            }
            *bytes -= 8;
            if !read_u64(file, &mut v1) {
                return false;
            }
            *bytes -= 8;
            let v = swap_byte_u64(v);

            let mut e = ((v >> 48) & 0x7FFF) as i32;
            e -= 16383;
            e += 127;
            e &= 0xFF;
            e <<= 23;

            let mut s = 0u32;
            s |= ((v >> 32) as u32) & 0x8000_0000;
            s |= e as u32;
            s |= ((v >> 25) as u32) & 0x007F_FFFF;
            *k = f32::from_bits(s);
        }
        true
    }

    fn read_coefficient_i16(
        coeff_type: i32,
        file: &mut dyn InfileBase,
        bytes: &mut Si32,
        k: &mut Si16,
    ) -> bool {
        if coeff_type == 0 {
            let mut v = 0u8;
            if !read_u8(file, &mut v) {
                return false;
            }
            *bytes -= 1;
            *k = v as Si8 as Si16;
        } else if coeff_type == 1 {
            let mut v = 0u16;
            if !read_u16(file, &mut v) {
                return false;
            }
            *bytes -= 2;
            *k = swap_byte_u16(v) as Si16;
        } else {
            return false;
        }
        true
    }

    fn init_irv97(&mut self) {
        self.Satk = 0x4A00;
        self.Katk = 1.230174104914001f32;
        self.Natk = 4;
        self.Latk =
            (5 + self.Natk as Ui16 + core::mem::size_of::<f32>() as Ui16 * (1 + self.Natk as Ui16))
                as Ui16;
        let d = self.step_ptr_mut();
        unsafe {
            (*d.add(0)).irv.aatk = 0.443506852043971f32;
            (*d.add(1)).irv.aatk = 0.882911075530934f32;
            (*d.add(2)).irv.aatk = -0.052980118572961f32;
            (*d.add(3)).irv.aatk = -1.586134342059924f32;
        }
    }

    fn init_rev53(&mut self) {
        self.Satk = 0x5801;
        self.Natk = 2;
        self.Latk = (5
            + 2 * self.Natk as Ui16
            + core::mem::size_of::<Ui8>() as Ui16 * (self.Natk as Ui16 + self.Natk as Ui16))
            as Ui16;
        let d = self.step_ptr_mut();
        unsafe {
            (*d.add(0)).rev.aatk = 1;
            (*d.add(0)).rev.batk = 2;
            (*d.add(0)).rev.eatk = 2;
            (*d.add(1)).rev.aatk = -1;
            (*d.add(1)).rev.batk = 1;
            (*d.add(1)).rev.eatk = 1;
        }
    }

    fn add_object(&mut self) -> *mut ParamAtk {
        // Source typo is `assert(top_atk = NULL);`.
        // `assert` is compiled out in release C++, so the exact typo cannot be
        // preserved in Rust. Keep the intended invariant check only.
        debug_assert!(self.top_atk.is_null());

        let mut p = self as *mut ParamAtk;
        unsafe {
            while !(*p).next.is_null() {
                p = (*p).next;
            }
            if !self.avail.is_null() {
                (*p).next = self.avail;
                self.avail = (*self.avail).next;
            } else {
                (*p).next = Box::into_raw(Box::new(ParamAtk::default()));
            }
            p = (*p).next;
            (*p).init(self);
        }
        p
    }
}

#[inline]
fn write_u16(
    file: &mut dyn OutfileBase,
    value: Ui16,
) -> crate::openjph::ojph_message::OjphResult<bool> {
    Ok(file.write(&value.to_ne_bytes())? == 2)
}

#[inline]
fn write_u32(
    file: &mut dyn OutfileBase,
    value: Ui32,
) -> crate::openjph::ojph_message::OjphResult<bool> {
    Ok(file.write(&value.to_ne_bytes())? == 4)
}

#[inline]
fn read_u16(file: &mut dyn InfileBase, value: &mut Ui16) -> bool {
    let mut buf = [0u8; 2];
    if file.read(&mut buf) != 2 {
        return false;
    }
    *value = Ui16::from_ne_bytes(buf);
    true
}

#[inline]
fn read_u32(file: &mut dyn InfileBase, value: &mut Ui32) -> bool {
    let mut buf = [0u8; 4];
    if file.read(&mut buf) != 4 {
        return false;
    }
    *value = Ui32::from_ne_bytes(buf);
    true
}

#[inline]
fn read_u64(file: &mut dyn InfileBase, value: &mut Ui64) -> bool {
    let mut buf = [0u8; 8];
    if file.read(&mut buf) != 8 {
        return false;
    }
    *value = Ui64::from_ne_bytes(buf);
    true
}

#[inline]
fn read_u8(file: &mut dyn InfileBase, value: &mut Ui8) -> bool {
    let mut buf = [0u8; 1];
    if file.read(&mut buf) != 1 {
        return false;
    }
    *value = buf[0];
    true
}

#[inline]
fn read_i16_be(file: &mut dyn InfileBase, value: &mut Si16) -> bool {
    let mut buf = [0u8; 2];
    if file.read(&mut buf) != 2 {
        return false;
    }
    *value = swap_byte_u16(Ui16::from_ne_bytes(buf)) as Si16;
    true
}

unsafe fn alloc_lifting_steps(num_steps: usize) -> *mut LiftingStep {
    let layout = match Layout::array::<LiftingStep>(num_steps) {
        Ok(layout) => layout,
        Err(_) => {
            debug_assert!(false, "valid lifting_step layout");
            return core::ptr::null_mut();
        }
    };
    let raw = unsafe { alloc_zeroed(layout) } as *mut LiftingStep;
    if raw.is_null() {
        handle_alloc_error(layout);
    }
    raw
}

unsafe fn free_lifting_steps(ptr: *mut LiftingStep, num_steps: usize) {
    let layout = match Layout::array::<LiftingStep>(num_steps) {
        Ok(layout) => layout,
        Err(_) => {
            debug_assert!(false, "valid lifting_step layout");
            return;
        }
    };
    unsafe {
        dealloc(ptr as *mut u8, layout);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DfsDwtType, Jp2kMarker, OJPH_PN_STRING_BROADCAST, OJPH_PO_STRING_RPCL, OjphProfileNum,
        ParamAtk, ParamCap, ParamCod, ParamDfs, ParamNlt, ParamQcd, ParamSiz, ParamSot, ParamTlm,
        ProgressionOrder, TtlmPtlmPair,
    };
    use crate::openjph::ojph_base::Point;
    use crate::openjph::ojph_file::{MemInfile, MemOutfile};
    use crate::openjph::ojph_params::Nonlinearity;

    /// `Ccap^15` is found by part number, not by position: with a Part-2 word present (`Ccap^2`
    /// first, as in 4KuF_H0L1_arri_1bpp) index 0 is the wrong word. Words are big-endian.
    #[test]
    fn cap_part_lookup_and_endianness() {
        use crate::openjph::ojph_file::MemInfile;
        // Lcap=10, Pcap = Part2 | Part15, Ccap^2 = 0x4000, Ccap^15 = 0xD823.
        let bytes = [0x00u8, 0x0A, 0x40, 0x02, 0x00, 0x00, 0x40, 0x00, 0xD8, 0x23];
        let mut inp = MemInfile::default();
        inp.open(&bytes);
        let mut cap = ParamCap::default();
        cap.read(&mut inp).expect("Part 2 + Part 15 CAP must parse");
        assert_eq!(cap.get_ccap_part(2), Some(0x4000));
        assert_eq!(cap.get_ccap_part(15), Some(0xD823));
        assert_eq!(cap.get_ccap_part(1), None);

        // Part-15-only CAP: Ccap^15 is word 0, and an unknown part is still rejected.
        let bytes = [0x00u8, 0x08, 0x00, 0x02, 0x00, 0x00, 0x18, 0x23];
        let mut inp = MemInfile::default();
        inp.open(&bytes);
        let mut cap = ParamCap::default();
        cap.read(&mut inp).expect("Part 15 CAP");
        assert_eq!(cap.get_ccap_part(15), Some(0x1823));
        let bad = [0x00u8, 0x08, 0x00, 0x02, 0x80, 0x00, 0x00, 0x00];
        let mut inp = MemInfile::default();
        inp.open(&bad);
        assert!(ParamCap::default().read(&mut inp).is_err());
    }

    /// In-tile QCC seeding (M8): the tile QCD keeps the main QCC chain, and a tile QCC for one
    /// component REPLACES that component's main entry without disturbing the others.
    #[test]
    fn tile_qcd_keeps_qcc_chain_and_override_replaces() {
        let mut main = Box::new(ParamQcd::default());
        main.Sqcd = 2;
        for (c, v) in [(1u32, 11u16), (2, 22)] {
            let n = main.add_qcc_object(c);
            unsafe { (*n).Lqcd = v };
        }
        let mut tile = Box::new(ParamQcd::default());
        tile.copy_quant_from(&main);
        assert_eq!(unsafe { (*tile.get_qcc(1)).Lqcd }, 11);
        assert_eq!(unsafe { (*tile.get_qcc(2)).Lqcd }, 22);

        let n = tile.add_qcc_object(ParamQcd::OJPH_QCD_UNKNOWN as u32);
        unsafe {
            (*n).comp_idx = 1;
            (*n).Lqcd = 99;
        }
        tile.commit_qcc(n);
        assert_eq!(unsafe { (*tile.get_qcc(1)).Lqcd }, 99, "override wins");
        assert_eq!(unsafe { (*tile.get_qcc(2)).Lqcd }, 22, "other component untouched");
        assert_eq!(unsafe { (*main.get_qcc(1)).Lqcd }, 11, "main header unchanged");
    }

    /// One place decides which code-block styles are decodable: HT takes vertical-causal and the
    /// MIXED flag; Part-1 takes the six T.800 flags; nothing else.
    #[test]
    fn block_style_rules() {
        assert!(ParamCod::block_style_ok(0x40)); // HT
        assert!(ParamCod::block_style_ok(0x48)); // HT + vertical causal
        assert!(ParamCod::block_style_ok(0xC0)); // HT MIXED
        assert!(!ParamCod::block_style_ok(0x41)); // HT + BYPASS is not a thing
        assert!(ParamCod::block_style_ok(0x3F)); // every Part-1 flag
        assert!(!ParamCod::block_style_ok(0x80)); // MIXED without HT
    }

    /// Growing then shrinking the component count used to leave `cptr` pointing into the inline
    /// 4-entry store while `old_Csiz > 4`, so the second call wrote past it. Every count must be
    /// usable repeatedly, and each resize starts from zeroed components.
    #[test]
    fn siz_set_num_components_is_repeatable() {
        let mut siz = ParamSiz::default();
        for n in [10u32, 6, 3, 40, 1] {
            siz.set_num_components(n);
            for c in 0..n {
                assert_eq!(siz.get_downsampling(c).x, 0, "fresh components are zeroed");
                siz.set_comp_info(c, Point::new(1 + c % 3, 1), 8 + c % 8, c % 2 == 0);
            }
            for c in 0..n {
                assert_eq!(siz.get_bit_depth(c), 8 + c % 8);
                assert_eq!(siz.is_signed(c), c % 2 == 0);
                assert_eq!(siz.get_downsampling(c).x, 1 + c % 3);
            }
        }
    }

    #[test]
    fn marker_values_match_openjph_reference() {
        assert_eq!(Jp2kMarker::Soc as u16, 0xFF4F);
        assert_eq!(Jp2kMarker::Cap as u16, 0xFF50);
        assert_eq!(Jp2kMarker::Siz as u16, 0xFF51);
        assert_eq!(Jp2kMarker::Cod as u16, 0xFF52);
        assert_eq!(Jp2kMarker::Coc as u16, 0xFF53);
        assert_eq!(Jp2kMarker::Qcd as u16, 0xFF5C);
        assert_eq!(Jp2kMarker::Qcc as u16, 0xFF5D);
        assert_eq!(Jp2kMarker::Sot as u16, 0xFF90);
        assert_eq!(Jp2kMarker::Sod as u16, 0xFF93);
        assert_eq!(Jp2kMarker::Eoc as u16, 0xFFD9);
    }

    #[test]
    fn marker_names_match_reference_names() {
        assert_eq!(Jp2kMarker::Soc.name(), "SOC");
        assert_eq!(Jp2kMarker::Cap.name(), "CAP");
        assert_eq!(Jp2kMarker::Qcd.name(), "QCD");
        assert_eq!(Jp2kMarker::Eoc.name(), "EOC");
    }

    #[test]
    fn set_progression_order_rejects_unknown_name() {
        let mut cod = ParamCod::default();
        assert!(cod.set_progression_order("RPCL").is_ok());
        assert!(cod.set_progression_order("NOTAREALORDER").is_err());
    }

    #[test]
    fn parse_known_markers() {
        assert_eq!(Jp2kMarker::from_be_word(0xFF4F), Some(Jp2kMarker::Soc));
        assert_eq!(Jp2kMarker::from_be_word(0xFF76), Some(Jp2kMarker::Nlt));
        assert_eq!(Jp2kMarker::from_be_word(0xFFD9), Some(Jp2kMarker::Eoc));
    }

    #[test]
    fn reject_unknown_markers() {
        assert_eq!(Jp2kMarker::from_be_word(0x0000), None);
        assert_eq!(Jp2kMarker::from_be_word(0xFF54), None);
        assert_eq!(Jp2kMarker::from_be_word(0xFFFF), None);
    }

    #[test]
    fn progression_and_profile_tables_match_reference_constants() {
        assert_eq!(ProgressionOrder::OjphPoRpcl as i32, 2);
        assert_eq!(OjphProfileNum::OjphPnBroadcast as i32, 7);
        assert_eq!(OJPH_PO_STRING_RPCL, "RPCL");
        assert_eq!(OJPH_PN_STRING_BROADCAST, "BROADCAST");
    }

    #[test]
    fn cap_write_and_read_match_reference_layout() {
        let mut cap = ParamCap::default();
        let mut out = MemOutfile::default();
        out.open(16, false).expect("open");
        assert!(cap.write(&mut out).expect("write"));
        assert_eq!(
            out.get_data(),
            &[0xFF, 0x50, 0x00, 0x08, 0x00, 0x02, 0x00, 0x00, 0x00, 0x00]
        );

        let mut inp = MemInfile::default();
        inp.open(&out.get_data()[2..]);
        let mut parsed = ParamCap::default();
        parsed.read(&mut inp).expect("read");
        assert_eq!(parsed.get_pcap(), 0x0002_0000);
        assert_eq!(parsed.get_ccap(0), 0);
    }

    /// `read_rgn` over a hand-built marker body (cursor positioned after `FF5E`, as `find_marker`
    /// leaves it). Guards the RGN byte parsing without the external conformance oracle.
    fn parse_rgn(body: &[u8], csiz: u32, resilient: bool) -> (i32, u32, u8) {
        let mut inp = MemInfile::default();
        inp.open(&body);
        let (mut comp, mut shift) = (0u32, 0u8);
        let r = super::read_rgn(&mut inp, csiz, resilient, &mut comp, &mut shift);
        (r.expect("read_rgn"), comp, shift)
    }

    #[test]
    fn rgn_read_narrow_and_wide() {
        // Narrow (csiz < 257): Lrgn=5, Crgn=1 byte. Mirrors ds0_ht_03's `FF5E 0005 00 00 07`.
        assert_eq!(parse_rgn(&[0x00, 0x05, 0x00, 0x00, 0x07], 1, false), (0, 0, 7));
        assert_eq!(parse_rgn(&[0x00, 0x05, 0x02, 0x00, 0x1F], 3, false), (0, 2, 31));
        // Wide (csiz >= 257): Lrgn=6, Crgn=2 bytes big-endian.
        assert_eq!(parse_rgn(&[0x00, 0x06, 0x01, 0x00, 0x00, 0x05], 300, false), (0, 256, 5));
    }

    #[test]
    fn rgn_read_truncated_is_resilient() {
        // Body shorter than Lrgn claims; resilient ⇒ -1 (matches skip_marker), no panic.
        assert_eq!(parse_rgn(&[0x00, 0x05, 0x00, 0x00], 1, true).0, -1);
    }

    #[test]
    #[should_panic]
    fn rgn_read_rejects_nonzero_srgn() {
        // Srgn != 0 (non-max-shift ROI) is loudly rejected.
        parse_rgn(&[0x00, 0x05, 0x00, 0x01, 0x07], 1, false);
    }

    #[test]
    #[should_panic]
    fn rgn_read_rejects_oversized_shift() {
        // SPrgn >= 32 would overflow the code-block shift; rejected loudly.
        parse_rgn(&[0x00, 0x05, 0x00, 0x00, 0x20], 1, false);
    }

    #[test]
    #[should_panic]
    fn rgn_read_rejects_out_of_range_component() {
        parse_rgn(&[0x00, 0x05, 0x05, 0x00, 0x07], 1, false);
    }

    #[test]
    fn sot_write_and_read_match_reference_lengths() {
        let mut sot = ParamSot::default();
        sot.init(100, 3, 4, 5);

        let mut out = MemOutfile::default();
        out.open(16, false).expect("open");
        assert!(sot.write(&mut out, 100).expect("write"));

        assert_eq!(
            out.get_data(),
            &[
                0xFF, 0x90, 0x00, 0x0A, 0x00, 0x03, 0x00, 0x00, 0x00, 0x72, 0x04, 0x05
            ]
        );

        let mut inp = MemInfile::default();
        inp.open(out.get_data());

        let mut marker = [0u8; 2];
        use crate::openjph::ojph_file::InfileBase;
        assert_eq!(inp.read(&mut marker), 2);
        assert_eq!(marker, [0xFF, 0x90]);

        let mut parsed = ParamSot::default();
        assert!(parsed.read(&mut inp, true).expect("read"));
        assert_eq!(parsed.get_tile_index(), 3);
        assert_eq!(parsed.get_payload_length(), Some(102));
        assert_eq!(parsed.get_tile_part_index(), 4);
        assert_eq!(parsed.get_num_tile_parts(), 5);
    }

    #[test]
    fn tlm_write_matches_reference_layout() {
        let mut tlm = ParamTlm::default();
        tlm.init(2, vec![TtlmPtlmPair::default(); 2]).expect("init");
        tlm.set_next_pair(7, 20);
        tlm.set_next_pair(8, 30);

        let mut out = MemOutfile::default();
        out.open(32, false).expect("open");
        assert!(tlm.write(&mut out).expect("write"));
        assert_eq!(
            out.get_data(),
            &[
                0xFF, 0x55, 0x00, 0x10, 0x00, 0x60, 0x00, 0x07, 0x00, 0x00, 0x00, 0x22, 0x00, 0x08,
                0x00, 0x00, 0x00, 0x2C
            ]
        );
    }

    #[test]
    fn dfs_helpers_match_reference_indexing_rules() {
        let mut dfs = ParamDfs::default();
        let bytes = [0x00, 0x06, 0x00, 0x02, 0x03, 0x60];
        let mut inp = MemInfile::default();
        inp.open(&bytes);
        assert!(dfs.read(&mut inp).is_ok());
        assert_eq!(dfs.get_dwt_type(1), DfsDwtType::BidirDwt);
        assert_eq!(dfs.get_dwt_type(2), DfsDwtType::HorzDwt);
        // The reference accumulates the previous resolution contribution first
        // and then adds the current subband id, so this layout lands on 3, not 5.
        assert_eq!(dfs.get_subband_idx(2, 2, 2), 3);
        assert_eq!(dfs.get_res_downsamp(2).x, 4);
        assert_eq!(dfs.get_res_downsamp(2).y, 2);
        assert!(dfs.get_dfs(2).is_some());
    }

    #[test]
    fn atk_default_filters_match_reference_initializers() {
        let mut atk = ParamAtk::default();
        let p0 = atk.get_atk(0);
        let p1 = atk.get_atk(1);

        assert!(!p0.is_null());
        assert!(!p1.is_null());
        assert_ne!(p0, p1);

        unsafe {
            assert_eq!((*p0).get_index(), 0);
            assert!(!(*p0).is_reversible());
            assert_eq!((*p0).get_num_steps(), 4);
            assert!(((*p0).get_k() - 1.2301741).abs() < 1e-6);
            assert!(((*(*p0).get_step(0)).irv.aatk - 0.44350684).abs() < 1e-6);
            assert!(((*(*p0).get_step(3)).irv.aatk + 1.5861343).abs() < 1e-6);

            assert_eq!((*p1).get_index(), 1);
            assert!((*p1).is_reversible());
            assert_eq!((*p1).get_num_steps(), 2);
            assert_eq!((*(*p1).get_step(0)).rev.eatk, 2);
            assert_eq!((*(*p1).get_step(0)).rev.batk, 2);
            assert_eq!((*(*p1).get_step(0)).rev.aatk, 1);
            assert_eq!((*(*p1).get_step(1)).rev.eatk, 1);
            assert_eq!((*(*p1).get_step(1)).rev.batk, 1);
            assert_eq!((*(*p1).get_step(1)).rev.aatk, -1);
        }
    }

    #[test]
    fn qcd_write_and_read_match_reference_layout() {
        let mut qcd = ParamQcd::default();
        qcd.Sqcd = 0x20;
        qcd.num_subbands = 4;
        unsafe {
            qcd.SPqcd.u8_[0] = 0x28;
            qcd.SPqcd.u8_[1] = 0x30;
            qcd.SPqcd.u8_[2] = 0x30;
            qcd.SPqcd.u8_[3] = 0x38;
        }

        let mut out = MemOutfile::default();
        out.open(16, false).expect("open");
        assert!(qcd.write(&mut out).expect("write"));
        assert_eq!(
            out.get_data(),
            &[0xFF, 0x5C, 0x00, 0x07, 0x20, 0x28, 0x30, 0x30, 0x38]
        );

        let mut inp = MemInfile::default();
        inp.open(&out.get_data()[2..]);
        let mut parsed = ParamQcd::default();
        parsed.read(&mut inp).expect("read");
        assert_eq!(parsed.Lqcd, 7);
        assert_eq!(parsed.Sqcd, 0x20);
        assert_eq!(parsed.num_subbands, 4);
        unsafe {
            assert_eq!(parsed.SPqcd.u8_[0], 0x28);
            assert_eq!(parsed.SPqcd.u8_[3], 0x38);
        }
    }

    #[test]
    fn nlt_write_and_read_match_reference_layout() {
        let mut nlt = ParamNlt::default();
        nlt.set_nonlinear_transform(1, Nonlinearity::OjphNltBinaryComplementNlt as u8).expect("set_nonlinear_transform");
        let p = nlt.get_nlt_object(1);
        assert!(!p.is_null());
        unsafe {
            (*p).BDnlt = 0x87;
        }

        let mut out = MemOutfile::default();
        out.open(16, false).expect("open");
        assert!(nlt.write(&mut out).expect("write"));
        assert_eq!(
            out.get_data(),
            &[0xFF, 0x76, 0x00, 0x06, 0x00, 0x01, 0x87, 0x03]
        );

        let mut inp = MemInfile::default();
        inp.open(&out.get_data()[2..]);
        let mut parsed = ParamNlt::default();
        parsed.read(&mut inp).expect("read");
        let p = parsed.get_nlt_object(1);
        assert!(!p.is_null());
        unsafe {
            assert!((*p).enabled);
            assert_eq!((*p).Cnlt, 1);
            assert_eq!((*p).BDnlt, 0x87);
            assert_eq!((*p).Tnlt, 3);
        }
    }
}
