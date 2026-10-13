# CONTRACTS.md — exact codec contracts extracted from the consumer ports

Source of truth for implementing the codec-specific families WITHOUT
guessing.  Extracted verbatim from the sibling codec crates under
`C:\projects\projects.rust.cg\cglibs\`.  Each entry: the exact constants,
rounding offsets and shift schedule, with file references.

The crate stays codec-agnostic where a single parameterized primitive can
serve multiple codecs (caller supplies constants/shift); where the
algorithm itself differs (HEVC DCT vs H.264 integer transform vs DST),
separate primitives are warranted.

---

## HEVC — `kvazaar-rs`

### Already satisfied by existing codec-simd primitives
- **dequant**: `coeff = (q_coef * scale + (1<<(shift-1))) >> shift`,
  `scale = INV_QUANT_SCALES[qp%6] << (qp/6)`,
  `INV_QUANT_SCALES = [40,45,51,57,64,72]`,
  `shift = 20 - 14 - transform_shift`.
  → **`dequant_block_i16(buf, scale, shift)`** is exactly this (caller
  computes `scale`,`shift`).  IDENTIFIED CALLER. ✅
- **SATD 4×4**: 4×4 Hadamard then `sum(|d|)` with the `(sum+1)>>1`
  normalization → **`satd_block_4x4_u8`** / **`hadamard_4x4_i16`**. ✅
- **SAD**: `Σ|a-b|` then `>> (bitdepth-8)` → **`sad_block_u8`** (the
  bitdepth shift is the caller's). ✅
- **8×8 SATD**: 8×8 Hadamard then `(sad+2)>>2` → needs **`hadamard_8x8_i16`**
  (IDENTIFIED CALLER → justifies adding it).

### Inverse transform (to implement) — `dct_tables.rs`, `kvz_transform.rs`
- **DCT-4 matrix** `[[64,64,64,64],[83,36,-36,-83],[64,-64,-64,64],[36,-83,83,-36]]`.
- **DST-VII 4×4** (intra 4×4 luma only)
  `[[29,55,74,84],[74,74,0,-74],[84,-29,-74,55],[55,-84,74,-29]]`.
- **8/16/32 DCT** tables `KVZ_G_DCT_{8,16,32}`.
- **Inverse 2D shifts**: `shift_1st = 7` (constant), `shift_2nd = 12 - (bitdepth-8)`.
  Rounding `add = 1<<(shift-1)` per pass.
- Standard inverse DCT-4 partial butterfly (per column then row):
  `E0=64*s0+64*s2; E1=64*s0-64*s2; O0=83*s1+36*s3; O1=36*s1-83*s3;`
  `d0=(E0+O0+add)>>shift; d3=(E0-O0+add)>>shift; d1=(E1+O1+add)>>shift; d2=(E1-O1+add)>>shift`.
  → primitive `idct_4x4_add_u8` / `idst_4x4_add_u8` (v0.4.x), 2D
  (vert pass shift 7 → horiz pass shift 12 → add to pred → clip u8).

### Intra prediction — `kvz_predict.rs`
- modes: PLANAR=0, DC=1, HOR=10, VER=26; angular 2..34.
- `MODEDISP2SAMPLEDISP = [0,2,5,9,13,17,21,26,32]` (1/32-sample).
- `MODEDISP2INVSAMPLEDISP = [0,4096,1638,910,630,482,390,315,256]`.
- angular interp per sample: `((32-fract)*ref1 + fract*ref2 + 16) >> 5`,
  `delta_pos += sampledisp; delta_int = delta_pos>>5; delta_fract = delta_pos&31`.
- reference smoothing `[1 2 1]/4`; DC fallback `1<<(bitdepth-1)`.

### Deblock — `kvz_deblock.rs`
- `TC_TABLE[54]` indexed `qp + 2*(strength-1) + (tc_offset_div2<<1)` clip[0,53];
  `BETA_TABLE[52]` indexed `qp + (beta_offset_div2<<1)` clip[0,51].
  `bitdepth_scale = 1<<(bitdepth-8)`.
- strong luma (8-sample line, reaches idx 1..6), clip to `m±2tc`:
  `l1=(2m0+3m1+m2+m3+m4+4)>>3; l2=(m1+m2+m3+m4+2)>>2; l3=(m1+2m2+2m3+2m4+m5+4)>>3` (symmetric Q side).
- weak luma: `delta=(9*(m4-m3)-3*(m5-m2)+8)>>4`, abort if `|delta|>=tc*10`,
  clamp `[-tc,tc]`; secondary `delta1=clip(-tc/2,tc/2,(((m1+m3+1)>>1)-m2+delta)>>1)`.
- chroma: `delta=clip(-tc,tc,((m4-m3)*4 + m2-m5 + 4)>>3)`.

### Quant (encoder) — `kvz_transform.rs`
- `level = (|coeff|*QUANT_SCALES[qp%6] + add) >> q_bits`,
  `QUANT_SCALES=[26214,23302,20560,18396,16384,14564]`,
  `q_bits = 14 + qp/6 + transform_shift`, `add = (intra?171:85)<<(q_bits-9)`.
  + sign-data-hiding over 4×4 CGs.

---

## H.264 — `h264-rs`

### 4×4 integer IDCT + add (`transform.rs:54-98`)
Horizontal pass: `t0=s0+s2; t1=s0-s2; t2=(s1>>1)-s3; t3=s1+(s3>>1);`
`o0=t0+t3; o1=t1+t2; o2=t1-t2; o3=t0-t3`.  Vertical+add+clip
(`>>6`, round 32): `kt1=v0+v2; kt2=v1+(v3>>1); kt1b=v0-v2; kt2b=(v1>>1)-v3;`
`blk = clip1(((32 + kt1 ± kt2) >> 6) + pred, max)`.
### 8×8 integer IDCT (`transform.rs:651-727`)
8-point integer butterfly (see survey), add+clip identical `(32+res)>>6`.
### Luma DC iwht 4×4 (`transform.rs:114-166`)
row/col Hadamard, then `coeff = ((z + ...) * ki_q_mul + (1<<5)) >> 6`,
`ki_q_mul = DEQUANT_COEFF[qp][0] << (4 + shift)`.
### Chroma DC 2×2 (`transform.rs:180-200`) and 2×4 4:2:2 (`:215-240`)
2×2: `[e=a-b, a2=a+b, b2=c-d, c2=c+d] -> [a2+c2, e+b2, a2-c2, e-b2]`;
dequant `(val*scale)>>1`.  2×4: `((z)*qmul + 128) >> 8`.
### MC luma 6-tap (`mc.rs:54-87`)
`[1,-5,20,20,-5,1]`; half-pel `clip1((filt + 16) >> 5)`; centre
`clip1((row + 512) >> 10)`; qpel `avg=(a+b+1)>>1`.
### MC chroma bilinear (`mc.rs:24-247`)
`A=(8-dx)(8-dy), B=dx(8-dy), C=(8-dx)dy, D=dx*dy`;
`v=(A*p00+B*p10+C*p01+D*p11 + 32) >> 6`.
### Deblock (`tables.rs:791-834`, `deblock.rs:61-297`)
`ALPHA[76]`, `BETA[76]`, `TC0[76][4]`; indexed `(qp+offset+12).clamp(0,75)`,
`<<shift`.  lt4 (soft): `delta=clip(-tc,tc,((q0-p0)*4+(p1-q1)+4)>>3)`,
P1/Q1 conditional `((p2 + ((p0+q0+1)>>1) - 2*p1) >> 1)` clip±tc.
eq4 (strong) per spec (see survey).
### Weighted pred (`inter.rs:541-682`)
uni: `clip(((p*w + (1<<(log2d-1))) >> log2d) + o, 0, max)`;
implicit bi (log2d=5): `((p0*w1 + p1*(64-w1) + 32) >> 6)`;
explicit bi (a=log2d+1): `((w0*p0+w1*p1+(1<<(a-1)))>>a) + ((o0+o1+1)>>1)`.
→ all covered by `weighted_avg_u8/u16` (caller folds rounding/offset).

---

## APV — `apv-rs`
### IDCT DCT-II 8×8 (`transform.rs`, `tables.rs:37-46`)
`TBL_TM8 = [[64..],[89,75,50,18,-18,-50,-75,-89],[84,35,-35,-84,...],...]`.
shifts `ITX_SHIFT1=7`, `itx_shift2 = 20 - bit_depth`; `add=1<<(shift-1)`.
butterfly `dst[k]=((e[k]+o[k]+add)>>shift); dst[k+4]=((e[3-k]-o[3-k]+add)>>shift)`.
### Dequant (`quant.rs:55-87`) — PER-POSITION matrix
`s = bit_depth - 2 - qp/6`; if s>0: `lev=(coef[i]*q_matrix[i] + (1<<(s-1)))>>s`;
else left-shift; `clip16`.  → needs `dequant_block_matrix_i16`
(`coef[i]*q_matrix[i]`), since `dequant_block_i16` is uniform-scale only.
Forward: `QUANT_SCALE=[26214,23302,20560,18396,16384,14769]`, `QUANT_SHIFT=14`.
No MC (intra codec).

---

## HTJ2K — `jph-rs`
### DWT 5/3 reversible (`transform.rs:150-153`)
`if synthesis: *dst -= (b + s1 + s2) >> e; else: *dst += (b + s1 + s2) >> e`.
step0: `b=2, e=2`; step1: `b=1, e=1` (signs a=±1).
### DWT 9/7 irreversible (float)
lift coeffs `0.443506852043971, 0.882911075530934, -0.052980118572961,
-1.586134342059924`; norm `Katk=1.2301741`.
### RCT (reversible, `ojph_colour.rs:510,568`)
fwd: `Y=(R + 2*G + B) >> 2; Cb=B-G; Cr=R-G`.
bwd: `G=Y-((Cb+Cr)>>2); R=Cr+G; B=Cb+G`.
### ICT (irreversible float, `ojph_colour.rs:618,642`)
fwd: `Y=0.299R+0.587G+0.114B; Cb=0.564335(B-Y); Cr=0.714137(R-Y)`.
bwd: `G=Y-0.354083*Cr-0.114309*Cb; R=Y+1.402*Cr; B=Y+1.772*Cb`.

---

## HEVC decoder — `ffmpeg-rs`  (confirms kvazaar, decoder side)
### IDCT (`av-codec-h265/transform/idct.rs`)
DST-VII 4×4 (`29,55,74`), DCT-II 4/8/16/32 (`TRANSFORM[32][32]`,
`TR_4_CORE` uses `64,83,36`).  Pass1 shift `7`, pass2 shift `20 - bit_depth`.
DC-only: `clip16((((c0+1)>>1)+add) >> (14-bit_depth))`.
transform-skip shift `15 - bit_depth - log2_size`.  bit-depth {8,9,10,12}.
### MC (`av-codec-h265/mc.rs`)
luma QPEL `QPEL_FILTERS[4][8]`: `[-1,4,-10,58,17,-5,1,0]`,
`[-1,4,-11,40,40,-11,4,-1]`, `[0,1,-5,17,58,-10,4,-1]`; hpass `bd-8`,
vpass `6`, uni `14-bd`.  chroma EPEL `EPEL_FILTERS[8][4]`
`[-2,58,10,-2]`, `[-4,54,16,-2]`, ... `[-2,10,58,-2]`.
### add_residual (`transform.rs:60-71`)
`dst[u16] = clip(pred + res[i16], 0, (1<<bd)-1)`.  → `add_clip_to_pixel_u16`.
### Deblock (`filter/deblock.rs`) TC[54]/BETA[52], `<<(bd-8)`; strong/weak.
### SAO (`filter/sao.rs`) band shift `bd-5`, edge categories `2+sign+sign`.

---

## AV1 — `heif-rs` (av1 decoder)
DCT/ADST/flip-ADST 4-64 + Identity, ported from dav1d `itx_1d.c`,
i64 intermediates.  DCT-4: `t0=(in0*181+128)>>8; t2=(in1*1567+2048)>>12;
t3=(in1*3784+2048)>>12`.  Also mc/deblock/lr/filmgrain/ipred/warp.
(`heif-rs` HEVC path is a binding to `rust_h265` — no own DSP.)
