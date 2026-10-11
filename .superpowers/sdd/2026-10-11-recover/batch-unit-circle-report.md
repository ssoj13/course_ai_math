# Batch report: 0.4 unit circle (worktree branch worktree-agent-aaf61909e9890eaef)

## Resume note (paused by coordinator, 2026-10-11)
- State: all four scenes, EN + RU pages, dictionary, tests done; last gates run after the final code change:
  `bun test tests/js` 383 pass / 0 fail; `bun run check` 0/0/0; `bun run build` 0 warnings, 39 pages;
  renderer chunk ViewportCard 268,106 B gz (raw 1,005,446).
- The last edit (shadow bands -> solid beams in UC-1/UC-3) is built but NOT re-screenshotted. To resume:
  1. `bunx astro preview --port 4512`; `node <scratchpad>/uc/shots.mjs <out>`; look at UC-1/UC-3 at 1280/360 EN/RU.
  2. Copy final screenshots into this folder (`batch-unit-circle-evidence/`), finish this report, commit as non-WIP.
  3. RU stamp is current (EN unchanged since `bun run i18n:stamp`); restamp if EN prose changes.
- Scripts used (scratchpad `uc/`): shots.mjs (cards + controls per locale/width), interact.mjs (snap buttons,
  timeline, unroll, keyboard handle), drag.mjs (real mouse drags located by the grab cursor), overflow.mjs.

## Scenes delivered vs required (storyboard §2 0.4, completion map row 0.4)
- UC-1 shadows: P dragged by angle (atan2, unwrap, 1° steps, magnet 3° on multiples of 30/45 incl. 360), cos/sin
  bars on the axes, dashed legs (horizontal = |cos| red, vertical = |sin| green), values, quadrant signs overlay,
  special-angle marks, formula (θ scrub, P = (cos, sin), cos²+sin² = 1), readout with quadrant signs, 3 challenges.
- UC-2 radian ruler: arc unrolls onto a ruler from (1,0) along x (unit = radius, π / 2π marks); `roll` 0..1 eased by
  buttons or N-panel; end handle on circle (roll 0) or along ruler; formula d·π/180, s = r·θ, θ/2π; 2 challenges.
- UC-3 waves: circle + cos/sin graphs (1 world unit = 1 radian), traced solid up to θ, dashed ahead; drag P or either
  graph point; 5-step quarter-turn Timeline (step change eases θ; θ on a quarter syncs the step); 2 challenges.
- UC-4 special angles + 45° mirror: P at θ, Q = mirror at 90° − θ (draggable too), diagonal, chord with right angle,
  both θ arcs, legs; snap buttons + NumGrid table whose current row stays bright; 1 challenge.
- Prose: removed English words from TeX (two display formulas), added scene instructions and an atan2/full-wrap
  paragraph. RU page full translation, stamped; removed from untranslated.json.

## Engine changes (shared files)
- `src/viz/scene/ViewportCard.tsx`: new optional prop `panelOpen` (initial desktop N-panel state, default true).
- `src/styles/lab.css`: `.lab-num-table .is-dim { opacity: .35; }` (NumGrid dim marks had no CSS).
- `src/i18n/ui.ts`: new `uc` namespace (EN + RU) inserted before the Python-cell block.

## Verified in browser (headless Chrome, production preview)
- 1 canvas, 4 cards; no horizontal scroll at 360 (EN/RU); console: known THREE.Clock warning, favicon 404 only.
- Real mouse drags: UC-1 P to the bottom (θ 279), UC-3 graph point +60 px (+58°), UC-4 Q drag moves P to 90° − angle.
- Keyboard handle (arrows), snap buttons, timeline sweep, unroll at 360 → challenges complete.

## Open items
- [ ] Re-screenshot after the bar change; copy evidence here.
- [ ] Phone (360) scenes are small; UC-3 graph labels overlap guide lines at 360 (engine framing / label overlay).
- [ ] UC-2 unroll is a straight blend between exact ends (lengths exact at roll 0 and 1 only); documented in math.ts.
