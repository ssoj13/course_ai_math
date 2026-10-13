//! Port target:
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/coding/ojph_block_encoder.h`
//! - `https://github.com/aous72/OpenJPH/blob/v0.26.3/src/core/coding/ojph_block_encoder.cpp`

use std::sync::LazyLock;

use crate::openjph::ojph_arch::{
    count_leading_zeros_u32, count_leading_zeros_u64, population_count,
};
use crate::openjph::ojph_defs::{ojph_max, ojph_min, Ui16, Ui32, Ui64, Ui8};
use crate::openjph::ojph_mem::{mem_elastic_allocator, CodedLists};

use super::ojph_block_common::VlcSrcTable;
use super::{table0::TABLE0, table1::TABLE1};

pub const NUM_UVLC_ENTRIES: usize = 75;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct UvlcTblStruct {
    pub pre: Ui8,
    pub pre_len: Ui8,
    pub suf: Ui8,
    pub suf_len: Ui8,
    pub(crate) ext: Ui8,
    pub(crate) ext_len: Ui8,
}

fn build_encoder_vlc_table(src_tbl: &[VlcSrcTable]) -> [Ui16; 2048] {
    let mut tgt_tbl = [0u16; 2048];
    let mut pattern_popcnt = [0i32; 16];
    for i in 0..16 {
        pattern_popcnt[i] = population_count(i as Ui32) as i32;
    }

    for (i, slot) in tgt_tbl.iter_mut().enumerate() {
        let c_q = (i >> 8) as i32;
        let rho = ((i >> 4) & 0xF) as i32;
        let emb = (i & 0xF) as i32;
        if ((emb & rho) != emb) || (rho == 0 && c_q == 0) {
            *slot = 0;
        } else {
            let mut best_entry: Option<&VlcSrcTable> = None;
            if emb != 0 {
                let mut best_e_k = -1;
                for entry in src_tbl {
                    if entry.c_q == c_q
                        && entry.rho == rho
                        && entry.u_off == 1
                        && (emb & entry.e_k) == entry.e_1
                    {
                        let ones_count = pattern_popcnt[entry.e_k as usize];
                        if ones_count >= best_e_k {
                            best_entry = Some(entry);
                            best_e_k = ones_count;
                        }
                    }
                }
            } else {
                for entry in src_tbl {
                    if entry.c_q == c_q && entry.rho == rho && entry.u_off == 0 {
                        best_entry = Some(entry);
                        break;
                    }
                }
            }

            if let Some(best_entry) = best_entry {
                *slot =
                    ((best_entry.cwd << 8) + (best_entry.cwd_len << 4) + best_entry.e_k) as Ui16;
            } else {
                debug_assert!(false, "OpenJPH encoder VLC table entry missing");
            }
        }
    }

    tgt_tbl
}

fn build_uvlc_table() -> [UvlcTblStruct; NUM_UVLC_ENTRIES] {
    let mut tbl = [UvlcTblStruct::default(); NUM_UVLC_ENTRIES];

    tbl[0] = UvlcTblStruct {
        pre: 0,
        pre_len: 0,
        suf: 0,
        suf_len: 0,
        ext: 0,
        ext_len: 0,
    };
    tbl[1] = UvlcTblStruct {
        pre: 1,
        pre_len: 1,
        suf: 0,
        suf_len: 0,
        ext: 0,
        ext_len: 0,
    };
    tbl[2] = UvlcTblStruct {
        pre: 2,
        pre_len: 2,
        suf: 0,
        suf_len: 0,
        ext: 0,
        ext_len: 0,
    };
    tbl[3] = UvlcTblStruct {
        pre: 4,
        pre_len: 3,
        suf: 0,
        suf_len: 1,
        ext: 0,
        ext_len: 0,
    };
    tbl[4] = UvlcTblStruct {
        pre: 4,
        pre_len: 3,
        suf: 1,
        suf_len: 1,
        ext: 0,
        ext_len: 0,
    };

    for (i, entry) in tbl.iter_mut().enumerate().take(33).skip(5) {
        *entry = UvlcTblStruct {
            pre: 0,
            pre_len: 3,
            suf: (i - 5) as Ui8,
            suf_len: 5,
            ext: 0,
            ext_len: 0,
        };
    }

    for (i, entry) in tbl.iter_mut().enumerate().skip(33) {
        *entry = UvlcTblStruct {
            pre: 0,
            pre_len: 3,
            suf: (28 + (i - 33) % 4) as Ui8,
            suf_len: 5,
            ext: ((i - 33) / 4) as Ui8,
            ext_len: 4,
        };
    }

    tbl
}

#[allow(non_upper_case_globals)]
pub static vlc_tbl0: LazyLock<[Ui16; 2048]> = LazyLock::new(|| build_encoder_vlc_table(&TABLE0));

#[allow(non_upper_case_globals)]
pub static vlc_tbl1: LazyLock<[Ui16; 2048]> = LazyLock::new(|| build_encoder_vlc_table(&TABLE1));

#[allow(non_upper_case_globals)]
pub static uvlc_tbl: LazyLock<[UvlcTblStruct; NUM_UVLC_ENTRIES]> = LazyLock::new(build_uvlc_table);

pub(crate) fn initialize_block_encoder_tables() -> bool {
    let _ = &*vlc_tbl0;
    let _ = &*vlc_tbl1;
    let _ = &*uvlc_tbl;
    true
}

pub(crate) fn initialize_block_encoder_tables_avx2() -> bool {
    initialize_block_encoder_tables()
}

pub(crate) fn initialize_block_encoder_tables_avx512() -> bool {
    initialize_block_encoder_tables()
}

#[allow(non_camel_case_types)]
pub(crate) struct mel_struct {
    pub(crate) buf: *mut Ui8,
    pub(crate) pos: Ui32,
    pub(crate) buf_size: Ui32,
    pub(crate) remaining_bits: i32,
    pub(crate) tmp: i32,
    pub(crate) run: i32,
    pub(crate) k: i32,
    pub(crate) threshold: i32,
}

impl Default for mel_struct {
    fn default() -> Self {
        Self {
            buf: core::ptr::null_mut(),
            pos: 0,
            buf_size: 0,
            remaining_bits: 0,
            tmp: 0,
            run: 0,
            k: 0,
            threshold: 0,
        }
    }
}

#[inline]
pub(crate) unsafe fn mel_init(melp: &mut mel_struct, buffer_size: Ui32, data: *mut Ui8) {
    melp.buf = data;
    melp.pos = 0;
    melp.buf_size = buffer_size;
    melp.remaining_bits = 8;
    melp.tmp = 0;
    melp.run = 0;
    melp.k = 0;
    melp.threshold = 1;
}

#[inline]
pub(crate) unsafe fn mel_emit_bit(
    melp: &mut mel_struct,
    v: i32,
) -> crate::openjph::ojph_message::OjphResult<()> {
    debug_assert!(v == 0 || v == 1);
    melp.tmp = (melp.tmp << 1) + v;
    melp.remaining_bits -= 1;
    if melp.remaining_bits == 0 {
        if melp.pos >= melp.buf_size {
            return Err(crate::ojph_error!(
                0x00020001,
                "mel encoder's buffer is full"
            ));
        }
        *melp.buf.add(melp.pos as usize) = melp.tmp as Ui8;
        melp.pos += 1;
        melp.remaining_bits = if melp.tmp == 0xFF { 7 } else { 8 };
        melp.tmp = 0;
    }
    Ok(())
}

