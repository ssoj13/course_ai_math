//! Exact per-pixel inverses of 1D and 3D LUTs: ports of OCIO's `InvLut1DRenderer`
//! (`ops/lut1d/Lut1DOpCPU.cpp`) and `InvLut3DRenderer` (`ops/lut3d/Lut3DOpCPU.cpp`).
//!
//! OCIO evaluates an inverse LUT this way on the CPU unless `OPTIMIZATION_LUT_INV_FAST` bakes it
//! into a forward LUT first; that flag is part of OCIO's default (VERY_GOOD) level, not of
//! LOSSLESS. This port always uses the exact form on the CPU (its default level is LOSSLESS and
//! the exact inverse is the more accurate one); the GPU, which cannot run it, gets the bake OCIO's
//! GPU path gets (`bake_grid`, `MakeFastLut3DFromInverse`), see `gpu.rs`.
//!
//! The previous inverses were tables with the forward LUT's own resolution (Newton per node for
//! 3D), off by up to 0.1 against OCIO on small LUTs.

/// One channel of an inverse 1D LUT, prepared as `InvLut1DRenderer::updateData` does.
#[derive(Debug, Clone, PartialEq)]
struct InvChannel {
    /// The forward values with reversals flattened, negated when the LUT decreases, so the
    /// search runs on an increasing table.
    lut: Vec<f32>,
    /// First and last effective entries (the ends of flat runs, `startDomain`/`endDomain`).
    start: usize,
    end: usize,
    /// -1 for a decreasing LUT.
    flip: f32,
    /// Half-domain only: effective domain of the negative half (codes 32768..) and the
    /// forward value at +0 that decides which half an input inverts in (`bisectPoint`).
    neg_start: usize,
    neg_end: usize,
    bisect: f32,
}

/// The exact inverse of a 1D LUT, uniform (`InvLut1DRenderer`) or half-domain
/// (`InvLut1DRendererHalfCode`).
#[derive(Debug, Clone, PartialEq)]
pub struct InvLut1d {
    channels: Vec<InvChannel>,
    /// `1 / (size - 1)`: index units to the forward LUT's normalised input.
    scale: f32,
    /// The forward LUT's input domain, which is the inverse's output.
    domain_min: [f32; 3],
    domain_max: [f32; 3],
    /// The forward LUT is indexed by half code (65536 entries); see [`Self::new_half`].
    half: bool,
}

/// `std::lower_bound` over `lut[start..end)` then OCIO's bracketing: the lower entry index and
/// the fraction of `cv` between it and the next (0 on a flat spot).
fn bracket(lut: &[f32], start: usize, end: usize, cv: f32) -> (usize, f32) {
    let mut lo = start + lut[start..end].partition_point(|x| *x < cv);
    if lo > start {
        lo -= 1;
    }
    let hi = if lo < end { lo + 1 } else { lo };
    let delta = if lut[hi] > lut[lo] {
        (cv - lut[lo]) / (lut[hi] - lut[lo])
    } else {
        0.0
    };
    (lo, delta)
}

