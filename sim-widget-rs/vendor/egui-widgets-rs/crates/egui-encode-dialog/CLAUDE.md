# egui-encode-dialog

A generic, **data-driven** "export / encode settings" modal for egui 0.34.

## What it is
A staged settings dialog: pick an output **format**, then a **codec**, then edit
that codec's **options** (bit depth / compression / quality / bitrate / …), set an
optional **frame range** and **output path**, and press **Start** or **Cancel**.

It is a decoupled port of playa's `dialogs/encode/encode_ui.rs` + `encode.rs`. The
original was welded to playa: hard-coded H.264/H.265/ProRes/AV1/EXR/PNG tables,
`playa_io::ffmpeg` encoder probing + encoding, `playa_engine` frame pulling, and
`rfd` for the file picker. **None of that lives here** — the widget is pure UI +
state and knows nothing about ffmpeg, playa, or a file dialog.

## Why it's generic
The codec/option set is supplied by the caller as data, so any egui app can reuse
the dialog by describing its own export targets.

## Model (`src/`)
- `schema.rs` — what the caller provides (read-only to the dialog):
  - `EncodeOption` — `Bool` | `Int{min,max}` | `Float{min,max}` | `Choice{labels}` |
    `Text`, each with an `id` (machine key) + `label` (display). Typed ctors:
    `EncodeOption::int/float/boolean/choice/text(...)`.
  - `Codec { id, label, available, hint, options }` — `Codec::new(...).available(b).hint(s)`.
  - `Format { id, label, extension, codecs }` — `Format::new(...)`.
  - `EncodeSchema { formats }` — `EncodeSchema::new([...])`.
- `settings.rs` — the chosen values (working state **and** the `Start` payload):
  - `OptionValue` — `Bool/Int/Float/Choice(index)/Text`, with `as_*()` readers.
  - `FrameRange { enabled, start, end }` — `enabled=false` ⇒ "use host's range".
  - `EncodeSettings { format_id, codec_id, output_path, frame_range, option_values }`
    with `get_bool/int/float/choice/text(id)` accessors. `option_values` holds the
    **currently selected codec's** values, keyed by option id.
- `egui_encode_dialog.rs` — the widget:
  - `EncodeDialog::new(schema)` / `with_settings(schema, settings)` (restores +
    repairs persisted state against the schema).
  - `EncodeDialog::show(ctx, &ShowConfig) -> EncodeDialogResult`.
  - `EncodeDialogResult` = `Open` | `Cancelled` | `Start(EncodeSettings)`.
  - `ShowConfig { title, width, show_browse, show_frame_range, start_label }`.

## Result enum / flow
Call `show()` every frame while open. On `Cancelled` or `Start(settings)`, stop
calling it. The host maps `settings.format_id` / `codec_id` / per-option values
back onto its real encoder.

## Path / Browse
The dialog edits `output_path` as **text** and keeps its extension synced to the
selected format. There is NO `rfd`: a "Browse…" button (toggle via
`ShowConfig::show_browse`) just sets a flag — call `take_browse_request()`, open
your own picker, and write the result back via `set_output_path(...)`. A "+" button
increments the trailing number in the filename (file009→file010), preserving pad.

## Gotchas
- egui 0.34: uses native `egui::Modal` (centered, backdrop-dim, Esc/backdrop close
  via `ModalResponse::should_close`). `ComboBox::from_id_salt`, `Slider::new`.
- Switching codec **re-seeds** `option_values` from the new codec's defaults (each
  codec presents its own option set). To preserve per-codec edits across switches,
  persist the whole `EncodeSettings` per codec on the host side.
- `Choice` stores the selected **index** into `labels`; resolve back via the schema.
- Unavailable codecs (`available=false`) are listed greyed-out with a ✗ hover, but
  selection skips them; `EncodeDialog::new` picks the first *available* codec.

## What was dropped vs. source (host's job now)
- All ffmpeg probing/encoding, the progress channel / worker thread / `EncodeStage`.
- Concrete codec & sequence-format enums + per-format settings structs → caller schema.
- `rfd` file dialog → `show_browse` flag + `take_browse_request()`.
- playa frame-pull and EXR pass-through layer inspection.

## Build / verify
Standalone crate (carries its own bare `[workspace]`; not in the parent members).
From this dir: `cargo check` / `cargo clippy --all-targets` / `cargo test`.

(Gathered from playa.)