#[inline]
pub(crate) unsafe fn mel_encode(
    melp: &mut mel_struct,
    bit: bool,
) -> crate::openjph::ojph_message::OjphResult<()> {
    static MEL_EXP: [i32; 13] = [0, 0, 0, 1, 1, 1, 2, 2, 2, 3, 3, 4, 5];

    if !bit {
        melp.run += 1;
        if melp.run >= melp.threshold {
            mel_emit_bit(melp, 1)?;
            melp.run = 0;
            melp.k = ojph_min(12, melp.k + 1);
            melp.threshold = 1 << MEL_EXP[melp.k as usize];
        }
    } else {
        mel_emit_bit(melp, 0)?;
        let mut t = MEL_EXP[melp.k as usize];
        while t > 0 {
            t -= 1;
            mel_emit_bit(melp, (melp.run >> t) & 1)?;
        }
        melp.run = 0;
        melp.k = ojph_max(0, melp.k - 1);
        melp.threshold = 1 << MEL_EXP[melp.k as usize];
    }
    Ok(())
}

#[allow(non_camel_case_types)]
pub(crate) struct vlc_struct {
    pub(crate) buf: *mut Ui8,
    pub(crate) pos: Ui32,
    pub(crate) buf_size: Ui32,
    pub(crate) used_bits: i32,
    pub(crate) tmp: i32,
    pub(crate) last_greater_than_8f: bool,
}

impl Default for vlc_struct {
    fn default() -> Self {
        Self {
            buf: core::ptr::null_mut(),
            pos: 0,
            buf_size: 0,
            used_bits: 0,
            tmp: 0,
            last_greater_than_8f: false,
        }
    }
}

#[inline]
pub(crate) unsafe fn vlc_init(vlcp: &mut vlc_struct, buffer_size: Ui32, data: *mut Ui8) {
    vlcp.buf = data.add(buffer_size as usize - 1);
    vlcp.pos = 1;
    vlcp.buf_size = buffer_size;
    *vlcp.buf = 0xFF;
    vlcp.used_bits = 4;
    vlcp.tmp = 0xF;
    vlcp.last_greater_than_8f = true;
}

#[inline]
pub(crate) unsafe fn vlc_encode(
    vlcp: &mut vlc_struct,
    mut cwd: i32,
    mut cwd_len: i32,
) -> crate::openjph::ojph_message::OjphResult<()> {
    while cwd_len > 0 {
        if vlcp.pos >= vlcp.buf_size {
            return Err(crate::ojph_error!(
                0x00020002,
                "vlc encoder's buffer is full"
            ));
        }

        let mut avail_bits = 8 - if vlcp.last_greater_than_8f { 1 } else { 0 } - vlcp.used_bits;
        let t = ojph_min(avail_bits, cwd_len);
        vlcp.tmp |= (cwd & ((1 << t) - 1)) << vlcp.used_bits;
        vlcp.used_bits += t;
        avail_bits -= t;
        cwd_len -= t;
        cwd >>= t;
        if avail_bits == 0 {
            if vlcp.last_greater_than_8f && vlcp.tmp != 0x7F {
                vlcp.last_greater_than_8f = false;
                continue;
            }
            *vlcp.buf.sub(vlcp.pos as usize) = vlcp.tmp as Ui8;
            vlcp.pos += 1;
            vlcp.last_greater_than_8f = vlcp.tmp > 0x8F;
            vlcp.tmp = 0;
            vlcp.used_bits = 0;
        }
    }
    Ok(())
}

#[inline]
pub(crate) unsafe fn terminate_mel_vlc(
    melp: &mut mel_struct,
    vlcp: &mut vlc_struct,
) -> crate::openjph::ojph_message::OjphResult<()> {
    if melp.run > 0 {
        mel_emit_bit(melp, 1)?;
    }

    melp.tmp <<= melp.remaining_bits;
    let mel_mask = ((0xFF << melp.remaining_bits) & 0xFF) as i32;
    let vlc_mask = (0xFF >> (8 - vlcp.used_bits)) as i32;
    if (mel_mask | vlc_mask) == 0 {
        return Ok(());
    }
    if melp.pos >= melp.buf_size {
        return Err(crate::ojph_error!(
            0x00020003,
            "mel encoder's buffer is full"
        ));
    }
    let fuse = melp.tmp | vlcp.tmp;
    if ((((fuse ^ melp.tmp) & mel_mask) | ((fuse ^ vlcp.tmp) & vlc_mask)) == 0)
        && fuse != 0xFF
        && vlcp.pos > 1
    {
        *melp.buf.add(melp.pos as usize) = fuse as Ui8;
        melp.pos += 1;
    } else {
        if vlcp.pos >= vlcp.buf_size {
            return Err(crate::ojph_error!(
                0x00020004,
                "vlc encoder's buffer is full"
            ));
        }
        *melp.buf.add(melp.pos as usize) = melp.tmp as Ui8;
        melp.pos += 1;
        *vlcp.buf.sub(vlcp.pos as usize) = vlcp.tmp as Ui8;
        vlcp.pos += 1;
    }
    Ok(())
}

#[allow(non_camel_case_types)]
pub(crate) struct ms_struct {
    pub(crate) buf: *mut Ui8,
    pub(crate) pos: Ui32,
    pub(crate) buf_size: Ui32,
    pub(crate) max_bits: i32,
    pub(crate) used_bits: i32,
    pub(crate) tmp: Ui32,
}

impl Default for ms_struct {
    fn default() -> Self {
        Self {
            buf: core::ptr::null_mut(),
            pos: 0,
            buf_size: 0,
            max_bits: 0,
            used_bits: 0,
            tmp: 0,
        }
    }
}

#[inline]
pub(crate) unsafe fn ms_init(msp: &mut ms_struct, buffer_size: Ui32, data: *mut Ui8) {
    msp.buf = data;
    msp.pos = 0;
    msp.buf_size = buffer_size;
    msp.max_bits = 8;
    msp.used_bits = 0;
    msp.tmp = 0;
}

#[inline]
pub(crate) unsafe fn ms_encode(
    msp: &mut ms_struct,
    mut cwd: Ui32,
    mut cwd_len: i32,
) -> crate::openjph::ojph_message::OjphResult<()> {
    while cwd_len > 0 {
        if msp.pos >= msp.buf_size {
            return Err(crate::ojph_error!(
                0x00020005,
                "magnitude sign encoder's buffer is full"
            ));
        }
        let t = ojph_min(msp.max_bits - msp.used_bits, cwd_len);
        msp.tmp |= (cwd & ((1u32 << t) - 1)) << msp.used_bits;
        msp.used_bits += t;
        cwd >>= t;
        cwd_len -= t;
        if msp.used_bits >= msp.max_bits {
            *msp.buf.add(msp.pos as usize) = msp.tmp as Ui8;
            msp.pos += 1;
            msp.max_bits = if msp.tmp == 0xFF { 7 } else { 8 };
            msp.tmp = 0;
            msp.used_bits = 0;
        }
    }
    Ok(())
}

#[inline]
pub(crate) unsafe fn ms_encode64(
    msp: &mut ms_struct,
    mut cwd: Ui64,
    mut cwd_len: i32,
) -> crate::openjph::ojph_message::OjphResult<()> {
    while cwd_len > 0 {
        if msp.pos >= msp.buf_size {
            return Err(crate::ojph_error!(
                0x00020005,
                "magnitude sign encoder's buffer is full"
            ));
        }
        let t = ojph_min(msp.max_bits - msp.used_bits, cwd_len);
        msp.tmp |= ((cwd & ((1u64 << t) - 1)) << msp.used_bits) as Ui32;
        msp.used_bits += t;
        cwd >>= t;
        cwd_len -= t;
        if msp.used_bits >= msp.max_bits {
            *msp.buf.add(msp.pos as usize) = msp.tmp as Ui8;
            msp.pos += 1;
            msp.max_bits = if msp.tmp == 0xFF { 7 } else { 8 };
            msp.tmp = 0;
            msp.used_bits = 0;
        }
    }
    Ok(())
}