impl InvLut1d {
    /// Prepares the inverse of a half-domain forward table (65536 entries, one per half code,
    /// `channels` interleaved). OCIO `Lut1DOpData::initializeFromForward` (half-domain branch:
    /// increasing is judged on the entries for 0 and 1.0, reversals are flattened in each half
    /// separately, each half has its own effective domain up to +/-65504) and
    /// `InvLut1DRendererHalfCode::updateData` (the negative half is sign reversed so both
    /// halves search an increasing table).
    pub fn new_half(data: &[f32], channels: usize) -> Self {
        const SIZE: usize = 65536;
        let channels = (0..channels)
            .map(|c| {
                let mut v: Vec<f32> = (0..SIZE).map(|i| data[i * channels + c]).collect();
                let bisect = v[0];
                let increasing = v[0] < v[15360];
                // OCIO walks `idx <= 31744 * maxChannels` / `64512 * maxChannels` with stride 3
                // from `c`, so the +/-infinity rows are only reached for channel 0.
                let inf_row = |row: usize| if c == 0 { row } else { row - 1 };
                let mut prev = v[0];
                for x in v.iter_mut().take(inf_row(31744) + 1).skip(1) {
                    if increasing != (*x > prev) {
                        *x = prev;
                    } else {
                        prev = *x;
                    }
                }
                // Negative half: the opposite sense, starting from +0 (no overlap).
                let mut prev = v[0];
                for x in v.iter_mut().take(inf_row(64512) + 1).skip(32768) {
                    if !increasing != (*x > prev) {
                        *x = prev;
                    } else {
                        prev = *x;
                    }
                }
                let mut end = 31743;
                while end > 0 && v[end - 1] == v[31743] {
                    end -= 1;
                }
                let mut start = 0;
                while start < end && v[start + 1] == v[0] {
                    start += 1;
                }
                let mut neg_end = 64511;
                while neg_end > 32768 && v[neg_end - 1] == v[64511] {
                    neg_end -= 1;
                }
                let mut neg_start = 32768;
                while neg_start < neg_end && v[neg_start + 1] == v[32768] {
                    neg_start += 1;
                }
                let flip = if increasing { 1.0 } else { -1.0 };
                let lut = v
                    .iter()
                    .enumerate()
                    .map(|(i, x)| if i < 32768 { x * flip } else { -x * flip })
                    .collect();
                InvChannel {
                    lut,
                    start,
                    end,
                    flip,
                    neg_start,
                    neg_end,
                    bisect,
                }
            })
            .collect();
        Self {
            channels,
            scale: 1.0,
            domain_min: [0.0; 3],
            domain_max: [1.0; 3],
            half: true,
        }
    }

    /// Prepares the inverse of a forward table (`channels` interleaved values per entry).
    /// OCIO `Lut1DOpData::initializeFromForward` + `InvLut1DRenderer::updateData`.
    pub fn new(
        data: &[f32],
        size: usize,
        channels: usize,
        domain_min: [f32; 3],
        domain_max: [f32; 3],
    ) -> Self {
        let channels = (0..channels)
            .map(|c| {
                let mut v: Vec<f32> = (0..size).map(|i| data[i * channels + c]).collect();
                // Flat LUTs count as decreasing, as OCIO (`values[low] < values[high]`).
                let increasing = v[0] < v[size - 1];
                // Flatten reversals: there is no unique inverse over them.
                let mut prev = v[0];
                for x in v.iter_mut().skip(1) {
                    if increasing != (*x > prev) {
                        *x = prev;
                    } else {
                        prev = *x;
                    }
                }
                // Effective domain from the flat runs at either end.
                let mut end = size - 1;
                while end > 0 && v[end - 1] == v[size - 1] {
                    end -= 1;
                }
                let mut start = 0;
                while start < end && v[start + 1] == v[0] {
                    start += 1;
                }
                let flip = if increasing { 1.0 } else { -1.0 };
                InvChannel {
                    lut: v.into_iter().map(|x| x * flip).collect(),
                    start,
                    end,
                    flip,
                    neg_start: 0,
                    neg_end: 0,
                    bisect: 0.0,
                }
            })
            .collect();
        Self {
            channels,
            scale: 1.0 / (size - 1) as f32,
            domain_min,
            domain_max,
            half: false,
        }
    }

    /// Whether the forward LUT is half-domain.
    pub fn is_half_domain(&self) -> bool {
        self.half
    }

    /// OCIO `FindLutInv` (or, half-domain, `InvLut1DRendererHalfCode::apply` + `FindLutInvHalf`)
    /// for channel `c` (the single channel of a mono LUT for every `c`), mapped into the forward
    /// LUT's input domain.
    pub fn eval(&self, c: usize, val: f32) -> f32 {
        let ch = &self.channels[c.min(self.channels.len() - 1)];
        if self.half {
            // OCIO passes the RED channel's sign for the blue negative-half search
            // (`-this->m_paramsR.flipSign`, Lut1DOpCPU.cpp:1516); kept for parity.
            let red_flip = self.channels[0].flip;
            let neg_flip = if c == 2 { -red_flip } else { -ch.flip };
            let increasing = ch.flip > 0.0;
            return if increasing == (val >= ch.bisect) {
                find_lut_inv_half(&ch.lut, ch.start, ch.end, ch.flip, val)
            } else {
                find_lut_inv_half(&ch.lut, ch.neg_start, ch.neg_end, neg_flip, val)
            };
        }
        let (lut, start, end) = (&ch.lut, ch.start, ch.end);
        // Clamp to the range of the LUT (NaN takes the low end, as `std::max` does).
        let cv = (val * ch.flip).max(lut[start]).min(lut[end]);
        let cv = if cv.is_nan() { lut[start] } else { cv };
        let (lo, delta) = bracket(lut, start, end, cv);
        let t = (lo as f32 + delta) * self.scale;
        let d = c.min(2);
        self.domain_min[d] + t * (self.domain_max[d] - self.domain_min[d])
    }
}

