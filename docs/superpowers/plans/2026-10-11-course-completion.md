# Whole-course completion and acceptance map

Status: planned implementation, 2026-10-11. Scope: **all 98 curriculum entries**, all requested simulations, English and Russian, Python and Colab, term maps, machine depth, and a verified GitHub Pages release. A published text page, demo primitive, storyboard, or passing build alone does not complete a lesson.

This map extends the [engine plan](2026-10-10-engine-and-spikes.md) and [engine addendum](2026-10-10-engine-addendum-6d-6e.md). It does not replace their detailed contracts or the scene specifications. Execute in batches; continue until the entire acceptance register is closed.

## Contents

- [Authority and current gaps](#authority-and-current-gaps)
- [Completion contract for every lesson](#completion-contract-for-every-lesson)
- [Prerequisite phases and implementation batches](#prerequisite-phases-and-implementation-batches)
- [Lesson acceptance register](#lesson-acceptance-register)
- [Named scene acceptance register](#named-scene-acceptance-register)
- [Cross-course requirements and traceability](#cross-course-requirements-and-traceability)
- [Verification and release](#verification-and-release)

## Authority and current gaps

Read [task.md](../../../task.md), [CURRICULUM.md](../../../CURRICULUM.md), [AGENTS.md](../../../AGENTS.md), and [HANDOFF.md](../../../HANDOFF.md) before execution. The local SDD ledger is `.superpowers/sdd/2026-10-10-engine-and-spikes/progress.md`; it and actual files/commits establish implementation state. The operator has explicitly authorized full-course implementation now. Historical pause, pending Plan 1 approval, and proposed repository/visibility notes are not current execution blockers.

The 2026-10-11 planning inspection found:

| Area                 | Evidence present                                                                                                     | Work still required                                                                                                                      |
| -------------------- | -------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| Course shell         | Astro/Starlight, course manifest, progress, Quiz, Pages workflow; English Part 0/1 files                             | Expand manifest to all 98 entries; locale parity, roadmap, glossary, full navigation and no placeholder destinations                     |
| Reference prose      | 12 English lesson files; law-of-cosines MDX contains LC scene comments, static Python, and quizzes                   | Mount real scenes, add executable Python and term maps, translate, verify each lesson                                                    |
| Powers               | Separate powers worktree and partial review recorded in handoff                                                      | Recover/review/integrate 0.2 and storyboard §7; resolve all review items before treating it as delivered                                 |
| Visualization engine | Shared Canvas/View, store, Formula/Num, primitives, ViewportCard, N-panel, gizmo, Ledger, Challenge, LitSphere, demo | 6h holo/clay; 6g localization; 6f size modes; 7 Timeline/NumTable/CodeView and Formula extensions; lesson composition                    |
| Numeric core         | Geometry helpers; **no `src/core` implementation in inspected source inventory**                                     | Tensor shape/strides/ops and recorded reverse autodiff; solver and format helpers as dependent batches require                           |
| Learning services    | ColabBadge component exists; package has a size script command                                                       | No Pyodide workers, notebook source, term graph, Russian lesson tree, or scripts/size-report.ts in the verified baseline; implement them |
| Later content        | Curriculum and scene backlogs                                                                                        | Parts 2–10, A1–A10, Q1–Q12 are implementation work, not completed by these documents                                                     |

Historical baseline from HANDOFF: commit `8868525`, 178 tests, check 0/0/0, 18 built pages, viz core about 272.7 kB gzip. These are previous results, not fresh validation of this plan or later changes. Re-inventory at the start of each batch because concurrent work can change the checkout.

Resolve inherited contradictions without reducing scope:

- Bun **1.4.3**, on-demand SceneHost, lazy SSR-safe entries, 300 kB gzip viz-core budget, and plain-wheel page scrolling supersede older plan text.
- The standalone powers lesson is **0.2**; keep **0.G** for ground. Part 0/1 now has 13 entries. Derive progress totals from the manifest.
- The **Start here** module (`start`: 0.0a `why-this-course`, 0.0b `how-it-connects`, added 2026-10-11) precedes Part 0 in the manifest and the sidebar; its scenes INTRO-1..3 and the procedural DotUnit system are specified in [intro-and-dotunit.md](../../design/intro-and-dotunit.md), which also owns the DotUnit presets reused by 1.4, 2.2, 2.4, 3.6/3.7 and attention.
- General inverse/change of basis links point to **2.9**, not the old 2.7. Eigenvectors follow matrices at **2.11**.
- Old “Plan 1 of 2” and “not yet Tensor/autodiff” describe sequencing only. This map includes the complete numeric core and all later lessons.
- Exact holo reference and “remove nothing” override automatic low-power/mobile removal or a fallback that drops dials. Reflow/reposition instruments, offer explicit reader controls, and measure. Do not silently omit galaxies, stars, floor, or instruments.
- Fable consultation is recorded historically. If the model is unavailable, record that fact and use an available reviewer without claiming Fable approval.

## Completion contract for every lesson

Each register row below inherits **C1–C9**. Its final column adds topic-specific checks; it is not the whole acceptance test.

| ID  | Required result                                                                                                                                                                                     | Evidence                                                                                                               |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| C1  | English page and faithful Russian page; same symbols, examples, numbers, challenges, scene features, and prerequisite/next-question links                                                           | Route pair and locale coverage report; reviewed prose in both languages                                                |
| C2  | Hook → interactive scenes with a clear action → simpler explanation → hand exercises with revealed answers → misconception quiz → runnable Python/Colab → machine layer → next question → local map | Rendered page audit; every new symbol explained in words and followed by a numerical example                           |
| C3  | Real scene mounted in the lesson; scene, formula, table, and code use shared values and semantic IDs; every promised timeline supports back/step/scrub                                              | Real input recording plus state/number assertions; no remaining scene placeholder for required content                 |
| C4  | Intuition → formula → NumPy/TS code → machine depth, with CG/VFX analogy and an AI destination                                                                                                      | Cross-links and working code; hardware-specific claims dated and verified against primary/vendor sources when authored |
| C5  | One notebook source drives browser cells and generated Colab notebook; deterministic small examples and explicit seeds/tolerances                                                                   | Source ID/hash, generated notebook, clean execution, outputs matching lesson values                                    |
| C6  | Local term map, dependency genealogy, depth stack, glossary entry, and flashcard coverage from one typed graph                                                                                      | Valid IDs/routes, acyclic built-on edges, accessible map/list, correct prerequisite chain                              |
| C7  | Keyboard, pointer, touch, reduced motion, theme, no-WebGL alternatives, enlargement and full screen work                                                                                            | Browser matrix below; meaningful DOM numbers/captions and controls without canvas                                      |
| C8  | Mathematical and numerical edge cases correct; undefined quantities explained; stochastic demos distinguish observation from theorem                                                                | Independent calculation/oracle tests, boundary fixtures, reviewed formulas and answer keys                             |
| C9  | Page fits budgets, exposes no unintended runtime errors, and works on the deployed project base path                                                                                                | Build/size report, network/console report, published route and interaction smoke test                                  |

A lesson is complete only when C1–C9 and its row-specific checks pass. **The operation itself must be visible and interactive:** show what projects onto which ruler, how row/column contributions form an output, which index contracts, or where a gradient travels. Use meaningful 3D where spatial relationships need it. Guided animations must support pause, step, back, and time scrubbing, with values/formulas/code linked to the scene. This is a per-lesson acceptance gate, not optional polish; static screenshots alone cannot prove it. Do not credit a shared scene implementation to a lesson until that lesson mounts and explains it. Every batch report must list lesson IDs, scene IDs, changed files, test commands/results, screenshots, reviewed commit, and remaining findings.

Use `src/content/docs/<module>/<slug>.mdx` and `src/content/docs/ru/<module>/<slug>.mdx`; keep existing slugs stable. New lesson composition uses an SSR-safe `src/viz/lessons/<slug>.tsx` entry, lazy `<slug>.impl.tsx`, and a three-free shared rows/derived-values module when needed. Register new slugs centrally together with their real lesson content and scenes before using `lesson:` links. Keep the complete syllabus inventory in planning/validation data; do not replace the live course manifest with 98 empty syllabus stubs.

## Prerequisite phases and implementation batches

Each batch follows: inspect current files and graph freshness → write concrete fixtures/scene states → implement minimal shared capability → compose lessons → verify C1–C9 → review/fix → record evidence → integrate. Use GitNexus impact before symbol edits and reanalyze after edits under AGENTS rules. Never reset or overwrite concurrent work. Commit/push only under the controller's current authorization.

| Phase                        | Ordered work and concrete output                                                                                              | Exit gate                                                                                                                                                                |
| ---------------------------- | ----------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| P0: recovery                 | Recover powers review and branch; reconcile numbering/links and outstanding ledger items; inventory all scene slots and files | 98-entry ID manifest; preserved history; powers x/A/V write contract, exponent key/timeline contract, thin-axis counting, gamma domain and range issues resolved in spec |
| P1: visual foundation        | 6h holo/clay → 6g bilingual site/UI → 6f 2×/full screen → Task 7 controls                                                     | Shared canvas still singular; complete reference comparison; locale tests; all controls keep state across size modes                                                     |
| P2: first interactive slice  | LC-0..3, DP-1..4; then 0.G and 0.2 including TileGrid/FnCurve                                                                 | Interactive slice verified in both locales; scene numerical/interaction checks pass; C5/C6 and dependent service checks remain pending until P4                                                                         |
| P3: course services          | Task 8 Pyodide → 9 terms/maps → 10 notebooks → 11 size/CI; add quiz-lint and lesson completeness checker                      | Working failure/retry paths, source parity, schema/route/quiz gates, dependency-aware size report                                                                        |
| P4: reference completion     | 0.3–0.4; 1.1–1.3; 1.5–1.8 in batches of two/three; retrofit C1–C9 to P2                                                       | All 13 Part 0/1 entries complete; no reference scene comments left as deliverables                                                                                       |
| P5: matrix/core foundation   | Tensor buffer/shape/strides foundation; stable tiny matrix/solve helpers; 2.1–2.3, 2.4–2.5, 2.6–2.8, 2.9–2.11                 | All four multiplication views and full basis scene; solver/conditioning/rank/eigen edge cases                                                                            |
| P6: tensors                  | 3.1–3.3 tensor road/memory; 3.4–3.5 broadcasts/reductions; 3.6–3.7 contraction/layer                                          | Actual core values power S3/S4 and all index/shape views                                                                                                                 |
| P7: scalar calculus          | 4.1–4.3 D1/S6; 4.4–4.5; 4.6–4.7                                                                                               | Continuous probe, honest kink/domain behavior, finite-difference accuracy explorer                                                                                       |
| P8: multivariable/autodiff   | 5.1–5.3 D2/D3; D4–D6; 5.4 Jacobian; recorded autodiff; 5.5–5.6 B2/B3                                                          | Finite-difference gradient checks; graph and tape are the same computation                                                                                               |
| P9: accumulation/probability | 6.1–6.2; 6.3–6.4; 7.1–7.2; 7.3–7.5; 7.6–7.7                                                                                   | Density normalization, integration convergence, seeded probability experiments                                                                                           |
| P10: statistics              | 8.1–8.3 B1; 8.4–8.5; 8.6–8.8                                                                                                  | Entire why-squares chain, assumptions and robust-loss counterexamples                                                                                                    |
| P11: learning machines       | 9.1–9.3; 9.4–9.5; 9.6–9.7                                                                                                     | One training state links data, loss, graph, parameters, and optimizer steps                                                                                              |
| P12: attention               | 10.1–10.2 S5; 10.3–10.4; 10.5–10.6                                                                                            | Complete block with shape-safe math and clickable provenance to fundamentals                                                                                             |
| P13: advanced                | A1–A2; A3–A5; A7–A8; A9–A10; A6 branch entrance                                                                               | Each topic has its AI mechanism and executable numerical comparison                                                                                                      |
| P14: quantization            | Q1–Q3; Q4–Q6; Q7–Q8; Q9–Q10; Q11–Q12                                                                                          | All requested formats/algorithms, actual toy-network damage, sourced hardware support                                                                                    |
| P15: release                 | Whole-course audit and publishing sequence below                                                                              | 98/98 complete, both locales, all named scenes, no unresolved required deliverable                                                                                       |

P2 is an interim scene-delivery milestone, not lesson completion. Its report must say **“interactive slice verified; C5/C6 and dependent checks pending”** and list the unfinished Pyodide execution/failure paths, notebook/Colab source parity, term-map/genealogy/glossary/flashcard checks, and service-dependent portions of C2/C7/C9. P2 still requires its scene-specific numerical, interaction, accessibility, and locale checks. P3 supplies the services; P4 retrofits and verifies all C1–C9 for the reference lessons before any receives a lesson-complete flag.

These are dependency batches, not a reduced release scope. Independently implementable work may overlap only with explicit file ownership. Do not expand a batch into a repository-wide rewrite. A review iteration cap triggers a fresh diagnosis or reviewer, never acceptance of a required defect or removal of a scene. Within a large scene family, review one numerical model and one interaction at a time before multiplying its views.

### Shared capabilities that must have explicit implementation tasks

- **Task 7 extensions:** NumTable, Timeline, CodeView; maximum visited timeline step; Formula marks/cancellation, narrow layout, tooltips, keyboard focus, stable widths and live ARIA values. Poly and pure piece placement for LC dissection; glyph readouts; point sizes and typed-value constraints for the lamp.
- **Powers:** TileGrid exact whole/partial measure with explicit logical dimensions independent of rendering thickness; FnCurve; integer/preset controls; live 2D/3D mode; explicit camera pose; one normalized write path for x/A/V from handles, fields, Formula, Num, keyboard and timeline. Test x = 0, 0.1, 2.5 and clamped input.
- **Tensor core:** flat typed buffer, shape/strides/offset, bounds and shape validation, views/reshape/transpose/contiguous, broadcast, reductions, matmul/einsum. Establish dtype and accumulation semantics; test NumPy-equivalent small fixtures, aliasing, non-contiguous access and invalid shapes.
- **Autodiff core:** stable operation IDs, saved values, parents/local derivatives, topological forward/backward trace, gradient accumulation and clearing, broadcast-gradient reduction, matmul/activation/loss rules. Test fan-out, repeated operands, constants, non-differentiable conventions, finite differences and update ordering.
- **Plot/field primitives:** sampled curves, parametric surfaces, contour extraction, heat slices, isosurfaces, instanced cells/arrows, sparklines, freeze controls, microscope View. Keep pure numeric/layout helpers independently testable; no animated values invented solely for the picture.
- **Linear algebra/probability/quantization:** small pedagogic solvers, seeded RNG and distribution sampling, number encoders/decoders and quantizer traces. Prefer established implementations where appropriate and validate against an independent reference. Explain algorithm limits.

## Lesson acceptance register

All rows are initially open. “Deliverable” means real interactive content plus C1–C9, including a Python notebook and term/machine depth. The curriculum's prerequisite edges remain binding; the prerequisite-phase table specifies implementation grouping.

### Start here

| ID   | Interactive and pedagogic deliverable                                                                                                                                                                                      | Specific correctness evidence                                                                                                                                                                           |
| ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0.0a | Why this course (EN + RU prose landed 2026-10-11): what a model computes with dated multiply-add counts, CG bridges table, spine sentence, parts with outcomes, uses, why, how to use, honest expectations, quiz; INTRO-1 DotUnit playing its `unit` timeline | Multiply-add figures trace to Kaplan et al. 2020 (2N per token) and the GPT-2 small / Llama 3 8B parameter counts; INTRO-1 step 8 stack top and projection bar top agree within 1 px; RU stamped and parity-checked |
| 0.0b | How it all connects (EN + RU prose landed 2026-10-11): two faces of a·b with a = (3, 1, 2), b = (1, 2, −1); INTRO-2 zoom ladder with dive-in; INTRO-3 einsum playground; NumPy cell with the same numbers               | Every rung shows 3 at the documented cell; dive from a rung-5 cell lands on the rung-1 tile; `einsum` core equals NumPy fixtures to 1e-12; LOD/instancing budget per the spec §1.4–1.5                   |

### Part 0 — Ground

| ID  | Interactive and pedagogic deliverable                                                                                                                      | Specific correctness evidence                                                                                                                                        |
| --- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0.G | Square as counted area, sign-forgetting, area-to-root reversal; number line and draggable coordinate plane; 0.G-1..3 clay/holo scenes                      | Negative input has positive square; principal root is nonnegative; zero and fractional tiles preserve measure                                                        |
| 0.2 | 0.2-1 square; 0.2-2 length→area→volume; 0.2-3 zero/negative exponent ladder; 0.2-4 roots in 2D/3D; 0.2-5 power/inverse curves with gamma/specular examples | x/A/V stay coupled through every writer; x=2.5 gives area 6.25 and volume 15.625; 0.1 never mistaken for decorative thickness; exponent/domain restrictions explicit |
| 0.3 | P/Q distance, three area squares, four-triangle dissection timeline, 3D floor/up decomposition, editable n-D sum                                           | 3–4–5 and 3D fixtures; dissection conserves area; stable norm versus overflow/underflow                                                                              |
| 0.4 | Unit circle shadows, arc unrolling into radians, linked sin/cos plots, special-angle table and snap                                                        | Degrees/radians conversion, negative coordinates/quadrants, full wrap and atan2                                                                                      |
| 0.5 | LC-0..3/S1; missing rectangles, signed ledger, algebra timeline, two-bone IK                                                                               | 0/60/90/120/180 degrees; a=b and c=0; reach boundaries; IK fixture gives about 112.02 degrees                                                                        |

### Part 1 — Vectors

| ID  | Interactive and pedagogic deliverable                                                                                                         | Specific correctness evidence                                                                                                |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| 1.1 | Tip/whole-arrow drag, position/direction toggle, 3D components, B−A linked subtraction                                                        | Whole-arrow translation leaves components unchanged; reversing endpoints reverses vector; buffer/alignment explanation       |
| 1.2 | Head-to-tail/parallelogram sum, signed scaling, span painting, 3D combinations, six-point blend-shape analogy                                 | Coefficients including zero/negative; sum views agree; SIMD example scoped to its instruction/layout                         |
| 1.3 | Component-square summation timeline, normalization to circle/sphere, editable eight-component bars                                            | Zero normalization explicit; norm stays one within tolerance; fp16 sum overflow/FP32 accumulation                            |
| 1.4 | DP-1..4/S2: law-of-cosines derivation, sign/area/product views, 3D, N·L lamp                                                                  | Component and geometric products agree; zero/parallel/antiparallel; unclamped versus clamped lighting and summation rounding |
| 1.5 | Signed shadow, vector projection and perpendicular remainder, Pythagorean tiles, coordinates as shadows, plane projection                     | Remainder orthogonal; normalized/non-unit forms agree; zero-axis undefined                                                   |
| 1.6 | Movable basis lattice and collapse, 3D coplanarity, orthonormal coordinate transfer; basis scene preview                                      | Same world vector under coordinate change; independence versus orthogonality; nonorthogonal mode points to 2.9               |
| 1.7 | Editable toy semantic arrows explicitly labeled toy; real small embedding heatmap, component products, scale invariance and nearest neighbors; embedding table lookup (token id → row), semantic search/RAG retrieval demo, analogy arithmetic with its failure cases | Asset provenance/generation; display PCA labeled projection; real cosine computed in full dimension; zero-vector guard       |
| 1.8 | Orbitable cross-product normal, area parallelogram, operand swap/orientation, triangle normal                                                 | Cross orthogonal to both inputs; magnitude equals area; parallel zero; handedness and sign                                   |
| 1.9 | Sampled signal as an N-vector next to cos/sin frequency rulers; live dot product per frequency ("how much the signal points in this direction"), spectrum bars, orthogonality grid, inverse rebuild by adding scaled rulers; epicycle scene: a drawing (preset outlines and the reader's own freehand path) rebuilt by a chain of spinning sticks, slider for how many sticks, play/pause/scrub time | DFT values equal NumPy `np.fft.rfft` within tolerance; orthogonality of rulers; Parseval; aliasing and leakage cases shown honestly; epicycle stick lengths/phases equal |DFT|/arg of the path samples and the chain endpoint retraces the path; more sticks = smaller error (measured) |

### Part 2 — Matrices

| ID   | Interactive and pedagogic deliverable                                                                      | Specific correctness evidence                                                                                     |
| ---- | ---------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| 2.1  | Drag basis images while grid/points follow; contrast linear and nonlinear/translated maps                  | Origin/additivity/scaling tests; straight grid alone is not the entire definition                                 |
| 2.2  | Columns as images, row-dot and column-combination readings; complete 3D new-rulers row/column transfer     | Both calculations equal Mv; distinguish active transformation from coordinate change                              |
| 2.3  | Scale/rotation/shear/reflection/projection gallery; homogeneous 4×4 position versus direction; bias bridge | w=1 translates, w=0 does not; fixtures for determinant/orientation and projection                                 |
| 2.4  | Four synchronized matmul views: row·column, column combinations, composition, S3 light box; AB/BA switch   | Every C cell equals sum of actual products; noncommuting example; rectangular shape errors                        |
| 2.5  | Transpose flips indices/walls; inner-product adjoint identity; orthonormal transpose/inverse scene         | Inner products agree; transpose is inverse only under stated orthogonality conditions                             |
| 2.6  | Unit square/cube transformed with signed area/volume and collapse                                          | Measured geometric scale equals determinant; reflection sign, zero volume                                         |
| 2.7  | 2×2 area, 3×3 cofactor/Sarrus, row-operation determinant ledger, det(AB)                                   | Row swaps/scaling tracked; Sarrus limited to 3×3; multiplicativity fixtures                                       |
| 2.8  | Lines/planes intersections and column-combination target; elimination timeline and Cramer area ratios      | Unique/no/infinite solution examples; row operations preserve solutions; singular Cramer guarded                  |
| 2.9  | Gauss–Jordan inverse and Ax=b solve; nonorthogonal basis/dual rulers; nearly singular sensitivity          | Residual checks; solve versus explicit inverse; singular refusal and conditioning not judged by determinant alone |
| 2.10 | Column/null spaces and rank collapse; vectors lost under a map                                             | Rank/nullity consistent; Av=0 null vectors; tolerance shown for near dependence                                   |
| 2.11 | Eigenvector search, fixed directions, characteristic determinant, diagonalizing basis                      | Av=λv; repeated/complex eigenvalues and defective matrices handled honestly; no claim every matrix diagonalizes   |

### Part 3 — Tensors

| ID  | Interactive and pedagogic deliverable                                                          | Specific correctness evidence                                                                           |
| --- | ---------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| 3.1 | Tensor road rank 0..6 and general N nesting model; editable dimensions, shape/rank/product HUD | Every index has exactly one cell; rank distinguished from matrix rank; product counts match             |
| 3.2 | Scalar loss, embedding, grayscale/RGB channels, image batch, video and text B×T×D presets      | Real tiny image provenance; labeled axes match actual data; channels peel without swapping semantics    |
| 3.3 | Cell↔index↔flat offset memory strip; strides, transpose, reshape/view/contiguous               | Offset dot product exact; views do not silently copy; invalid reshape rejected; non-contiguous behavior |
| 3.4 | Aligned-axis broadcasting and elementwise operations with replicated visual guides             | Size-one expansion, compatible outputs, incompatible shape explanation; no fake memory allocation       |
| 3.5 | Select sum/mean/max axis, collapse cells and remove/keep dimension                             | Values and shape correct; negative axis/keepdims and max ties documented                                |
| 3.6 | Batched S3 light boxes plus S4 tensor sockets/cables; live einsum and free/contracted indices  | Numeric output agrees with core/NumPy; batch axes stay free; mismatched sockets rejected                |
| 3.7 | Batch layer xWᵀ+b and ReLU; hover output traces all inputs and bias                            | Shape assertions, actual sums, negative/zero activation; link to GEMM plus elementwise kernel           |

### Part 4 — Change

| ID  | Interactive and pedagogic deliverable                                           | Specific correctness evidence                                                                   |
| --- | ------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| 4.1 | Line slope/intercept controls, rise/run, moving secant, CG Graph Editor curve   | Slope equals Δy/Δx; vertical secant/zero denominator explained                                  |
| 4.2 | D1/S6 microscope, P/Q and h scrub, tangent and derivative plot, speed car       | x²/sin/exp agree with derivatives; absolute-value kink never presented as differentiable        |
| 4.3 | Power/sum/product rule decomposition with linked tangent contributions          | Product rule both terms, signed powers within domain, finite-difference checks                  |
| 4.4 | exp growth and ln inverse; tangent comparison and product→sum mapping           | exp derivative identity, ln domain, overflow/underflow machine view                             |
| 4.5 | Nested function pipeline and linked local-slope multipliers                     | Composition chain multiplies in correct order; zero/negative local derivatives                  |
| 4.6 | Min/max and stationary points; draggable c minimizing squared distances to data | Mean minimizes L2; stationary inflection counterexample; convex parabola and constant-data case |
| 4.7 | Tangent/Taylor approximation over selectable radius; h-versus-error plot        | Truncation versus roundoff tradeoff; approximation order measured; no universal best h claim    |

### Part 5 — Many variables

| ID  | Interactive and pedagogic deliverable                                                       | Specific correctness evidence                                                                          |
| --- | ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------ |
| 5.1 | D2 surface/contours and movable probe with synchronized floor/height readouts               | Surface and contours sample same function; axis labels and domain clear                                |
| 5.2 | D2 frozen-coordinate slices; D3 volume; D4/D5/D6 independent time-like inputs and 1D slices | Each partial holds every other input fixed; all six bars match numerical derivatives                   |
| 5.3 | Tangent plane, gradient arrow, rotatable directional slice, six-partial extension           | Direction u normalized; ∇f·u matches slope; zero-gradient contour exception explicit                   |
| 5.4 | Warped grid microscope, local Jacobian table, vector-valued map and composition             | Local residual shrinks with step; matrix dimensions/order; chain rule matches composed derivative      |
| 5.5 | B3 gradient descent on selectable loss surfaces; learning-rate and freeze controls          | Actual −η gradient updates, overshoot/divergence visible; repeatable initial conditions                |
| 5.6 | B2 expression then 2-2-1 MLP; forward/loss/backward trace; local slopes and accumulation    | Graph IDs equal tape IDs; fan-out gradients add; repeated backward/reset safe; finite-difference check |

### Part 6 — Accumulation

| ID  | Interactive and pedagogic deliverable                                                  | Specific correctness evidence                                                                                        |
| --- | -------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| 6.1 | Draggable bounds, signed Riemann rectangles and partition slider                       | Rectangle sum matches numbers; negative area sign; convergence under refinement                                      |
| 6.2 | Accumulation curve and derivative probe linked to original function                    | FTC demonstrated at continuous points; moving lower/upper bounds signs                                               |
| 6.3 | Antiderivative families/constant; trapezoid and Simpson numeric comparison             | Known integral fixtures and error convergence; finite sampling limitations                                           |
| 6.4 | Double-integral columns/volume, slice order, Gaussian integral area-to-polar reasoning | Same volume across integration orders under stated conditions; Gaussian squared-integral derivation and numerical √π |

### Part 7 — Probability

| ID  | Interactive and pedagogic deliverable                                        | Specific correctness evidence                                                             |
| --- | ---------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| 7.1 | Editable event sets/frequency grid, sum/product and conditional restriction  | Bounds and normalization; disjoint versus independent; condition of zero probability      |
| 7.2 | Bayes frequency table/tree with base-rate slider and posterior               | Counts and probabilities agree; rare-event base-rate counterexample                       |
| 7.3 | PMF bars versus density curve and interval-area brush                        | Continuous point probability zero; density may exceed one; total area one                 |
| 7.4 | Weighted-value balance/dot product and continuous expectation slices         | Nonnegative weights sum to one; expectation can be an unobserved value                    |
| 7.5 | Mean, residual squares, variance/std and unit comparison                     | Translation invariance, scaling squared, population/sample distinction                    |
| 7.6 | Bernoulli/categorical/uniform/normal parameter explorer with samples         | Parameter domains, normalized mass/density, analytic versus empirical moments             |
| 7.7 | Seeded repeated samples, running mean and standardized sample-mean histogram | LLN versus CLT distinction; sample-size effect; assumptions and counterexample discussion |

### Part 8 — Statistics and why squares

| ID  | Interactive and pedagogic deliverable                                                        | Specific correctness evidence                                                                                                |
| --- | -------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| 8.1 | Data-vector centering and removal of all-ones component                                      | Centered components sum to zero; projection decomposition                                                                    |
| 8.2 | B1 residual square tiles, sum versus MSE, L1 bars, mean/median minimizers                    | Actual tile areas equal residual²; MSE divides by n; outlier sensitivity                                                     |
| 8.3 | Centered vector dot/cosine, covariance scatter and variance-addition diagram                 | Covariance normalization explicit; correlation undefined at zero variance; independence implies uncorrelated, converse false |
| 8.4 | Least-squares projection, fitted/residual vectors, R² Pythagoras                             | Orthogonality and R² decomposition state intercept/centering assumptions; constant-target case                               |
| 8.5 | Likelihood slider, Gaussian negative-log loss↔MSE, Bernoulli/categorical cross-entropy B1    | Constants/variance assumptions exposed; stable logs; confident-wrong penalty                                                 |
| 8.6 | Probability distributions, surprise bars, entropy/cross-entropy/KL overlays                  | KL direction, zero mass/infinite loss, identical-distribution zero; log base units                                           |
| 8.7 | Repeated-data fits, bias²/variance/noise, train/test curves; L1/L2/Huber outlier drag        | Squared-loss decomposition assumptions; robust-loss transition; regularization versus loss distinction                       |
| 8.8 | Interactive statistics genealogy/review challenges joining distance→projection→Gaussian→loss | All seven why-squares links and limitations navigable; no orphan/mislabeled dependency                                       |

### Part 9 — Learning machines

| ID  | Interactive and pedagogic deliverable                                   | Specific correctness evidence                                                                 |
| --- | ----------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| 9.1 | B1/B3 linear regression from forward/loss/backward/update               | Loss, line and parameters share actual state; compare optimum with least-squares solver       |
| 9.2 | Logistic probability/boundary and binary cross-entropy trace            | Stable sigmoid/log loss; gradient check; probability versus decision threshold                |
| 9.3 | Softmax probability bars and log-sum-exp overflow comparison            | Shift invariance and sum one; large logits stay finite in stable version                      |
| 9.4 | Fully inspectable MLP with B2/B3, every weight/activation/gradient      | One hand-worked step matches core and NumPy; no decorative graph values                       |
| 9.5 | Momentum/Adam trajectories and internal moment/bias-correction readouts | Match explicit reference steps; reset optimizer state; parameter controls affect real updates |
| 9.6 | Learned embedding table, training examples, neighborhoods before/after  | Real training/data provenance; projection caveat; lookup gradients reach correct rows         |
| 9.7 | LayerNorm/RMSNorm centering/scaling and activation distribution         | Normalized axes and epsilon explicit; constant vectors; mean behavior differs between methods |

### Part 10 — Attention and Transformer

| ID   | Interactive and pedagogic deliverable                                                                                       | Specific correctness evidence                                                                                       |
| ---- | --------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| 10.1 | S5 query/key lighting, logits, mask, softmax exposure, value-weighted output                                                | Negative logits remain signed before softmax; output is exact weighted sum; explicit masks                          |
| 10.2 | Dimension slider, random dot-product histograms before/after √d scaling                                                     | Seeded variance experiment under independence/unit-variance assumptions                                             |
| 10.3 | Multi-head reshape/split/concat, batched contraction and linked shape table                                                 | Equivalent einsum and matmul outputs; head index never confused with token axis                                     |
| 10.4 | Positional information, paired-coordinate rotations and RoPE relative-angle effect                                          | Norm preservation, pair layout and frequency convention stated; dot-product relative-position identity              |
| 10.5 | Complete residual/norm/attention/MLP block with selectable token/channel                                                    | Residual dimensions valid; actual computation order stated; every displayed tensor from the model                   |
| 10.6 | End-to-end capstone: select any output number and follow provenance through attention to dot product, cosine and Pythagoras | Automated trace closure plus manual numerical walk; every dependency opens the correct lesson/scene in both locales |

### Advanced track

| ID  | Interactive and pedagogic deliverable                                                               | Specific correctness evidence                                                                                  |
| --- | --------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------- |
| A1  | Gram–Schmidt/QR step animation, basis rulers, least-squares solve and orthogonality residual        | QR reconstruction; dependent columns; classical versus modified stability example                              |
| A2  | SVD sphere/ellipse, PCA centered cloud, rank truncation and LoRA low-rank update                    | Reconstruction/singular values; explained variance; low-rank error; actual LoRA parameter count/update         |
| A3  | Quadratic forms, positive-definite ellipses, Hessian eigen-directions, Newton versus gradient steps | Symmetry assumptions, saddle/indefinite cases, damping and solve failures                                      |
| A4  | Complex plane multiplication, Fourier phasors/signal reconstruction, DFT matrix and FFT butterfly, RoPE connection | Fourier normalization/convention explicit; inverse reconstruction; complex rotation identity; FFT equals DFT, op count n log n |
| A5  | Matrix calculus, Jacobian-vector/VJP paths and forward/reverse work comparison                      | Shape-safe Jv and vᵀJ, same derivative result, tape/memory tradeoffs                                           |
| A6  | Number-format/quantization entrance with rounding-grid and machine-depth map leading through Q1–Q12 | All Q nodes linked; representative error example executes; branch is required completion scope                 |
| A7  | Warp grid/area cells, Jacobian determinant and density change in a toy flow                         | Density uses absolute determinant; invertibility/domain checked; probability mass conserved                    |
| A8  | Covariance ellipses, multivariate Gaussian, whitening and score-vector field                        | Positive-definite covariance, normalized density, score equals analytic gradient; diffusion link               |
| A9  | Convex/nonconvex function explorer, feasible set and Lagrange tangent/gradient geometry             | Local/global distinction, constraint qualification, feasible residual and multiplier conditions                |
| A10 | Basis transformation of vectors/covectors, contractions and tensor products with index notation     | Scalar contractions invariant; component transformation verified; array rank versus geometric tensor explained |

### Branch Q — Number formats and quantization

| ID  | Interactive and pedagogic deliverable                                                                       | Specific correctness evidence                                                                                                                      |
| --- | ----------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| Q1  | Clickable unsigned/two's-complement bits; INT8/INT4 signed ranges                                           | Exhaustive small-format decode; negative endpoints, overflow policy                                                                                |
| Q2  | FP32/FP16 sign/exponent/fraction inspector, ulp ruler, rounding/subnormal/Inf/NaN                           | Bit-exact boundary fixtures, signed zero, ties policy; storage versus arithmetic precision                                                         |
| Q3  | BF16/TF32/FP16/FP32 shared number-line and range/precision comparison                                       | BF16 exponent range; TF32 compute-mode/storage distinction verified against vendor docs                                                            |
| Q4  | E4M3/E5M2/INT8 grid comparison and weight/activation/gradient distributions                                 | Format variant named; saturation/special values correct; no blanket hardware claim                                                                 |
| Q5  | FP6 E2M3/E3M2, FP4 E2M1 all 16 bit patterns, INT4, NF4 quantile grid, ternary/binary/BitNet b1.58           | Exhaustive tiny grids; distinguish signed-zero bit patterns from distinct real values; NF4/codebook source and ternary effective-bit explanation   |
| Q6  | Block scales: MXFP8/MXFP6/MXFP4, NVFP4 and GGUF k-quants with block selection                               | MX E8M0 block-32 and NVFP4 E4M3 block-16 plus tensor FP32 traced; GGUF variants individually named/sourced                                         |
| Q7  | Quantizer scale/zero-point controls, tensor/channel/group selection, clipping/rounding error                | Quantize/dequantize actual data; saturation and zero-range cases; Δ²/12 assumptions                                                                |
| Q8  | Calibration histograms and min-max/percentile/MSE/KL objective sweeps                                       | Shared calibration dataset; train/calibration/evaluation split; independently computed optimum                                                     |
| Q9  | Tiny-network RTN→GPTQ compensation→AWQ scaling, per-column update timeline                                  | Actual reconstructed outputs/loss; Hessian approximation/damping stated; compare baseline fairly                                                   |
| Q10 | W8A8, activation outliers, SmoothQuant transfer, Hadamard/QuaRot/SpinQuant rotation                         | Pre-quantization transform preserves modeled result; scaling/rotation constraints; post-quantization error measured                                |
| Q11 | Fake quant forward, STE backward and QAT training; FP8/FP4 training path explanation                        | STE explicitly an estimator, not true rounding derivative; real training comparison and precision states                                           |
| Q12 | KV-cache quantization and full-network evaluation dashboard; MSE/KL/perplexity/task accuracy/hardware table | Identical evaluation samples; memory includes scales/metadata; real metric computation; vendor-supported format/operation/generation/date recorded |

## Named scene acceptance register

The detailed [LC/DP scene specification](../../design/lessons-law-of-cosines-dot-product.md), [signature scenes](../../design/signature-illustrations.md), [reference storyboard](../../design/part0-1-storyboard.md), and the [basis projection](../../design/backlog-basis-projection-scene.md), [tensor road](../../design/backlog-tensor-road-scene.md), [derivatives](../../design/backlog-derivatives-scenes.md), and [loss/backprop](../../design/backlog-loss-backprop-scenes.md) backlogs remain binding. This register prevents names from disappearing inside broad lesson batches.

| Scene ID         | Owner lessons          | Required visible behavior and evidence                                                                                                                                                       |
| ---------------- | ---------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| INTRO-1          | 0.0a, 1.4 teaser       | The DotUnit (`i,i->` preset, two faces, timeline `unit` steps 0–9): shadow × \|a\| and the tile stack end on the same number; hover of a tile lights its table cell, formula term and code line; drags never reset the timeline          |
| INTRO-2          | 0.0b                   | Zoom ladder: five rungs over live presets (tile, DotUnit, matmul cube, layer cube, attention cubes), fixed camera timeline, click-to-dive and Escape to climb; the number 3 visible at the documented cell of every rung                |
| INTRO-3          | 0.0b, 3.7              | Einsum playground: `parseEinsum`/`layoutEinsum` presets, index sliders 1..8, LOD 8/32/64 with the cap message, summed axis named in the status bar; ≥ 55 fps at ≤ 20 000 instances on the reference iGPU                                 |
| LC-0 / S1        | 0.5                    | 12-step missing-rectangles dissection: actual polygon placement, equal-area shear/rotation, signed obtuse treatment; every step area-checked and reversible; a=b,c=0 hides undefined pieces  |
| LC-1             | 0.5                    | Shared triangle and signed correction ledger sweep from 0 to 180 degrees; exact Pythagoras state and consistent cross-scene hover                                                            |
| LC-2             | 0.5                    | 10-step algebra proof with highlights/marks tied to geometry; dragging never resets timeline; correct projected-foot/sign conditions                                                         |
| LC-3             | 0.5                    | Target-driven two-bone IK, reach rings, raw/clamped acos, elbow side and unreachable gap                                                                                                     |
| DP-1             | 1.4                    | Expand/cancel law-of-cosines terms to component dot sum, numeric product table, matched term IDs and rectangle reveal                                                                        |
| DP-2 / S2        | 1.4                    | Signed rectangle, half-planes, perpendicular magnet, sum/area/brightness faces; zero vectors explicitly undefined where needed                                                               |
| DP-3             | 1.4                    | Orbitable 3D counterparts, projection, perpendicular disc, signed rectangle; antiparallel normal fallback tested                                                                             |
| DP-4 / S2        | 1.4                    | Surface-constrained probe and external light, N/L/angle/terminator, raw versus clamped shader; pixel luminance agrees with numeric result                                                    |
| S3               | 2.4, 3.6               | m×k×n products as light-box cells, signed hue+hatch, k-scrub accumulated C wall, hovered output beam, batches and transposes                                                                 |
| S4               | 3.6                    | Labeled axis sockets/sizes and cables; live einsum/free axes/output shape; invalid connections explain mismatch; actual S3 numbers                                                           |
| S5               | 10.1–10.3              | Query as light, signed dot logits, √d exposure, softmax normalization and weighted V; metaphor never clamps logits to lighting intensity                                                     |
| S6 / D1          | 4.1–4.3                | Microscope zoom-to-straight, h secant→tangent, rise/run, speed car, derivative plot and CG tangent handles; kink remains kink                                                                |
| D2               | 5.1–5.5                | Surface/contours, x/y freeze slices, partial tangents and plane, gradient and directional slice, descent follow-up                                                                           |
| D3               | 5.2–5.4                | Scalar volume, level-set surface, three heat slices, sparse gradients, 3D probe and freeze-two 1D curve                                                                                      |
| D4               | 5.2–5.4                | Independent t₁ scrub with probe-value sparkline/tangent; four partial readouts                                                                                                               |
| D5               | 5.2–5.4                | Independent t₂, five partial readouts, selected two-input surface with others frozen                                                                                                         |
| D6               | 5.2–5.4                | Independent t₃, six linked partial bars, arbitrary pair slice and frozen-value small multiples                                                                                               |
| B1               | 8.2, 8.5, 9.1          | Residuals as actual squares, total and mean distinction, L1 bars/kink, cross-entropy probability/−log curve                                                                                  |
| B2               | 5.6, 9.4               | Actual tape as graph: forward values, local tangent glyphs, reverse products, accumulated gradients and tape/memory view                                                                     |
| B3               | 5.5, 9.1–9.4           | Forward→Loss→Backward→Update controls and optional controlled Auto; η, weight deltas, linked data/graph/loss/surface                                                                         |
| Basis projection | 1.6, 2.2, 2.5, 2.9, A1 | Three ticked rulers, row products/shadows, ruler transfer, column combination, orthonormal flag, norms/pair dots/Gram heatmap, handedness, ghost reconstruction gap, oblique and dual rulers |
| Tensor road      | 3.1–3.5                | Rank 0..6 slider and N-axis model, nested blocks, editable sizes, real-data presets, shape/product, index↔offset, transpose/reshape and reduction                                            |

Basis convention must be explicit and tested: if the basis columns form B, coordinates are solve(B,v); for orthonormal B they are Bᵀv. If the scene calls R=Bᵀ its row-reading matrix, the general coordinate map is R⁻ᵀ. Label basis Gram BᵀB=RRᵀ and row-map Gram RᵀR accurately; both are identity in the square orthogonal case but are different off that case. Singular rulers show lost coordinates instead of an invented inverse.

Tensor road “time”/nested axes and D4–D6 are representations of extra inputs, not a claim to draw six independent spatial directions on a screen. Freeze controls must make this distinction visible.

## Cross-course requirements and traceability

| Requirement from task/notes                    | Implementation coverage                                                           | Acceptance evidence                                                                                         |
| ---------------------------------------------- | --------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| Whole coherent program                         | 2 Start-here pages (0.0a/0.0b, 2026-10-11) + 77 spine entries (1.9 DFT added 2026-10-11) + A1–A10 + Q1–Q12; all rows above | Compare manifest IDs to this register and CURRICULUM; no missing/duplicate entry                            |
| Gentle explanations for CG/VFX reader          | C2/C4, IK, N·L, object/world basis, blend shapes, Graph Editor, tensors as images | Editorial review of every symbol/example; actionable challenges and explanation feedback                    |
| Mind maps everywhere                           | C6, P3, 8.8, 10.6                                                                 | Course roadmap + per-lesson map + genealogy + depth + glossary + flashcards; one source graph               |
| Every number linked                            | C3, Task 7, core, S3/S4/B2/10.6                                                   | Cross-view hover/focus and actual value provenance assertions                                               |
| Beautiful exact holo + secondary clay          | P1, visual-language v3 and reference images                                       | Side-by-side screenshots at desktop/mobile plus complete element checklist below                            |
| DCC interaction                                | ViewportCard, ruler/gizmo/N-panel/status, 6f                                      | Real input tests; standard navigation, no scroll trap, 2× and full screen                                   |
| Tensor/matrix geometry                         | Basis scene, tensor road, S3/S4, Parts 2–3                                        | Coordinates, shapes, products, memory offsets and degeneracies verified                                     |
| Derivatives through six inputs                 | D1–D6/S6, Parts 4–5                                                               | Frozen-coordinate checks, slice/partial consistency and microscope tests                                    |
| Error/backprop step by step                    | B1–B3, core, Parts 8–9                                                            | Numeric tape trace, accumulation and learning updates verified                                              |
| Mandatory statistics/why squares               | Parts 7–8, links to 0.3/1.4/4.6/6.4                                               | All seven AGENTS spine reasons plus L1/Huber limits                                                         |
| Advanced AI anchors                            | A1–A10, Parts 9–10                                                                | QR, LoRA, Newton, RoPE, VJP, flows, score, constraints, true tensors each implemented                       |
| All major low-bit formats/network quantization | A6, Q1–Q12                                                                        | Format coverage, algorithms, calibration, QAT, KV evaluation and sourced hardware support                   |
| English root/Russian /ru                       | 6g and C1                                                                         | All routes, controls, errors, hints, quiz, challenges, tooltips, maps, size controls and notebooks reviewed |
| Python plus Colab                              | P3/C5                                                                             | One source, successful browser/Colab execution, offline/error paths and correct badges                      |
| Static GitHub Pages                            | P15                                                                               | Public deployed routes/assets under /math-course; no server dependency                                      |

### Holo and clay acceptance

Compare to [holo reference](../../design/refs/holo-style-ref.png) and [clay reference](../../design/refs/clay-style-ref.png), using the [visual-language specification](../../design/visual-language.md). Required holo elements: **starfield, colorful nebulae/spiral galaxies, glowing filaments, glass squares/faces with world-unit grid, rulers and ticks, side HUD instruments/dials/readout panels, miniature galaxy panels, the planet/ring instrument, reflective floor, framed formula plate**. Blender header, N-panel, gizmo and status remain.

Record a screenshot location and pass/finding for every element. A shader called “galaxy” or a token called “glass” is not visual proof. Reflection must match actual geometry and never register duplicate handles/labels. Instruments display real angle/scale/term data; the miniature galaxy and planet/ring panels remain part of the reference composition. Glow pixel tests use the normative layer widths and actual half-width radii, not incompatible sample coordinates from an older brief. Clay tiles/cubes use exact sizes and counts, soft material/bevels/contact shadows; the N·L teaching sphere retains exact numeric lighting.

Keep all required elements available and visibly composed at narrow widths by adapting placement and spacing. Reduced motion cuts movement, not content. Reader-controlled overlays are allowed; automatic scope removal for budget/readability is not. Measure bundle/frame/memory costs and optimize or split lazy chunks; any unresolved tradeoff requiring a change to the chosen reference goes back to the operator with concrete evidence.

Covers are separate deliverables for every module and any lesson cover promised by its design. Inventory cover placement; run the existing geometry/text/style checklist before publishing. Reject incorrect squares, perpendiculars, labels, or reflections. Generated covers never replace accurate engine diagrams.

## Verification and release

### Math and browser evidence

For each batch, retain deterministic fixture inputs, expected values with tolerances, screenshot states and the command needed to reproduce them. Test algebraic identities with independently derived values or NumPy references, not a second call to the function under test. Seed random demos; state approximations, domains, and undefined cases. Validate all quiz answers and distractors; port the quiz-lint length/position checks without claiming a statistically balanced quiz teaches the right concept by itself.

Browser coverage on a production build:

- Run the shared interaction and rendering suite in **Chromium, Firefox, and WebKit**. Record each engine/browser version, OS, headless/headed mode, GPU/software rendering mode, automation version, and exact command. Every lesson gets route/render/primary-control smoke coverage in all three engines; each distinct scene/control family gets the full matrix below. Retain traces, screenshots, console logs, and numeric assertions for the tested commit.
- Treat Playwright WebKit or another explicitly named WebKit test environment as representative engine coverage, not proof of Safari on a physical iPhone. Record actual Safari/macOS/iOS/device versions only when those environments were tested; label unavailable physical-device coverage as untested, and keep touch emulation distinct from device input.
- Exercise the **iPhone-style missing-Fullscreen-API path** reproducibly in a named WebKit environment: use a recorded browser-init test fixture that makes the API unavailable before page scripts run, then activate the real size control. Verify fixed-overlay entry/exit, visible shared canvas and correctly aligned Views, retained lesson values/camera, reachable close button, scroll locking and exact restoration of scroll position and focus. Repeat resize/orientation emulation and normal/2×/overlay transitions. Save the fixture, command, browser/OS metadata, and trace; state explicitly that this verifies the fallback contract rather than physical iPhone behavior.

- 1280 px desktop, 900 px transition, 360 px and 320 px narrow layouts; both site themes and both locales.
- Real pointer/keyboard/touch input, Ctrl/Cmd wheel/pinch zoom, ordinary-wheel page scroll, pan/orbit, field typing, Escape/cancel, focus restoration, reset and overlays.
- Normal/2×/full screen transitions, resize and scroll with multiple Views; one canvas, no scene bleed, correct labels/hit tests and chrome stacking. Fullscreen API targets documentElement because the shared canvas lives in body.
- All timeline endpoints and a middle step; backwards and scrubbing; direct manipulation while paused; reduced-motion equivalents with identical numbers.
- Cold-load and drag layout-shift measurement; long Russian strings and live numeric widths; no horizontal page overflow.
- No WebGL/context loss, blocked Python CDN, worker timeout/cancel/retry, unavailable local storage and offline fallback. Show readable DOM math/numbers and Colab alternatives.
- Keyboard and screen-reader semantics: captions, focus order, live value announcements, non-color sign cues, accessible map/list alternatives. Respect touch scrolling.
- Console/network report distinguishes known upstream THREE.Clock notice from new errors; fix favicon/asset 404s and check /dev/ sitemap/noindex policy.

### Performance and accuracy gates

Run the project-required gates with Bun 1.4.3 before each authorized commit:

```powershell
bun test tests/js
bun run check
bun run build
```

Expected: tests pass, check reports **0 errors/warnings/hints**, build has **0 warnings**. Once P3 installs the actual size reporter, also run `bun run size`.

Enforce the ledger's **viz-core ≤300 kB gzip** and original **per-lesson ≤100 kB gzip**; maps/elk/xyflow remain lazy within their documented exception (≤600 kB gzip). Holo implementation allowance from the addendum is ≤18 kB gzip; count its dependencies honestly. Report transitive JavaScript downloaded for each route and after each optional action, not just individual chunk sizes. Pyodide/assets/network startup are reported separately. Restore the temporary raw chunk warning limit by fixing splitting; do not hide budget problems with a larger limit.

Pages without scenes create no WebGL context and load no scene code; Pyodide starts on Run; graph layout runs lazily in a worker. Demand rendering sleeps when no interaction/transition is active. Measure large tensor-road/field scenes on a documented desktop and mobile profile: frame time, draw calls, buffer sizes and peak memory. Preserve the tensor-road target of thousands of cells at 60 fps on the recorded capable profile; report device-specific results honestly. Rust/Wasm is justified only by a measured JS bottleneck and a compared prototype.

### Release checklist

- [ ] All 98 lesson entries satisfy C1–C9 and their specific row checks; all named scene IDs have evidence.
- [ ] Required term graph, roadmap, genealogy, glossary, flashcards and depth links are complete.
- [ ] English/Russian content and UI parity, notebook source/generated output parity, asset provenance and cover geometry review pass.
- [ ] Ledger deferred findings are rechecked: Formula focus/ARIA/layout shifts, DragLayer state-machine behavior, labels/focus rings, progress count, quiz lint, stale links, favicon, dev sitemap, HMR/context-recovery cases. Close with evidence or retain an explicit unresolved finding; do not silently delete it.
- [ ] Full-branch review checks requirements, mathematical accuracy, implementation and visual/browser evidence; requested Fable availability is reported honestly.
- [ ] Required test/check/build/size gates pass on the exact integrated commit; CI runner image is explicitly pinned after checking compatibility.
- [ ] Publish through the existing GitHub Pages workflow under current user authorization; record commit SHA and workflow result.
- [ ] Verify live English and Russian routes, lesson links, images/fonts, workers, notebook/Colab URLs, base path, and representative scenes from every module.
- [ ] Finish with a completion matrix and accessible course URL. If any row remains open, report the course as incomplete and continue its batch rather than reducing scope.