#[inline]
pub(crate) unsafe fn ms_terminate(
    msp: &mut ms_struct,
) -> crate::openjph::ojph_message::OjphResult<()> {
    if msp.used_bits != 0 {
        let t = msp.max_bits - msp.used_bits;
        msp.tmp |= (0xFF & ((1u32 << t) - 1)) << msp.used_bits;
        msp.used_bits += t;
        if msp.tmp != 0xFF {
            if msp.pos >= msp.buf_size {
                return Err(crate::ojph_error!(
                    0x00020006,
                    "magnitude sign encoder's buffer is full"
                ));
            }
            *msp.buf.add(msp.pos as usize) = msp.tmp as Ui8;
            msp.pos += 1;
        }
    } else if msp.max_bits == 7 {
        msp.pos -= 1;
    }
    Ok(())
}

#[inline]
pub(crate) unsafe fn store_encoded_block(
    mel: &mel_struct,
    vlc: &vlc_struct,
    ms: &ms_struct,
    lengths: &mut [Ui32],
    elastic: *mut mem_elastic_allocator,
    coded: &mut *mut CodedLists,
) {
    (*elastic).get_buffer(mel.pos + vlc.pos + ms.pos, coded);
    core::ptr::copy_nonoverlapping(ms.buf, (**coded).buf, ms.pos as usize);
    core::ptr::copy_nonoverlapping(
        mel.buf,
        (**coded).buf.add(ms.pos as usize),
        mel.pos as usize,
    );
    core::ptr::copy_nonoverlapping(
        vlc.buf.sub(vlc.pos as usize - 1),
        (**coded).buf.add((ms.pos + mel.pos) as usize),
        vlc.pos as usize,
    );

    lengths[0] = ms.pos + mel.pos + vlc.pos;
    let num_bytes = mel.pos + vlc.pos;
    *((**coded).buf.add(lengths[0] as usize - 1)) = (num_bytes >> 4) as Ui8;
    let last2 = (**coded).buf.add(lengths[0] as usize - 2);
    *last2 &= 0xF0;
    *last2 |= (num_bytes & 0xF) as Ui8;
    (**coded).avail_size -= lengths[0];
}

