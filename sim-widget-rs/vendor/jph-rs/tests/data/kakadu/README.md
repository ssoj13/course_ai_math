Kakadu 8.4 fixtures (lossless HT, 4 layers, 2 levels, 32x32 precincts, SOP+EPH, POC
`{0,0,2,3,3,LRCP},{0,0,4,3,3,CPRL}`) of `../corpus/rgb_prime_37x41.ppm`:

    kdu_compress -i rgb_prime_37x41.ppm -o poc.j2c Creversible=yes Clayers=4 Cmodes=HT Clevels=2 \
        Cprecincts="{32,32}" Cuse_sop=yes Cuse_eph=yes "Porder={0,0,2,3,3,LRCP},{0,0,4,3,3,CPRL}"
    kdu_makeppm poc.j2c poc_ppm.j2c          # headers packed into PPM
    kdu_makeppm poc.j2c poc_ppt.j2c -ppt     # headers packed into PPT

All three decode (kdu_expand and jph-rs) to the source image bit-exactly.

`ml_LRCP.j2c` / `ml_RLCP.j2c`: same image, 3 layers, 3 levels, 32x32 precincts, `Corder=LRCP|RLCP`
(`kdu_compress ... Clayers=3 Cmodes=HT Clevels=3 Cprecincts="{32,32}" Corder=...`).
`ml_*_r1.ppm` are `kdu_expand -reduce 1` outputs (regression for multi-layer reduced-resolution decode).
`poc_r1.ppm`: `kdu_expand -i poc_ppt.j2c -reduce 1` (POC + reduced resolution regression).
`poc1tp.j2c`: 1 layer, `ORGtparts=R` (3 tile-parts), `Porder={0,0,1,2,3,RPCL},{0,0,1,3,3,CPRL}` — POC across tile-parts; decodes to the source image.
`mlt*.j2c`: 3 layers, RPCL, `ORGtparts=R` (3 tile-parts), SOP+EPH; `mlt_ppm` / `mlt_ppt` via `kdu_makeppm [-ppt]` — multi-layer multi-tile-part with packed headers.