/// OCIO `FindLutInvHalf`: the search of `FindLutInv` over `lut[start..=end]` (absolute half
/// codes), then the fraction is spread over the two half values it lies between, since half
/// codes are not evenly spaced.
fn find_lut_inv_half(lut: &[f32], start: usize, end: usize, flip: f32, val: f32) -> f32 {
    let cv = (val * flip).max(lut[start]).min(lut[end]);
    let cv = if cv.is_nan() { lut[start] } else { cv };
    let (lo, delta) = bracket(lut, start, end, cv);
    let base = half::f16::from_bits(lo as u16).to_f32();
    let next = half::f16::from_bits((lo + 1) as u16).to_f32();
    base + delta * (next - base)
}

/// Grid of an extrapolated 3D LUT, Blue-fastest, 3 floats per node.
struct Grid<'a> {
    values: &'a [f32],
}

/// One level of OCIO's `RangeTree`.
#[derive(Debug, Clone, PartialEq, Default)]
struct TreeLevel {
    elems: usize,
    min_vals: Vec<f32>,
    max_vals: Vec<f32>,
    child0offsets: Vec<usize>,
    num_children: Vec<usize>,
}

/// The exact inverse of a 3D LUT (OCIO `InvLut3DRenderer`: the LUT is extrapolated by one node
/// on every side, a range tree over its cells finds the cells whose bounding box holds the value,
/// and `invert_hypercube` solves the tetrahedral interpolation inside the cell).
#[derive(Debug, Clone, PartialEq)]
pub struct InvLut3d {
    /// Extrapolated grid (`dim + 2` per side), Blue-fastest.
    grvec: Vec<f32>,
    /// `dim + 2`.
    gsz: usize,
    depth: usize,
    levels: Vec<TreeLevel>,
    /// Cell base indices in tree (hash) order.
    base_inds: Vec<[usize; 3]>,
    domain_min: [f32; 3],
    domain_max: [f32; 3],
}

impl InvLut3d {
    /// `data`: the forward LUT, Blue-fastest (`vfx_lut::Lut3D::data` order), `dim` per side.
    pub fn new(data: &[[f32; 3]], dim: usize, domain_min: [f32; 3], domain_max: [f32; 3]) -> Self {
        let gsz = dim + 2;
        let grvec = extrapolate(data, dim);
        let mut s = Self {
            grvec,
            gsz,
            depth: 0,
            levels: Vec::new(),
            base_inds: Vec::new(),
            domain_min,
            domain_max,
        };
        s.init_tree();
        s
    }