pub(crate) unsafe fn ojph_encode_codeblock32(
    buf: *mut Ui32,
    missing_msbs: Ui32,
    num_passes: Ui32,
    width: Ui32,
    height: Ui32,
    stride: Ui32,
    lengths: &mut [Ui32; 2],
    elastic: *mut mem_elastic_allocator,
    coded: &mut *mut CodedLists,
) -> crate::openjph::ojph_message::OjphResult<()> {
    debug_assert!(num_passes == 1);
    let ms_size = (16384 * 16 + 14) / 15;
    let mut ms_buf = [0u8; (16384 * 16 + 14) / 15];
    let mel_vlc_size = 3072usize;
    let mut mel_vlc_buf = [0u8; 3072];
    let mel_size = 192usize;
    let mel_buf = mel_vlc_buf.as_mut_ptr();
    let vlc_size = mel_vlc_size - mel_size;
    let vlc_buf = mel_buf.add(mel_size);

    let mut mel = mel_struct::default();
    mel_init(&mut mel, mel_size as Ui32, mel_buf);
    let mut vlc = vlc_struct::default();
    vlc_init(&mut vlc, vlc_size as Ui32, vlc_buf);
    let mut ms = ms_struct::default();
    ms_init(&mut ms, ms_size as Ui32, ms_buf.as_mut_ptr());

    let p = 30 - missing_msbs;

    let mut e_val = [0u8; 513];
    let mut cx_val = [0u8; 513];
    let mut lep = 0usize;
    let mut lcxp = 0usize;
    e_val[lep] = 0;
    cx_val[lcxp] = 0;

    let mut e_qmax = [0i32, 0];
    let mut e_q = [0i32; 8];
    let mut rho = [0i32, 0];
    let mut c_q0 = 0i32;
    let mut s = [0u32; 8];
    let mut sp = buf;
    let mut x = 0u32;
    while x < width {
        let mut t = *sp;
        let mut val = t.wrapping_add(t);
        val >>= p;
        val &= !1u32;
        if val != 0 {
            rho[0] = 1;
            val = val.wrapping_sub(1);
            e_q[0] = 32 - count_leading_zeros_u32(val) as i32;
            e_qmax[0] = e_q[0];
            s[0] = val.wrapping_sub(1).wrapping_add(t >> 31);
        }

        t = if height > 1 {
            *sp.add(stride as usize)
        } else {
            0
        };
        sp = sp.add(1);
        val = t.wrapping_add(t);
        val >>= p;
        val &= !1u32;
        if val != 0 {
            rho[0] += 2;
            val = val.wrapping_sub(1);
            e_q[1] = 32 - count_leading_zeros_u32(val) as i32;
            e_qmax[0] = ojph_max(e_qmax[0], e_q[1]);
            s[1] = val.wrapping_sub(1).wrapping_add(t >> 31);
        }

        if x + 1 < width {
            t = *sp;
            val = t.wrapping_add(t);
            val >>= p;
            val &= !1u32;
            if val != 0 {
                rho[0] += 4;
                val = val.wrapping_sub(1);
                e_q[2] = 32 - count_leading_zeros_u32(val) as i32;
                e_qmax[0] = ojph_max(e_qmax[0], e_q[2]);
                s[2] = val.wrapping_sub(1).wrapping_add(t >> 31);
            }

            t = if height > 1 {
                *sp.add(stride as usize)
            } else {
                0
            };
            sp = sp.add(1);
            val = t.wrapping_add(t);
            val >>= p;
            val &= !1u32;
            if val != 0 {
                rho[0] += 8;
                val = val.wrapping_sub(1);
                e_q[3] = 32 - count_leading_zeros_u32(val) as i32;
                e_qmax[0] = ojph_max(e_qmax[0], e_q[3]);
                s[3] = val.wrapping_sub(1).wrapping_add(t >> 31);
            }
        }

        let uq0 = ojph_max(e_qmax[0], 1);
        let u_q0 = uq0 - 1;
        let mut u_q1 = 0;

        let mut eps0 = 0i32;
        if u_q0 > 0 {
            eps0 |= (e_q[0] == e_qmax[0]) as i32;
            eps0 |= ((e_q[1] == e_qmax[0]) as i32) << 1;
            eps0 |= ((e_q[2] == e_qmax[0]) as i32) << 2;
            eps0 |= ((e_q[3] == e_qmax[0]) as i32) << 3;
        }
        e_val[lep] = ojph_max(e_val[lep], e_q[1] as Ui8);
        lep += 1;
        e_val[lep] = e_q[3] as Ui8;
        cx_val[lcxp] |= ((rho[0] & 2) >> 1) as Ui8;
        lcxp += 1;
        cx_val[lcxp] = ((rho[0] & 8) >> 3) as Ui8;

        let tuple0 = vlc_tbl0[((c_q0 << 8) + (rho[0] << 4) + eps0) as usize];
        vlc_encode(&mut vlc, (tuple0 >> 8) as i32, ((tuple0 >> 4) & 7) as i32)?;
        if c_q0 == 0 {
            mel_encode(&mut mel, rho[0] != 0)?;
        }

        let mut m = if (rho[0] & 1) != 0 {
            uq0 - (tuple0 & 1) as i32
        } else {
            0
        };
        ms_encode(&mut ms, s[0] & ((1u32 << m) - 1), m)?;
        m = if (rho[0] & 2) != 0 {
            uq0 - ((tuple0 & 2) >> 1) as i32
        } else {
            0
        };
        ms_encode(&mut ms, s[1] & ((1u32 << m) - 1), m)?;
        m = if (rho[0] & 4) != 0 {
            uq0 - ((tuple0 & 4) >> 2) as i32
        } else {
            0
        };
        ms_encode(&mut ms, s[2] & ((1u32 << m) - 1), m)?;
        m = if (rho[0] & 8) != 0 {
            uq0 - ((tuple0 & 8) >> 3) as i32
        } else {
            0
        };
        ms_encode(&mut ms, s[3] & ((1u32 << m) - 1), m)?;
        if x + 2 < width {
            t = *sp;
            val = t.wrapping_add(t);
            val >>= p;
            val &= !1u32;
            if val != 0 {
                rho[1] = 1;
                val = val.wrapping_sub(1);
                e_q[4] = 32 - count_leading_zeros_u32(val) as i32;
                e_qmax[1] = e_q[4];
                s[4] = val.wrapping_sub(1).wrapping_add(t >> 31);
            }

            t = if height > 1 {
                *sp.add(stride as usize)
            } else {
                0
            };
            sp = sp.add(1);
            val = t.wrapping_add(t);
            val >>= p;
            val &= !1u32;
            if val != 0 {
                rho[1] += 2;
                val = val.wrapping_sub(1);
                e_q[5] = 32 - count_leading_zeros_u32(val) as i32;
                e_qmax[1] = ojph_max(e_qmax[1], e_q[5]);
                s[5] = val.wrapping_sub(1).wrapping_add(t >> 31);
            }

            if x + 3 < width {
                t = *sp;
                val = t.wrapping_add(t);
                val >>= p;
                val &= !1u32;
                if val != 0 {
                    rho[1] += 4;
                    val = val.wrapping_sub(1);
                    e_q[6] = 32 - count_leading_zeros_u32(val) as i32;
                    e_qmax[1] = ojph_max(e_qmax[1], e_q[6]);
                    s[6] = val.wrapping_sub(1).wrapping_add(t >> 31);
                }

                t = if height > 1 {
                    *sp.add(stride as usize)
                } else {
                    0
                };
                sp = sp.add(1);
                val = t.wrapping_add(t);
                val >>= p;
                val &= !1u32;
                if val != 0 {
                    rho[1] += 8;
                    val = val.wrapping_sub(1);
                    e_q[7] = 32 - count_leading_zeros_u32(val) as i32;
                    e_qmax[1] = ojph_max(e_qmax[1], e_q[7]);
                    s[7] = val.wrapping_sub(1).wrapping_add(t >> 31);
                }
            }

            let c_q1 = (rho[0] >> 1) | (rho[0] & 1);
            let uq1 = ojph_max(e_qmax[1], 1);
            u_q1 = uq1 - 1;

            let mut eps1 = 0i32;
            if u_q1 > 0 {
                eps1 |= (e_q[4] == e_qmax[1]) as i32;
                eps1 |= ((e_q[5] == e_qmax[1]) as i32) << 1;
                eps1 |= ((e_q[6] == e_qmax[1]) as i32) << 2;
                eps1 |= ((e_q[7] == e_qmax[1]) as i32) << 3;
            }
            e_val[lep] = ojph_max(e_val[lep], e_q[5] as Ui8);
            lep += 1;
            e_val[lep] = e_q[7] as Ui8;
            cx_val[lcxp] |= ((rho[1] & 2) >> 1) as Ui8;
            lcxp += 1;
            cx_val[lcxp] = ((rho[1] & 8) >> 3) as Ui8;
            let tuple1 = vlc_tbl0[((c_q1 << 8) + (rho[1] << 4) + eps1) as usize];
            vlc_encode(&mut vlc, (tuple1 >> 8) as i32, ((tuple1 >> 4) & 7) as i32)?;
            if c_q1 == 0 {
                mel_encode(&mut mel, rho[1] != 0)?;
            }

            m = if (rho[1] & 1) != 0 {
                uq1 - (tuple1 & 1) as i32
            } else {
                0
            };
            ms_encode(&mut ms, s[4] & ((1u32 << m) - 1), m)?;
            m = if (rho[1] & 2) != 0 {
                uq1 - ((tuple1 & 2) >> 1) as i32
            } else {
                0
            };
            ms_encode(&mut ms, s[5] & ((1u32 << m) - 1), m)?;
            m = if (rho[1] & 4) != 0 {
                uq1 - ((tuple1 & 4) >> 2) as i32
            } else {
                0
            };
            ms_encode(&mut ms, s[6] & ((1u32 << m) - 1), m)?;
            m = if (rho[1] & 8) != 0 {
                uq1 - ((tuple1 & 8) >> 3) as i32
            } else {
                0
            };
            ms_encode(&mut ms, s[7] & ((1u32 << m) - 1), m)?;
        }

        if u_q0 > 0 && u_q1 > 0 {
            mel_encode(&mut mel, ojph_min(u_q0, u_q1) > 2)?;
        }

        if u_q0 > 2 && u_q1 > 2 {
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q0 as usize - 2].pre as i32,
                uvlc_tbl[u_q0 as usize - 2].pre_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q1 as usize - 2].pre as i32,
                uvlc_tbl[u_q1 as usize - 2].pre_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q0 as usize - 2].suf as i32,
                uvlc_tbl[u_q0 as usize - 2].suf_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q1 as usize - 2].suf as i32,
                uvlc_tbl[u_q1 as usize - 2].suf_len as i32,
            )?;
        } else if u_q0 > 2 && u_q1 > 0 {
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q0 as usize].pre as i32,
                uvlc_tbl[u_q0 as usize].pre_len as i32,
            )?;
            vlc_encode(&mut vlc, u_q1 - 1, 1)?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q0 as usize].suf as i32,
                uvlc_tbl[u_q0 as usize].suf_len as i32,
            )?;
        } else {
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q0 as usize].pre as i32,
                uvlc_tbl[u_q0 as usize].pre_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q1 as usize].pre as i32,
                uvlc_tbl[u_q1 as usize].pre_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q0 as usize].suf as i32,
                uvlc_tbl[u_q0 as usize].suf_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q1 as usize].suf as i32,
                uvlc_tbl[u_q1 as usize].suf_len as i32,
            )?;
        }

        c_q0 = (rho[1] >> 1) | (rho[1] & 1);
        s = [0; 8];
        e_q = [0; 8];
        rho = [0, 0];
        e_qmax = [0, 0];
        x += 4;
    }

    e_val[lep + 1] = 0;

    let mut y = 2u32; // the first row pair was coded above; then two rows per step
    while y < height {
        lep = 0;
        let mut max_e = ojph_max(e_val[lep], e_val[lep + 1]) as i32 - 1;
        e_val[lep] = 0;
        lcxp = 0;
        c_q0 = cx_val[lcxp] as i32 + ((cx_val[lcxp + 1] as i32) << 2);
        cx_val[lcxp] = 0;

        sp = buf.add((y * stride) as usize);
        x = 0;
        while x < width {
            let mut t = *sp;
            let mut val = t.wrapping_add(t);
            val >>= p;
            val &= !1u32;
            if val != 0 {
                rho[0] = 1;
                val = val.wrapping_sub(1);
                e_q[0] = 32 - count_leading_zeros_u32(val) as i32;
                e_qmax[0] = e_q[0];
                s[0] = val.wrapping_sub(1).wrapping_add(t >> 31);
            }

            t = if y + 1 < height {
                *sp.add(stride as usize)
            } else {
                0
            };
            sp = sp.add(1);
            val = t.wrapping_add(t);
            val >>= p;
            val &= !1u32;
            if val != 0 {
                rho[0] += 2;
                val = val.wrapping_sub(1);
                e_q[1] = 32 - count_leading_zeros_u32(val) as i32;
                e_qmax[0] = ojph_max(e_qmax[0], e_q[1]);
                s[1] = val.wrapping_sub(1).wrapping_add(t >> 31);
            }

            if x + 1 < width {
                t = *sp;
                val = t.wrapping_add(t);
                val >>= p;
                val &= !1u32;
                if val != 0 {
                    rho[0] += 4;
                    val = val.wrapping_sub(1);
                    e_q[2] = 32 - count_leading_zeros_u32(val) as i32;
                    e_qmax[0] = ojph_max(e_qmax[0], e_q[2]);
                    s[2] = val.wrapping_sub(1).wrapping_add(t >> 31);
                }

                t = if y + 1 < height {
                    *sp.add(stride as usize)
                } else {
                    0
                };
                sp = sp.add(1);
                val = t.wrapping_add(t);
                val >>= p;
                val &= !1u32;
                if val != 0 {
                    rho[0] += 8;
                    val = val.wrapping_sub(1);
                    e_q[3] = 32 - count_leading_zeros_u32(val) as i32;
                    e_qmax[0] = ojph_max(e_qmax[0], e_q[3]);
                    s[3] = val.wrapping_sub(1).wrapping_add(t >> 31);
                }
            }

            let kappa = if (rho[0] & (rho[0] - 1)) != 0 {
                ojph_max(1, max_e)
            } else {
                1
            };
            let uq0 = ojph_max(e_qmax[0], kappa);
            let u_q0 = uq0 - kappa;
            let mut u_q1 = 0;

            let mut eps0 = 0i32;
            if u_q0 > 0 {
                eps0 |= (e_q[0] == e_qmax[0]) as i32;
                eps0 |= ((e_q[1] == e_qmax[0]) as i32) << 1;
                eps0 |= ((e_q[2] == e_qmax[0]) as i32) << 2;
                eps0 |= ((e_q[3] == e_qmax[0]) as i32) << 3;
            }
            e_val[lep] = ojph_max(e_val[lep], e_q[1] as Ui8);
            lep += 1;
            max_e = ojph_max(e_val[lep], e_val[lep + 1]) as i32 - 1;
            e_val[lep] = e_q[3] as Ui8;
            cx_val[lcxp] |= ((rho[0] & 2) >> 1) as Ui8;
            lcxp += 1;
            let mut c_q1 = cx_val[lcxp] as i32 + ((cx_val[lcxp + 1] as i32) << 2);
            cx_val[lcxp] = ((rho[0] & 8) >> 3) as Ui8;
            let tuple0 = vlc_tbl1[((c_q0 << 8) + (rho[0] << 4) + eps0) as usize];
            vlc_encode(&mut vlc, (tuple0 >> 8) as i32, ((tuple0 >> 4) & 7) as i32)?;
            if c_q0 == 0 {
                mel_encode(&mut mel, rho[0] != 0)?;
            }

            let mut m = if (rho[0] & 1) != 0 {
                uq0 - (tuple0 & 1) as i32
            } else {
                0
            };
            ms_encode(&mut ms, s[0] & ((1u32 << m) - 1), m)?;
            m = if (rho[0] & 2) != 0 {
                uq0 - ((tuple0 & 2) >> 1) as i32
            } else {
                0
            };
            ms_encode(&mut ms, s[1] & ((1u32 << m) - 1), m)?;
            m = if (rho[0] & 4) != 0 {
                uq0 - ((tuple0 & 4) >> 2) as i32
            } else {
                0
            };
            ms_encode(&mut ms, s[2] & ((1u32 << m) - 1), m)?;
            m = if (rho[0] & 8) != 0 {
                uq0 - ((tuple0 & 8) >> 3) as i32
            } else {
                0
            };
            ms_encode(&mut ms, s[3] & ((1u32 << m) - 1), m)?;
            if x + 2 < width {
                t = *sp;
                val = t.wrapping_add(t);
                val >>= p;
                val &= !1u32;
                if val != 0 {
                    rho[1] = 1;
                    val = val.wrapping_sub(1);
                    e_q[4] = 32 - count_leading_zeros_u32(val) as i32;
                    e_qmax[1] = e_q[4];
                    s[4] = val.wrapping_sub(1).wrapping_add(t >> 31);
                }

                t = if y + 1 < height {
                    *sp.add(stride as usize)
                } else {
                    0
                };
                sp = sp.add(1);
                val = t.wrapping_add(t);
                val >>= p;
                val &= !1u32;
                if val != 0 {
                    rho[1] += 2;
                    val = val.wrapping_sub(1);
                    e_q[5] = 32 - count_leading_zeros_u32(val) as i32;
                    e_qmax[1] = ojph_max(e_qmax[1], e_q[5]);
                    s[5] = val.wrapping_sub(1).wrapping_add(t >> 31);
                }

                if x + 3 < width {
                    t = *sp;
                    val = t.wrapping_add(t);
                    val >>= p;
                    val &= !1u32;
                    if val != 0 {
                        rho[1] += 4;
                        val = val.wrapping_sub(1);
                        e_q[6] = 32 - count_leading_zeros_u32(val) as i32;
                        e_qmax[1] = ojph_max(e_qmax[1], e_q[6]);
                        s[6] = val.wrapping_sub(1).wrapping_add(t >> 31);
                    }

                    t = if y + 1 < height {
                        *sp.add(stride as usize)
                    } else {
                        0
                    };
                    sp = sp.add(1);
                    val = t.wrapping_add(t);
                    val >>= p;
                    val &= !1u32;
                    if val != 0 {
                        rho[1] += 8;
                        val = val.wrapping_sub(1);
                        e_q[7] = 32 - count_leading_zeros_u32(val) as i32;
                        e_qmax[1] = ojph_max(e_qmax[1], e_q[7]);
                        s[7] = val.wrapping_sub(1).wrapping_add(t >> 31);
                    }
                }

                let kappa = if (rho[1] & (rho[1] - 1)) != 0 {
                    ojph_max(1, max_e)
                } else {
                    1
                };
                c_q1 |= ((rho[0] & 4) >> 1) | ((rho[0] & 8) >> 2);
                let uq1 = ojph_max(e_qmax[1], kappa);
                u_q1 = uq1 - kappa;

                let mut eps1 = 0i32;
                if u_q1 > 0 {
                    eps1 |= (e_q[4] == e_qmax[1]) as i32;
                    eps1 |= ((e_q[5] == e_qmax[1]) as i32) << 1;
                    eps1 |= ((e_q[6] == e_qmax[1]) as i32) << 2;
                    eps1 |= ((e_q[7] == e_qmax[1]) as i32) << 3;
                }
                e_val[lep] = ojph_max(e_val[lep], e_q[5] as Ui8);
                lep += 1;
                max_e = ojph_max(e_val[lep], e_val[lep + 1]) as i32 - 1;
                e_val[lep] = e_q[7] as Ui8;
                cx_val[lcxp] |= ((rho[1] & 2) >> 1) as Ui8;
                lcxp += 1;
                c_q0 = cx_val[lcxp] as i32 + ((cx_val[lcxp + 1] as i32) << 2);
                cx_val[lcxp] = ((rho[1] & 8) >> 3) as Ui8;
                let tuple1 = vlc_tbl1[((c_q1 << 8) + (rho[1] << 4) + eps1) as usize];
                vlc_encode(&mut vlc, (tuple1 >> 8) as i32, ((tuple1 >> 4) & 7) as i32)?;
                if c_q1 == 0 {
                    mel_encode(&mut mel, rho[1] != 0)?;
                }

                m = if (rho[1] & 1) != 0 {
                    uq1 - (tuple1 & 1) as i32
                } else {
                    0
                };
                ms_encode(&mut ms, s[4] & ((1u32 << m) - 1), m)?;
                m = if (rho[1] & 2) != 0 {
                    uq1 - ((tuple1 & 2) >> 1) as i32
                } else {
                    0
                };
                ms_encode(&mut ms, s[5] & ((1u32 << m) - 1), m)?;
                m = if (rho[1] & 4) != 0 {
                    uq1 - ((tuple1 & 4) >> 2) as i32
                } else {
                    0
                };
                ms_encode(&mut ms, s[6] & ((1u32 << m) - 1), m)?;
                m = if (rho[1] & 8) != 0 {
                    uq1 - ((tuple1 & 8) >> 3) as i32
                } else {
                    0
                };
                ms_encode(&mut ms, s[7] & ((1u32 << m) - 1), m)?;
            }

            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q0 as usize].pre as i32,
                uvlc_tbl[u_q0 as usize].pre_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q1 as usize].pre as i32,
                uvlc_tbl[u_q1 as usize].pre_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q0 as usize].suf as i32,
                uvlc_tbl[u_q0 as usize].suf_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q1 as usize].suf as i32,
                uvlc_tbl[u_q1 as usize].suf_len as i32,
            )?;

            c_q0 |= ((rho[1] & 4) >> 1) | ((rho[1] & 8) >> 2);
            s = [0; 8];
            e_q = [0; 8];
            rho = [0, 0];
            e_qmax = [0, 0];
            x += 4;
        }

        y += 2;
    }

    terminate_mel_vlc(&mut mel, &mut vlc)?;
    ms_terminate(&mut ms)?;
    store_encoded_block(&mel, &vlc, &ms, lengths, elastic, coded);
    Ok(())
}

