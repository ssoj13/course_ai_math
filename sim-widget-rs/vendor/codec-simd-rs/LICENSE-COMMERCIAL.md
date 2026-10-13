# Commercial License — `codec-simd`

`codec-simd` is offered under a **dual-license** model:

1. **GPL-3.0-or-later** (free, open-source) — see [`LICENSE`](LICENSE).
2. **Commercial license** (paid, proprietary-friendly) — described below.

## When you need the commercial license

You need a commercial license if **any** of these are true for your project:

- You ship a product (binary, SaaS, hardware) that links against `codec-simd`
  **and** you cannot or do not want to release your product's source code
  under GPL-3.0-or-later.
- You distribute a modified version of `codec-simd` and do not want to publish
  the modifications under GPL-3.0-or-later.
- You embed `codec-simd` in proprietary firmware, console games, DRM-bearing
  media pipelines, or closed-source video editing / streaming tooling.
- Your legal team flagged "GPL contamination" as a deal-breaker.

You **do not** need the commercial license if:

- Your project is itself GPL-compatible and you are happy to ship full source.
- You only use `codec-simd` for personal R&D, academic research, or
  contributions back upstream.

## What the commercial license grants

- Royalty-free, perpetual, worldwide rights to use, modify, and distribute
  `codec-simd` and derivative works in proprietary / closed-source products.
- Removal of the GPL copyleft requirements on your codebase.
- Optional priority support, custom backend (e.g. SVE2, AVX-512, RISC-V V)
  contracting, and bit-exactness audit assistance (separately scoped).

## How to obtain a commercial license

Contact: **joss13@gmail.com**.  Include:

- Company name and product / use case (one paragraph is plenty).
- Estimated rough deployment scale (helps with pricing tier).
- Whether you need only the runtime library or also custom-backend / support.

Pricing is per-product per-major-version, with discounts for startups and
academic spin-offs.  Indicative ranges (subject to change):

- Indie studio / small-scale commercial product: low four figures USD.
- Mid-market SaaS / device manufacturer: low-to-mid five figures USD.
- Enterprise integration with priority support: negotiated annually.

A signed license agreement is delivered within ~10 business days of pricing
acceptance.

## What this is NOT

- Not a CLA — contributors retain copyright on their patches (per the
  Developer Certificate of Origin asserted in each commit).
- Not a usage-restriction or ethical-source license — purely a way to dual-
  license between GPL copyleft and a commercial waiver of that copyleft.
- Not negotiable down to MIT/BSD-permissive — that defeats the dual-license
  funding model.  If you need MIT-style permissiveness for free, build it
  yourself; there are several non-`codec-simd` SIMD crates with that posture.

---

If you ship code that includes `codec-simd` and you are not sure which
license applies to you, the safe default is "commercial" — reach out, we
will figure it out together.