    /// `RangeTree::initialize`.
    fn init_tree(&mut self) {
        const CHANS: usize = 3;
        let g = self.gsz - 2;
        // frexp(g) exponent: floor(log2(g)) + 1.
        self.depth = (usize::BITS - g.leading_zeros()) as usize;
        let depth = self.depth;
        self.levels = (0..depth)
            .map(|i| {
                let m = g >> (depth - 1 - i);
                TreeLevel {
                    elems: (m + 1).pow(3),
                    ..Default::default()
                }
            })
            .collect();
        let level_scales: Vec<u64> = (0..depth)
            .map(|l| 1u64 << ((CHANS + 1) * (depth - 1 - l)))
            .collect();

        // initInds + indsToHash + sort by hash.
        let lim = self.gsz - 1;
        let mut inds: Vec<([usize; 3], u64)> = Vec::with_capacity(lim.pow(3));
        for i in 0..lim {
            for j in 0..lim {
                for k in 0..lim {
                    let b = [i, j, k];
                    let hash = (0..depth)
                        .map(|level| {
                            let bits: u64 = (0..CHANS)
                                .map(|ch| (((b[ch] >> (depth - 1 - level)) & 1) as u64) << ch)
                                .sum();
                            bits * level_scales[level]
                        })
                        .sum();
                    inds.push((b, hash));
                }
            }
        }
        inds.sort_by_key(|&(_, h)| h);
        self.base_inds = inds.iter().map(|&(b, _)| b).collect();
        let mut hashes: Vec<u64> = inds.iter().map(|&(_, h)| h).collect();

        // initRanges: bounding box of each cell's 8 corners, widened by 1e-6.
        let (s1, s0) = (self.gsz, self.gsz * self.gsz);
        let corners = [0, 1, s1, s1 + 1, s0, s0 + 1, s0 + s1, s0 + s1 + 1];
        let leaf = &mut self.levels[depth - 1];
        leaf.min_vals = vec![0.0; leaf.elems * CHANS];
        leaf.max_vals = vec![0.0; leaf.elems * CHANS];
        for (n, b) in self.base_inds.iter().enumerate() {
            let base = b[0] * s0 + b[1] * s1 + b[2];
            for k in 0..CHANS {
                let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
                for off in corners {
                    let v = self.grvec[(base + off) * CHANS + k];
                    lo = lo.min(v);
                    hi = hi.max(v);
                }
                leaf.min_vals[n * CHANS + k] = lo - 1e-6;
                leaf.max_vals[n * CHANS + k] = hi + 1e-6;
            }
        }

        // Work up the tree.
        for level in (0..depth - 1).rev() {
            let size = self.levels[level].elems;
            let gap = level_scales[level + 1] * 8;
            let mut child0 = vec![0usize];
            for i in 1..hashes.len() {
                if hashes[i] - hashes[i - 1] > gap {
                    child0.push(i);
                }
            }
            // The group count equals the level size by construction; pad/trim defensively.
            let last = *child0.last().unwrap_or(&0);
            child0.resize(size, last);
            let mut num = vec![0usize; size];
            for i in 0..size - 1 {
                num[i] = child0[i + 1] - child0[i];
            }
            num[size - 1] = hashes.len() - child0[size - 1];
            for i in 0..size {
                hashes[i] = hashes[child0[i]];
            }
            hashes.truncate(size);

            let (upper, lower) = self.levels.split_at_mut(level + 1);
            let (cur, next) = (&mut upper[level], &lower[0]);
            cur.min_vals = vec![0.0; size * CHANS];
            cur.max_vals = vec![0.0; size * CHANS];
            for i in 0..size {
                let index = child0[i];
                for k in 0..CHANS {
                    cur.min_vals[i * CHANS + k] = next.min_vals[index * CHANS + k];
                    cur.max_vals[i * CHANS + k] = next.max_vals[index * CHANS + k];
                }
                for j in 2..=8usize {
                    if num[i] >= j {
                        let ind = index + j - 1;
                        for k in 0..CHANS {
                            let v = next.min_vals[ind * CHANS + k];
                            if v < cur.min_vals[i * CHANS + k] {
                                cur.min_vals[i * CHANS + k] = v;
                            }
                            let v = next.max_vals[ind * CHANS + k];
                            if v > cur.max_vals[i * CHANS + k] {
                                cur.max_vals[i * CHANS + k] = v;
                            }
                        }
                    }
                }
            }
            cur.child0offsets = child0;
            cur.num_children = num;
        }
    }