pub(crate) unsafe fn ojph_encode_codeblock64(
    buf: *mut Ui64,
    missing_msbs: Ui32,
    num_passes: Ui32,
    width: Ui32,
    height: Ui32,
    stride: Ui32,
    lengths: &mut [Ui32; 2],
    elastic: *mut mem_elastic_allocator,
    coded: &mut *mut CodedLists,
) -> crate::openjph::ojph_message::OjphResult<()> {
    debug_assert!(num_passes == 1);
    let ms_size = (22528 * 16 + 14) / 15;
    let mut ms_buf = [0u8; (22528 * 16 + 14) / 15];
    let mel_vlc_size = 3072usize;
    let mut mel_vlc_buf = [0u8; 3072];
    let mel_size = 192usize;
    let mel_buf = mel_vlc_buf.as_mut_ptr();
    let vlc_size = mel_vlc_size - mel_size;
    let vlc_buf = mel_buf.add(mel_size);

    let mut mel = mel_struct::default();
    mel_init(&mut mel, mel_size as Ui32, mel_buf);
    let mut vlc = vlc_struct::default();
    vlc_init(&mut vlc, vlc_size as Ui32, vlc_buf);
    let mut ms = ms_struct::default();
    ms_init(&mut ms, ms_size as Ui32, ms_buf.as_mut_ptr());

    let p = 62 - missing_msbs;

    let mut e_val = [0u8; 513];
    let mut cx_val = [0u8; 513];
    let mut lep = 0usize;
    let mut lcxp = 0usize;
    e_val[lep] = 0;
    cx_val[lcxp] = 0;

    let mut e_qmax = [0i32, 0];
    let mut e_q = [0i32; 8];
    let mut rho = [0i32, 0];
    let mut c_q0 = 0i32;
    let mut s = [0u64; 8];
    let mut sp = buf;
    let mut x = 0u32;
    while x < width {
        let mut t = *sp;
        let mut val = t.wrapping_add(t);
        val >>= p;
        val &= !1u64;
        if val != 0 {
            rho[0] = 1;
            val = val.wrapping_sub(1);
            e_q[0] = 64 - count_leading_zeros_u64(val) as i32;
            e_qmax[0] = e_q[0];
            s[0] = val.wrapping_sub(1).wrapping_add(t >> 63);
        }

        t = if height > 1 {
            *sp.add(stride as usize)
        } else {
            0
        };
        sp = sp.add(1);
        val = t.wrapping_add(t);
        val >>= p;
        val &= !1u64;
        if val != 0 {
            rho[0] += 2;
            val = val.wrapping_sub(1);
            e_q[1] = 64 - count_leading_zeros_u64(val) as i32;
            e_qmax[0] = ojph_max(e_qmax[0], e_q[1]);
            s[1] = val.wrapping_sub(1).wrapping_add(t >> 63);
        }

        if x + 1 < width {
            t = *sp;
            val = t.wrapping_add(t);
            val >>= p;
            val &= !1u64;
            if val != 0 {
                rho[0] += 4;
                val = val.wrapping_sub(1);
                e_q[2] = 64 - count_leading_zeros_u64(val) as i32;
                e_qmax[0] = ojph_max(e_qmax[0], e_q[2]);
                s[2] = val.wrapping_sub(1).wrapping_add(t >> 63);
            }

            t = if height > 1 {
                *sp.add(stride as usize)
            } else {
                0
            };
            sp = sp.add(1);
            val = t.wrapping_add(t);
            val >>= p;
            val &= !1u64;
            if val != 0 {
                rho[0] += 8;
                val = val.wrapping_sub(1);
                e_q[3] = 64 - count_leading_zeros_u64(val) as i32;
                e_qmax[0] = ojph_max(e_qmax[0], e_q[3]);
                s[3] = val.wrapping_sub(1).wrapping_add(t >> 63);
            }
        }

        let uq0 = ojph_max(e_qmax[0], 1);
        let u_q0 = uq0 - 1;
        let mut u_q1 = 0;

        let mut eps0 = 0i32;
        if u_q0 > 0 {
            eps0 |= (e_q[0] == e_qmax[0]) as i32;
            eps0 |= ((e_q[1] == e_qmax[0]) as i32) << 1;
            eps0 |= ((e_q[2] == e_qmax[0]) as i32) << 2;
            eps0 |= ((e_q[3] == e_qmax[0]) as i32) << 3;
        }
        e_val[lep] = ojph_max(e_val[lep], e_q[1] as Ui8);
        lep += 1;
        e_val[lep] = e_q[3] as Ui8;
        cx_val[lcxp] |= ((rho[0] & 2) >> 1) as Ui8;
        lcxp += 1;
        cx_val[lcxp] = ((rho[0] & 8) >> 3) as Ui8;

        let tuple0 = vlc_tbl0[((c_q0 << 8) + (rho[0] << 4) + eps0) as usize];
        vlc_encode(&mut vlc, (tuple0 >> 8) as i32, ((tuple0 >> 4) & 7) as i32)?;
        if c_q0 == 0 {
            mel_encode(&mut mel, rho[0] != 0)?;
        }

        let mut m = if (rho[0] & 1) != 0 {
            uq0 - (tuple0 & 1) as i32
        } else {
            0
        };
        ms_encode64(&mut ms, s[0] & ((1u64 << m) - 1), m)?;
        m = if (rho[0] & 2) != 0 {
            uq0 - ((tuple0 & 2) >> 1) as i32
        } else {
            0
        };
        ms_encode64(&mut ms, s[1] & ((1u64 << m) - 1), m)?;
        m = if (rho[0] & 4) != 0 {
            uq0 - ((tuple0 & 4) >> 2) as i32
        } else {
            0
        };
        ms_encode64(&mut ms, s[2] & ((1u64 << m) - 1), m)?;
        m = if (rho[0] & 8) != 0 {
            uq0 - ((tuple0 & 8) >> 3) as i32
        } else {
            0
        };
        ms_encode64(&mut ms, s[3] & ((1u64 << m) - 1), m)?;
        if x + 2 < width {
            t = *sp;
            val = t.wrapping_add(t);
            val >>= p;
            val &= !1u64;
            if val != 0 {
                rho[1] = 1;
                val = val.wrapping_sub(1);
                e_q[4] = 64 - count_leading_zeros_u64(val) as i32;
                e_qmax[1] = e_q[4];
                s[4] = val.wrapping_sub(1).wrapping_add(t >> 63);
            }

            t = if height > 1 {
                *sp.add(stride as usize)
            } else {
                0
            };
            sp = sp.add(1);
            val = t.wrapping_add(t);
            val >>= p;
            val &= !1u64;
            if val != 0 {
                rho[1] += 2;
                val = val.wrapping_sub(1);
                e_q[5] = 64 - count_leading_zeros_u64(val) as i32;
                e_qmax[1] = ojph_max(e_qmax[1], e_q[5]);
                s[5] = val.wrapping_sub(1).wrapping_add(t >> 63);
            }

            if x + 3 < width {
                t = *sp;
                val = t.wrapping_add(t);
                val >>= p;
                val &= !1u64;
                if val != 0 {
                    rho[1] += 4;
                    val = val.wrapping_sub(1);
                    e_q[6] = 64 - count_leading_zeros_u64(val) as i32;
                    e_qmax[1] = ojph_max(e_qmax[1], e_q[6]);
                    s[6] = val.wrapping_sub(1).wrapping_add(t >> 63);
                }

                t = if height > 1 {
                    *sp.add(stride as usize)
                } else {
                    0
                };
                sp = sp.add(1);
                val = t.wrapping_add(t);
                val >>= p;
                val &= !1u64;
                if val != 0 {
                    rho[1] += 8;
                    val = val.wrapping_sub(1);
                    e_q[7] = 64 - count_leading_zeros_u64(val) as i32;
                    e_qmax[1] = ojph_max(e_qmax[1], e_q[7]);
                    s[7] = val.wrapping_sub(1).wrapping_add(t >> 63);
                }
            }

            let c_q1 = (rho[0] >> 1) | (rho[0] & 1);
            let uq1 = ojph_max(e_qmax[1], 1);
            u_q1 = uq1 - 1;

            let mut eps1 = 0i32;
            if u_q1 > 0 {
                eps1 |= (e_q[4] == e_qmax[1]) as i32;
                eps1 |= ((e_q[5] == e_qmax[1]) as i32) << 1;
                eps1 |= ((e_q[6] == e_qmax[1]) as i32) << 2;
                eps1 |= ((e_q[7] == e_qmax[1]) as i32) << 3;
            }
            e_val[lep] = ojph_max(e_val[lep], e_q[5] as Ui8);
            lep += 1;
            e_val[lep] = e_q[7] as Ui8;
            cx_val[lcxp] |= ((rho[1] & 2) >> 1) as Ui8;
            lcxp += 1;
            cx_val[lcxp] = ((rho[1] & 8) >> 3) as Ui8;
            let tuple1 = vlc_tbl0[((c_q1 << 8) + (rho[1] << 4) + eps1) as usize];
            vlc_encode(&mut vlc, (tuple1 >> 8) as i32, ((tuple1 >> 4) & 7) as i32)?;
            if c_q1 == 0 {
                mel_encode(&mut mel, rho[1] != 0)?;
            }

            m = if (rho[1] & 1) != 0 {
                uq1 - (tuple1 & 1) as i32
            } else {
                0
            };
            ms_encode64(&mut ms, s[4] & ((1u64 << m) - 1), m)?;
            m = if (rho[1] & 2) != 0 {
                uq1 - ((tuple1 & 2) >> 1) as i32
            } else {
                0
            };
            ms_encode64(&mut ms, s[5] & ((1u64 << m) - 1), m)?;
            m = if (rho[1] & 4) != 0 {
                uq1 - ((tuple1 & 4) >> 2) as i32
            } else {
                0
            };
            ms_encode64(&mut ms, s[6] & ((1u64 << m) - 1), m)?;
            m = if (rho[1] & 8) != 0 {
                uq1 - ((tuple1 & 8) >> 3) as i32
            } else {
                0
            };
            ms_encode64(&mut ms, s[7] & ((1u64 << m) - 1), m)?;
        }

        if u_q0 > 0 && u_q1 > 0 {
            mel_encode(&mut mel, ojph_min(u_q0, u_q1) > 2)?;
        }

        if u_q0 > 2 && u_q1 > 2 {
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q0 as usize - 2].pre as i32,
                uvlc_tbl[u_q0 as usize - 2].pre_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q1 as usize - 2].pre as i32,
                uvlc_tbl[u_q1 as usize - 2].pre_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q0 as usize - 2].suf as i32,
                uvlc_tbl[u_q0 as usize - 2].suf_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q1 as usize - 2].suf as i32,
                uvlc_tbl[u_q1 as usize - 2].suf_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q0 as usize - 2].ext as i32,
                uvlc_tbl[u_q0 as usize - 2].ext_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q1 as usize - 2].ext as i32,
                uvlc_tbl[u_q1 as usize - 2].ext_len as i32,
            )?;
        } else if u_q0 > 2 && u_q1 > 0 {
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q0 as usize].pre as i32,
                uvlc_tbl[u_q0 as usize].pre_len as i32,
            )?;
            vlc_encode(&mut vlc, u_q1 - 1, 1)?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q0 as usize].suf as i32,
                uvlc_tbl[u_q0 as usize].suf_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q0 as usize].ext as i32,
                uvlc_tbl[u_q0 as usize].ext_len as i32,
            )?;
        } else {
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q0 as usize].pre as i32,
                uvlc_tbl[u_q0 as usize].pre_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q1 as usize].pre as i32,
                uvlc_tbl[u_q1 as usize].pre_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q0 as usize].suf as i32,
                uvlc_tbl[u_q0 as usize].suf_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q1 as usize].suf as i32,
                uvlc_tbl[u_q1 as usize].suf_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q0 as usize].ext as i32,
                uvlc_tbl[u_q0 as usize].ext_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q1 as usize].ext as i32,
                uvlc_tbl[u_q1 as usize].ext_len as i32,
            )?;
        }

        c_q0 = (rho[1] >> 1) | (rho[1] & 1);
        s = [0; 8];
        e_q = [0; 8];
        rho = [0, 0];
        e_qmax = [0, 0];
        x += 4;
    }

    e_val[lep + 1] = 0;

    let mut y = 2u32; // the first row pair was coded above; then two rows per step
    while y < height {
        lep = 0;
        let mut max_e = ojph_max(e_val[lep], e_val[lep + 1]) as i32 - 1;
        e_val[lep] = 0;
        lcxp = 0;
        c_q0 = cx_val[lcxp] as i32 + ((cx_val[lcxp + 1] as i32) << 2);
        cx_val[lcxp] = 0;

        sp = buf.add((y * stride) as usize);
        x = 0;
        while x < width {
            let mut t = *sp;
            let mut val = t.wrapping_add(t);
            val >>= p;
            val &= !1u64;
            if val != 0 {
                rho[0] = 1;
                val = val.wrapping_sub(1);
                e_q[0] = 64 - count_leading_zeros_u64(val) as i32;
                e_qmax[0] = e_q[0];
                s[0] = val.wrapping_sub(1).wrapping_add(t >> 63);
            }

            t = if y + 1 < height {
                *sp.add(stride as usize)
            } else {
                0
            };
            sp = sp.add(1);
            val = t.wrapping_add(t);
            val >>= p;
            val &= !1u64;
            if val != 0 {
                rho[0] += 2;
                val = val.wrapping_sub(1);
                e_q[1] = 64 - count_leading_zeros_u64(val) as i32;
                e_qmax[0] = ojph_max(e_qmax[0], e_q[1]);
                s[1] = val.wrapping_sub(1).wrapping_add(t >> 63);
            }

            if x + 1 < width {
                t = *sp;
                val = t.wrapping_add(t);
                val >>= p;
                val &= !1u64;
                if val != 0 {
                    rho[0] += 4;
                    val = val.wrapping_sub(1);
                    e_q[2] = 64 - count_leading_zeros_u64(val) as i32;
                    e_qmax[0] = ojph_max(e_qmax[0], e_q[2]);
                    s[2] = val.wrapping_sub(1).wrapping_add(t >> 63);
                }

                t = if y + 1 < height {
                    *sp.add(stride as usize)
                } else {
                    0
                };
                sp = sp.add(1);
                val = t.wrapping_add(t);
                val >>= p;
                val &= !1u64;
                if val != 0 {
                    rho[0] += 8;
                    val = val.wrapping_sub(1);
                    e_q[3] = 64 - count_leading_zeros_u64(val) as i32;
                    e_qmax[0] = ojph_max(e_qmax[0], e_q[3]);
                    s[3] = val.wrapping_sub(1).wrapping_add(t >> 63);
                }
            }

            let kappa = if (rho[0] & (rho[0] - 1)) != 0 {
                ojph_max(1, max_e)
            } else {
                1
            };
            let uq0 = ojph_max(e_qmax[0], kappa);
            let u_q0 = uq0 - kappa;
            let mut u_q1 = 0;

            let mut eps0 = 0i32;
            if u_q0 > 0 {
                eps0 |= (e_q[0] == e_qmax[0]) as i32;
                eps0 |= ((e_q[1] == e_qmax[0]) as i32) << 1;
                eps0 |= ((e_q[2] == e_qmax[0]) as i32) << 2;
                eps0 |= ((e_q[3] == e_qmax[0]) as i32) << 3;
            }
            e_val[lep] = ojph_max(e_val[lep], e_q[1] as Ui8);
            lep += 1;
            max_e = ojph_max(e_val[lep], e_val[lep + 1]) as i32 - 1;
            e_val[lep] = e_q[3] as Ui8;
            cx_val[lcxp] |= ((rho[0] & 2) >> 1) as Ui8;
            lcxp += 1;
            let mut c_q1 = cx_val[lcxp] as i32 + ((cx_val[lcxp + 1] as i32) << 2);
            cx_val[lcxp] = ((rho[0] & 8) >> 3) as Ui8;
            let tuple0 = vlc_tbl1[((c_q0 << 8) + (rho[0] << 4) + eps0) as usize];
            vlc_encode(&mut vlc, (tuple0 >> 8) as i32, ((tuple0 >> 4) & 7) as i32)?;
            if c_q0 == 0 {
                mel_encode(&mut mel, rho[0] != 0)?;
            }

            let mut m = if (rho[0] & 1) != 0 {
                uq0 - (tuple0 & 1) as i32
            } else {
                0
            };
            ms_encode64(&mut ms, s[0] & ((1u64 << m) - 1), m)?;
            m = if (rho[0] & 2) != 0 {
                uq0 - ((tuple0 & 2) >> 1) as i32
            } else {
                0
            };
            ms_encode64(&mut ms, s[1] & ((1u64 << m) - 1), m)?;
            m = if (rho[0] & 4) != 0 {
                uq0 - ((tuple0 & 4) >> 2) as i32
            } else {
                0
            };
            ms_encode64(&mut ms, s[2] & ((1u64 << m) - 1), m)?;
            m = if (rho[0] & 8) != 0 {
                uq0 - ((tuple0 & 8) >> 3) as i32
            } else {
                0
            };
            ms_encode64(&mut ms, s[3] & ((1u64 << m) - 1), m)?;
            if x + 2 < width {
                t = *sp;
                val = t.wrapping_add(t);
                val >>= p;
                val &= !1u64;
                if val != 0 {
                    rho[1] = 1;
                    val = val.wrapping_sub(1);
                    e_q[4] = 64 - count_leading_zeros_u64(val) as i32;
                    e_qmax[1] = e_q[4];
                    s[4] = val.wrapping_sub(1).wrapping_add(t >> 63);
                }

                t = if y + 1 < height {
                    *sp.add(stride as usize)
                } else {
                    0
                };
                sp = sp.add(1);
                val = t.wrapping_add(t);
                val >>= p;
                val &= !1u64;
                if val != 0 {
                    rho[1] += 2;
                    val = val.wrapping_sub(1);
                    e_q[5] = 64 - count_leading_zeros_u64(val) as i32;
                    e_qmax[1] = ojph_max(e_qmax[1], e_q[5]);
                    s[5] = val.wrapping_sub(1).wrapping_add(t >> 63);
                }

                if x + 3 < width {
                    t = *sp;
                    val = t.wrapping_add(t);
                    val >>= p;
                    val &= !1u64;
                    if val != 0 {
                        rho[1] += 4;
                        val = val.wrapping_sub(1);
                        e_q[6] = 64 - count_leading_zeros_u64(val) as i32;
                        e_qmax[1] = ojph_max(e_qmax[1], e_q[6]);
                        s[6] = val.wrapping_sub(1).wrapping_add(t >> 63);
                    }

                    t = if y + 1 < height {
                        *sp.add(stride as usize)
                    } else {
                        0
                    };
                    sp = sp.add(1);
                    val = t.wrapping_add(t);
                    val >>= p;
                    val &= !1u64;
                    if val != 0 {
                        rho[1] += 8;
                        val = val.wrapping_sub(1);
                        e_q[7] = 64 - count_leading_zeros_u64(val) as i32;
                        e_qmax[1] = ojph_max(e_qmax[1], e_q[7]);
                        s[7] = val.wrapping_sub(1).wrapping_add(t >> 63);
                    }
                }

                let kappa = if (rho[1] & (rho[1] - 1)) != 0 {
                    ojph_max(1, max_e)
                } else {
                    1
                };
                c_q1 |= ((rho[0] & 4) >> 1) | ((rho[0] & 8) >> 2);
                let uq1 = ojph_max(e_qmax[1], kappa);
                u_q1 = uq1 - kappa;

                let mut eps1 = 0i32;
                if u_q1 > 0 {
                    eps1 |= (e_q[4] == e_qmax[1]) as i32;
                    eps1 |= ((e_q[5] == e_qmax[1]) as i32) << 1;
                    eps1 |= ((e_q[6] == e_qmax[1]) as i32) << 2;
                    eps1 |= ((e_q[7] == e_qmax[1]) as i32) << 3;
                }
                e_val[lep] = ojph_max(e_val[lep], e_q[5] as Ui8);
                lep += 1;
                max_e = ojph_max(e_val[lep], e_val[lep + 1]) as i32 - 1;
                e_val[lep] = e_q[7] as Ui8;
                cx_val[lcxp] |= ((rho[1] & 2) >> 1) as Ui8;
                lcxp += 1;
                c_q0 = cx_val[lcxp] as i32 + ((cx_val[lcxp + 1] as i32) << 2);
                cx_val[lcxp] = ((rho[1] & 8) >> 3) as Ui8;
                let tuple1 = vlc_tbl1[((c_q1 << 8) + (rho[1] << 4) + eps1) as usize];
                vlc_encode(&mut vlc, (tuple1 >> 8) as i32, ((tuple1 >> 4) & 7) as i32)?;
                if c_q1 == 0 {
                    mel_encode(&mut mel, rho[1] != 0)?;
                }

                m = if (rho[1] & 1) != 0 {
                    uq1 - (tuple1 & 1) as i32
                } else {
                    0
                };
                ms_encode64(&mut ms, s[4] & ((1u64 << m) - 1), m)?;
                m = if (rho[1] & 2) != 0 {
                    uq1 - ((tuple1 & 2) >> 1) as i32
                } else {
                    0
                };
                ms_encode64(&mut ms, s[5] & ((1u64 << m) - 1), m)?;
                m = if (rho[1] & 4) != 0 {
                    uq1 - ((tuple1 & 4) >> 2) as i32
                } else {
                    0
                };
                ms_encode64(&mut ms, s[6] & ((1u64 << m) - 1), m)?;
                m = if (rho[1] & 8) != 0 {
                    uq1 - ((tuple1 & 8) >> 3) as i32
                } else {
                    0
                };
                ms_encode64(&mut ms, s[7] & ((1u64 << m) - 1), m)?;
            }

            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q0 as usize].pre as i32,
                uvlc_tbl[u_q0 as usize].pre_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q1 as usize].pre as i32,
                uvlc_tbl[u_q1 as usize].pre_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q0 as usize].suf as i32,
                uvlc_tbl[u_q0 as usize].suf_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q1 as usize].suf as i32,
                uvlc_tbl[u_q1 as usize].suf_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q0 as usize].ext as i32,
                uvlc_tbl[u_q0 as usize].ext_len as i32,
            )?;
            vlc_encode(
                &mut vlc,
                uvlc_tbl[u_q1 as usize].ext as i32,
                uvlc_tbl[u_q1 as usize].ext_len as i32,
            )?;

            c_q0 |= ((rho[1] & 4) >> 1) | ((rho[1] & 8) >> 2);
            s = [0; 8];
            e_q = [0; 8];
            rho = [0, 0];
            e_qmax = [0, 0];
            x += 4;
        }

        y += 2;
    }

    terminate_mel_vlc(&mut mel, &mut vlc)?;
    ms_terminate(&mut ms)?;
    store_encoded_block(&mel, &vlc, &ms, lengths, elastic, coded);
    Ok(())
}
