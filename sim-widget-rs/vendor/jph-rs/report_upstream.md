# Upstream report (DRAFT — not sent): OpenJPH encoder auto-selects `planar = 1` when the colour transform is on, which corrupts every component but the last

Target: https://github.com/aous72/OpenJPH/issues
Status: **draft, not filed.** Written from jph-rs's investigation (see `BUG1.md`, Rounds 4–5).

## Summary

When a caller leaves the line-exchange scheduling at its default (never calls
`codestream::set_planar()`), `codestream::write_headers()` auto-selects
`planar = 1` whenever the colour transform (RCT/ICT) is employed. That is the
opposite of what the encoder supports: an explicit `set_planar(true)` in the
same configuration is rejected with error `0x00030021` ("the planar interface
option cannot be used when colour transform is employed"). The auto-selected
path is not rejected, and it silently produces a corrupt codestream: every
component except the last one fed decodes as its final row repeated for every
row.

## Affected versions

- Tag `0.31.0` (what OpenEXR 3.4.15 links, e.g. via vcpkg).
- Still present on `master` at `8c2826f` (after 0.32.0).

## Location

`src/core/codestream/ojph_codestream_local.cpp`, `codestream::write_headers()`
(lines 623–636 at 0.31.0 and at 8c2826f):

```cpp
if (planar == -1) //not initialized
  planar = cod.is_employing_color_transform() ? 1 : 0;   // <-- inverted?
else if (planar == 0) //interleaved is chosen
{
}
else if (planar == 1) //plannar is chosen
{
  if (cod.is_employing_color_transform() == true)
    OJPH_ERROR(0x00030021,
      "the planar interface option cannot be used when colour "
      "transform is employed");
}
```

The explicit branch forbids `planar == 1` with the colour transform, yet the
auto-select branch chooses exactly that value for that case. The decoder's own
auto-select uses the opposite convention (`colour_transform ? 0 : 1`), which is
consistent with the explicit check. The encoder ternary appears to be inverted.

## Why it has not been noticed

`ojph_compress` always calls `set_planar(false)` for image inputs (and uses
`set_planar(true)` only for raw/YUV inputs that never enable the colour
transform), so the auto-select branch is never reached with the colour
transform on through the CLI. Library users who rely on the default hit it.

## Reproduction

Minimal program against a 0.31.0 build (MSVC, `/O2 /EHsc /std:c++14`, includes
`src/core/openjph` and `src/core/shared`, linked with `openjph.0.31.lib`):
32x32, 3 components, 8-bit unsigned, reversible, `set_color_transform(true)`,
**no** `set_planar()` call; lines are fed through `exchange()` in the order the
codestream requests (`is_planar()` reports 1; every fed component index matches
`next_comp`); then the file is decoded and compared.

```cpp
#include <cstdio>
#include <cstring>
#include <vector>
#include "ojph_arch.h"
#include "ojph_file.h"
#include "ojph_params.h"
#include "ojph_codestream.h"
#include "ojph_mem.h"
using namespace ojph;

static const ui32 W = 32, H = 32, NC = 3;

int main() {
  std::vector<std::vector<si32>> src(NC, std::vector<si32>(W * H));
  for (ui32 c = 0; c < NC; ++c)
    for (ui32 i = 0; i < W * H; ++i)
      src[c][i] = (si32)(i + c * 100);

  { // encode — planar left at its default (-1)
    codestream cs;
    param_siz siz = cs.access_siz();
    siz.set_image_extent(point(W, H));
    siz.set_tile_size(size(W, H));
    siz.set_num_components(NC);
    for (ui32 c = 0; c < NC; ++c) siz.set_component(c, point(1, 1), 8, false);
    param_cod cod = cs.access_cod();
    cod.set_num_decomposition(5);
    cod.set_block_dims(64, 64);
    cod.set_progression_order("RPCL");
    cod.set_reversible(true);
    cod.set_color_transform(true);

    j2c_outfile file;
    file.open("ct_planar_repro.j2c");
    cs.write_headers(&file);            // is_planar() == 1 after this

    ui32 next_comp;
    line_buf* line = cs.exchange(NULL, next_comp);
    for (ui32 c = 0; c < NC; ++c)       // component-major, as planar requires
      for (ui32 y = 0; y < H; ++y) {
        if (c != next_comp) { printf("protocol mismatch\n"); return 1; }
        memcpy(line->i32, src[c].data() + y * W, W * sizeof(si32));
        line = cs.exchange(line, next_comp);
      }
    cs.flush();
    cs.close();
  }

  std::vector<std::vector<si32>> dec(NC, std::vector<si32>(W * H));
  { // decode
    codestream cs;
    j2c_infile file;
    file.open("ct_planar_repro.j2c");
    cs.read_headers(&file);
    cs.create();
    ui32 next_row[NC] = {0, 0, 0};
    for (ui32 i = 0; i < NC * H; ++i) {
      ui32 c;
      line_buf* line = cs.pull(c);
      memcpy(dec[c].data() + (next_row[c]++) * W, line->i32, W * sizeof(si32));
    }
    cs.close();
  }

  ui32 total = 0;
  for (ui32 c = 0; c < NC; ++c) {
    ui32 diffs = 0;
    for (ui32 i = 0; i < W * H; ++i) diffs += src[c][i] != dec[c][i];
    printf("component %u: %u / %u samples differ\n", c, diffs, W * H);
    total += diffs;
  }
  return total ? 2 : 0;
}
```

Observed output:

```
component 0: 992 / 1024 samples differ
component 1: 992 / 1024 samples differ
component 2: 0 / 1024 samples differ
```

Each wrong sample equals the last row of its component (row 31) — i.e. every
row of components 0 and 1 is replaced by that component's final row.
Expected: 0 differences in all components (as with `set_planar(false)`).

Note: the sample values (`i + c * 100`) exceed the declared 8-bit range; this is
the exact program that produced the output above. Before filing, re-run it with
in-range samples (e.g. `(i + c * 100) & 0xFF`) to rule out range effects — the
row-replication pattern points at scheduling, not sample values.

## Suggested fix

Invert the auto-select so it matches the explicit-request rule and the decoder:

```cpp
if (planar == -1) //not initialized
  planar = cod.is_employing_color_transform() ? 0 : 1;
```

Alternatively (if component-major feeding with the colour transform is meant to
be supported), the planar + colour-transform encode path needs fixing and the
explicit `0x00030021` check removed — but as it stands the two branches
contradict each other.

## How jph-rs handles it

jph-rs (a Rust port of OpenJPH) reproduces the same behaviour 1:1. It now
rejects the auto-selected `planar = 1` + colour transform combination with a
dedicated error (`0x00030024`) instead of emitting a corrupt codestream, and
its high-level encoder always requests interleaved scheduling when the colour
transform is on. Details: `BUG1.md`, Rounds 4–6.