    /// `InvLut3DRenderer::apply` for one pixel, mapped into the forward LUT's input domain.
    pub fn eval(&self, rgb: [f32; 3]) -> [f32; 3] {
        const CHANS: usize = 3;
        let gsz = self.gsz;
        let max_dim = (gsz - 3) as f32;
        let scale = 1.0 / max_dim;
        let mut offs = [gsz * gsz, gsz, 1];
        const LIST_LEN: usize = 8;
        let ops_list: [i64; 8] = [0, 0, 1, 1, 1, 1, 1, 1];
        let entering_list: [usize; 8] = [2, 1, 0, 2, 0, 2, 0, 2];
        let new_verts: [usize; 24] = [
            1, 0, 0, 1, 1, 1, 1, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0,
        ];
        let path_list: [usize; 24] = [
            0, 0, 0, 0, 0, 0, 0, 1, 2, 1, 0, 2, 1, 2, 0, 2, 1, 0, 2, 0, 1, 0, 2, 1,
        ];
        let path_order: [usize; 3] = [1, 0, 2];
        let new_vert_list: [usize; 8] = std::array::from_fn(|i| {
            new_verts[i * 3] * offs[0]
                + new_verts[i * 3 + 1] * offs[1]
                + new_verts[i * 3 + 2] * offs[2]
        });
        for o in offs.iter_mut() {
            *o *= CHANS;
        }

        // The extrapolated inverse still only covers so much: clamp, as OCIO.
        let clamp01 = |v: f32| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
        let (r, g, b) = (clamp01(rgb[0]), clamp01(rgb[1]), clamp01(rgb[2]));
        let depth = self.depth;
        let mut current_child = [0usize; 16];
        let mut current_num = [1usize; 16];
        let mut current_ind = [0usize; 16];
        current_num[0] = self.levels[0].child0offsets.len();
        let mut result = [0.0f32; 3];
        let grid = Grid {
            values: &self.grvec,
        };

        let mut level: i64 = 0;
        'outer: while level >= 0 {
            let l = level as usize;
            while current_child[l] < current_num[l] {
                let lv = &self.levels[l];
                let node = current_ind[l];
                let in_range = r >= lv.min_vals[node * CHANS]
                    && g >= lv.min_vals[node * CHANS + 1]
                    && b >= lv.min_vals[node * CHANS + 2]
                    && r <= lv.max_vals[node * CHANS]
                    && g <= lv.max_vals[node * CHANS + 1]
                    && b <= lv.max_vals[node * CHANS + 2];
                current_child[l] += 1;
                current_ind[l] += 1;
                if in_range {
                    if l == depth - 1 {
                        let base = self.base_inds[node];
                        let mut x_out = [0.0f32; 3];
                        if invert_hypercube(
                            &grid,
                            &mut x_out,
                            &offs,
                            [r, g, b],
                            base,
                            LIST_LEN,
                            &ops_list,
                            &entering_list,
                            &new_vert_list,
                            &path_list,
                            &path_order,
                        ) {
                            result = x_out;
                            break 'outer;
                        }
                    } else {
                        let nl = l + 1;
                        current_num[nl] = lv.num_children[node];
                        current_ind[nl] = lv.child0offsets[node];
                        current_child[nl] = 0;
                        level = nl as i64;
                        continue 'outer;
                    }
                }
            }
            level -= 1;
        }
        // Indices include the extrapolation: subtract 1.
        std::array::from_fn(|c| {
            let t = (result[c] - 1.0).clamp(0.0, max_dim) * scale;
            self.domain_min[c] + t * (self.domain_max[c] - self.domain_min[c])
        })
    }

    /// The inverse sampled on an `n`-per-side grid over `[0, 1]^3`, Blue-fastest: OCIO
    /// `MakeFastLut3DFromInverse` (n = 48), the form OCIO's GPU path always uses.
    pub fn bake_grid(&self, n: usize) -> Vec<[f32; 3]> {
        let mut out = Vec::with_capacity(n * n * n);
        let step = 1.0 / (n - 1) as f32;
        for i in 0..n {
            for j in 0..n {
                for k in 0..n {
                    out.push(self.eval([i as f32 * step, j as f32 * step, k as f32 * step]));
                }
            }
        }
        out
    }
}

