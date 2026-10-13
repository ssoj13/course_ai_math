# Diagrams - jph-rs (refreshed 2026-09-30, see AGENTS.md / plan1.md)

The previous version (parity pie over 47 files, unsourced severity pie and parity percentages) was removed: it did not cover `mem.rs`, `parse.rs`,
the Part-1 decoder or the multilayer path, and its numbers were not sourced.

## Crate layers

```mermaid
flowchart TD
    subgraph L2[Layer 2 - safe API]
        MEM[mem.rs try_decode_with / try_encode_with]
        PARSE[parse.rs enumerate]
    end
    subgraph CLI[CLI feature cli]
        OJPH[bin/ojph.rs - thin CLI over mem]
        BENCH[bin/ojph_bench.rs]
    end
    subgraph L1[Layer 1 - 1:1 port, unsafe]
        CS[openjph Codestream + params + file + mem + arch]
        CSL[codestream local: tile, tile_comp, resolution, precinct, subband, codeblock]
        ML[ojph_packet_ml multilayer parser]
        COD[coding: HT enc/dec tiers, Part-1 MQ decoder, ojph_cmodes style flags + seg_end]
        TR[transform: DWT + colour, SIMD tiers]
    end
    MEM --> CS
    PARSE --> CS
    OJPH --> MEM
    BENCH --> CS
    CS --> CSL
    CSL --> ML
    CSL --> COD
    CSL --> TR
    EXR[exr-core] --> CS
    HEIF[heif-rs] --> MEM
    WGPU[jph-wgpu-rs] --> PARSE
```

## Decode path (untrusted-input gate before any allocation)

```mermaid
flowchart LR
    A[bytes] --> B[MemInfile::open]
    B --> C[read_headers: markers, COD validity]
    C --> R[restrict_input_resolution: reduce vs min per-component decomps]
    R --> E[check_siz: MAX_IMAGE_PIXELS budget]
    E --> D[create: arena alloc scaled by geometry]
    D --> F[alloc plane Vecs]
    F --> G{pull_ref per line}
    G --> H[parse_tile_header: packets]
    H --> I[Codeblock::decode HT or Part-1]
    I --> J[apply_roi RGN per block]
    J --> K[dequant tx_from_cb]
    K --> L[inverse DWT]
    L --> M[inverse colour]
    M --> N[Plane.samples]
    style E fill:#9f9
```

`parse::enumerate` runs the same `check_siz` gate. Green = the gate (plan1 H1, fixed).

## Packet iteration and header parsing (shared pieces)

```mermaid
flowchart TD
    T[Tile::parse_tile_header / flush] --> PO{progression}
    PO -- POC marker --> POC[poc_parse: per-precinct layer cursors]
    PO -- LRCP RLCP --> LR[resolution/layer loops, layer-outermost keeps HashMap of MlPrecinct]
    PO -- RPCL PCRL PRCL CPRL --> WP[walk_pos + pick_pos: ONE iterator for writer and reader]
    WP --> TC[TileComp::get_res / with_res]
    LR --> TC
    POC --> TC
    TC --> Q{num_layers > 1}
    Q -- no --> S1[Precinct::parse_header]
    Q -- yes --> ML[Precinct::parse_ml_layer -> parse_subband_ml]
    S1 --> SEG[cmodes::seg_end segment lengths - Part-1]
    ML --> SEG
    ML --> TT[tt_walk: inclusion + ZBP tag trees]
    BUF[ML multi-tile-part payloads buffered] -->|TNsot parts or EOC| SW[single sweep via MemInfile] --> PP[Tile::parse_packets]
    PP --> T
```

Dropped (reduced) resolutions: the headers are parsed, bodies stepped over (`skipped_res_for_read`).

## SIMD dispatch

```mermaid
flowchart TD
    ENV[OJPH_CPU_MAX_LEVEL cap / OJPH_CPU_FORCE_LEVEL clamped to detected, feature unsafe-force-simd lifts] --> LVL[get_cpu_ext_level]
    LVL --> ENC[encode: generic / avx2 0.31 kernels / avx512 old kernels]
    LVL --> DEC[decode: 32 / 64 / ssse3 / avx2 / wasm]
    LVL --> TRF[transform + colour: gen sse sse2 avx avx2 avx512 wasm]
    LVL --> QZ[quantize tx_to_cb: Rounding::Cpp = trunc generic vs cvtps sse2/avx2; Rounding::Trunc pins the scalar kernel]
```

## Codestream structure

```mermaid
flowchart TD
    SOC --> SIZ --> CAP --> COD[COD / COC] --> QCD[QCD / QCC]
    QCD --> OPT[optional: RGN, POC, PPM, TLM, PLM, COM, NLT, ATK, DFS]
    OPT --> SOT[SOT tile-part] --> TPH[tile-part header: in-tile QCD/QCC/RGN/POC/PPT; COD/COC/NLT refused in strict mode, warned+skipped when resilient] --> SOD --> PKT[packets: SOP? header EPH? body]
    PKT --> SOT
    PKT --> EOC
```

## Bug hunt status (source: plan1.md, AGENTS.md ledger)

```mermaid
flowchart LR
    P0[P0 gate and safety - done] --> P1[P1 multilayer - done] --> P2[P2 Part-1 - done] --> P3[P3 API soundness - partly, no-break policy] --> P4[P4 CLI - done] --> P5[P5 SIMD - done] --> P6[P6 tests docs license - done]
```

Conformance: 73/73 ISO vectors (`tests/conf_ref.rs`). Open items are listed in AGENTS.md.