/// `InvLut3DRenderer::extrapolate3DArray`: copy the LUT into the centre of a `dim + 2` grid and
/// fill the outer shell by pushing the face/edge/corner values away from 0.5 by 4x.
fn extrapolate(data: &[[f32; 3]], dim: usize) -> Vec<f32> {
    let nd = dim + 2;
    let mut out = vec![0.0f32; nd * nd * nd * 3];
    let at = |i: usize, j: usize, k: usize| data[(i * dim + j) * dim + k];
    let mut set = |i: usize, j: usize, k: usize, v: [f32; 3]| {
        let o = ((i * nd + j) * nd + k) * 3;
        out[o..o + 3].copy_from_slice(&v);
    };
    let ex = |v: [f32; 3]| v.map(|x| (x - 0.5) * 4.0 + 0.5);
    let edge = |x: usize| if x == 0 { 0 } else { dim + 1 };
    let ends = if dim > 1 { vec![0, dim - 1] } else { vec![0] };
    for i in 0..dim {
        for j in 0..dim {
            for k in 0..dim {
                set(i + 1, j + 1, k + 1, at(i, j, k));
            }
        }
    }
    // Faces.
    for i in 0..dim {
        for j in 0..dim {
            for &k in &ends {
                set(i + 1, j + 1, edge(k), ex(at(i, j, k)));
            }
        }
    }
    for i in 0..dim {
        for &j in &ends {
            for k in 0..dim {
                set(i + 1, edge(j), k + 1, ex(at(i, j, k)));
            }
        }
    }
    for &i in &ends {
        for j in 0..dim {
            for k in 0..dim {
                set(edge(i), j + 1, k + 1, ex(at(i, j, k)));
            }
        }
    }
    // Edges.
    for &i in &ends {
        for &j in &ends {
            for k in 0..dim {
                set(edge(i), edge(j), k + 1, ex(at(i, j, k)));
            }
        }
    }
    for i in 0..dim {
        for &j in &ends {
            for &k in &ends {
                set(i + 1, edge(j), edge(k), ex(at(i, j, k)));
            }
        }
    }
    for &i in &ends {
        for j in 0..dim {
            for &k in &ends {
                set(edge(i), j + 1, edge(k), ex(at(i, j, k)));
            }
        }
    }
    // Corners.
    for &i in &ends {
        for &j in &ends {
            for &k in &ends {
                set(edge(i), edge(j), edge(k), ex(at(i, j, k)));
            }
        }
    }
    out
}

/// OCIO `invert_hypercube` (after Gill, Murray and Wright), `n = 3`: does the cell at `guess`
/// hold `val`, and where (in grid index units)? A customised updating LU factorisation walks
/// the cell's tetrahedra.
#[allow(clippy::too_many_arguments)]
fn invert_hypercube(
    gr: &Grid<'_>,
    x_out: &mut [f32; 3],
    ind2off: &[usize; 3],
    val: [f32; 3],
    guess: [usize; 3],
    list_len: usize,
    ops_list: &[i64],
    entering_list: &[usize],
    new_vert_list: &[usize],
    path_list: &[usize],
    path_order: &[usize],
) -> bool {
    const N: usize = 3;
    const ZERO_TOL: f64 = 1.0e-9;
    const NEGZERO_TOL: f64 = -1.0e-9;
    const ONE_TOL: f64 = 1.0 + 1.0e-9;
    let gr = gr.values;
    let (nm1, nm2) = (N - 1, N - 2);
    let mut row_perm = [0usize; N];
    let mut col_perm = [0usize; N];
    let mut sweep_to = [0usize; 20];
    let mut sweep_from = [0usize; 20];
    let mut sweep_f = [0.0f64; 20];
    let mut base_vert = [0.0f64; N];
    let mut y = [0.0f64; N];
    let mut u = [[0.0f64; N]; N];
    let mut x = [0.0f64; N];
    let mut b = [0.0f64; N];
    let mut x2 = [0.0f64; N];
    let mut new_vert = [0.0f64; N];
    let mut infeas = false;
    let mut numsweeps = 0usize;

    let base_ind: usize = (0..N).map(|i| guess[i] * ind2off[i]).sum();
    for i in 0..N {
        row_perm[i] = i;
        col_perm[i] = i;
        base_vert[i] = gr[base_ind + i] as f64;
        b[i] = val[i] as f64 - base_vert[i];
        y[i] = b[i];
        for (j, u_ij) in u[i].iter_mut().enumerate() {
            *u_ij = if i == j { 1.0 } else { 0.0 };
        }
    }

    for i in 0..list_len {
        let mut backsub = ops_list[i];
        if backsub < 0 {
            numsweeps = 0;
            backsub = 0;
            for j in 0..N {
                y[j] = b[j];
                row_perm[j] = j;
                col_perm[j] = j;
                for (k, u_jk) in u[j].iter_mut().enumerate() {
                    *u_jk = if j == k { 1.0 } else { 0.0 };
                }
            }
        }
        let entering = entering_list[i];
        for j in 0..N {
            let tmp = base_ind + N * new_vert_list[i];
            new_vert[j] = gr[tmp + j] as f64 - base_vert[j];
        }
        for j in 0..numsweeps {
            new_vert[sweep_to[j]] -= sweep_f[j] * new_vert[sweep_from[j]];
        }
        let mut leaving_nz = 0usize;
        for j in 0..N {
            u[j][entering] = new_vert[j];
            if col_perm[j] == entering {
                leaving_nz = j + 1;
            }
        }
        if leaving_nz <= nm2 {
            let tmp = col_perm[leaving_nz - 1];
            for j in leaving_nz - 1..nm2 {
                col_perm[j] = col_perm[j + 1];
            }
            col_perm[nm2] = tmp;
        }
        for j in leaving_nz - 1..nm1 {
            let jp1 = j + 1;
            let mut piv = j;
            // Declared once per `j` and not reset per `h`, as in C++ (it matters: a column swap
            // repeats for later `h` once `abs_d` is above the tolerance).
            let mut col_piv = j;
            let mut abs_d = u[row_perm[j]][col_perm[j]].abs();
            for k in jp1..N {
                let abs_n = u[row_perm[k]][col_perm[j]].abs();
                if abs_n > abs_d {
                    abs_d = abs_n;
                    piv = k;
                }
            }
            if abs_d < ZERO_TOL {
                // Rank-revealing factorisation.
                for h in jp1..N {
                    for k in j..N {
                        let abs_n = u[row_perm[k]][col_perm[h]].abs();
                        if abs_n > abs_d {
                            abs_d = abs_n;
                            piv = k;
                            col_piv = h;
                        }
                    }
                    if abs_d > ZERO_TOL {
                        col_perm.swap(j, col_piv);
                    }
                }
            }
            if piv != j {
                row_perm.swap(j, piv);
            }
            let denom = u[row_perm[j]][col_perm[j]];
            for h in jp1..N {
                let num = u[row_perm[h]][col_perm[j]];
                if num.abs() >= ZERO_TOL {
                    let f = num / denom;
                    u[row_perm[h]][col_perm[j]] = 0.0;
                    for k in jp1..N {
                        u[row_perm[h]][col_perm[k]] -= f * u[row_perm[j]][col_perm[k]];
                    }
                    y[row_perm[h]] -= f * y[row_perm[j]];
                    sweep_to[numsweeps] = row_perm[h];
                    sweep_from[numsweeps] = row_perm[j];
                    sweep_f[numsweeps] = f;
                    numsweeps += 1;
                }
            }
        }
        if backsub != 0 {
            let mut running = 0.0f64;
            for js in (0..N).rev() {
                let denom = u[row_perm[js]][col_perm[js]];
                if denom.abs() < ZERO_TOL {
                    if y[row_perm[js]].abs() > ZERO_TOL {
                        infeas = true;
                        break;
                    }
                    x[js] = 0.0;
                    infeas = false;
                } else {
                    let sm: f64 = (js + 1..N)
                        .map(|k| u[row_perm[js]][col_perm[k]] * x[k])
                        .sum();
                    let xt = (y[row_perm[js]] - sm) / denom;
                    infeas = xt < NEGZERO_TOL;
                    if infeas {
                        break;
                    }
                    running += xt;
                    infeas = running > ONE_TOL;
                    if infeas {
                        break;
                    }
                    x[js] = xt;
                }
            }
            if !infeas {
                for j in 0..N {
                    x2[col_perm[j]] = x[j];
                }
                let mut t = i * N + N - 1;
                x_out[path_list[t]] = x2[path_order[0]] as f32;
                for j in 1..N {
                    t -= 1;
                    // `float(x2 + x_out)`: the sum is taken in double, as in C++.
                    x_out[path_list[t]] =
                        (x2[path_order[j]] + x_out[path_list[t + 1]] as f64) as f32;
                }
                break;
            }
        }
    }
    if infeas {
        return false;
    }
    for j in 0..N {
        x_out[j] += guess[j] as f32;
    }
    true
}
