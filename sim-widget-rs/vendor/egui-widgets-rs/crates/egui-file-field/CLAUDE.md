# egui-file-field — working notebook

## What this is

A path text field with a Browse icon on its right, plus the last directory the user browsed from.
Two types: `FileField` (the widget) and `FileFieldState` (the bit worth persisting).

## What it deliberately does NOT do

**It never opens a dialog.** That is the whole design decision, and it is not squeamishness about
dependencies:

- this widget set keeps pickers out — see `egui-arbor` and `egui-asset-browser`, whose Cargo.toml
  headers say "no rfd" in as many words;
- more importantly, WHICH picker is the host's call. playa lets the user choose in-app
  (`egui-file-dialog`) or native (`rfd`) in Preferences, and a widget that opened its own would
  silently override the setting that user chose.

So `show()` returns `browse_clicked()`, the host opens whatever it opens starting at `start_dir()`,
and hands the result back through `remember()`.

## `start_dir` — the rule, and why

1. the parent of whatever the field currently holds, when that directory exists;
2. otherwise the remembered directory;
3. otherwise **empty**.

(1) beats (2) because a field already pointing at a file must not send the user back to somewhere
they browsed an hour ago. (3) is empty rather than a guess: a picker that opens in the process's
working directory because a widget invented one is worse than letting the host apply its own
default. `a_parent_that_is_not_there_falls_back` pins the case where a path is typed by hand into a
directory that does not exist.

## State and persistence

`FileFieldState` is `Serialize + Deserialize` and holds only `last_dir`. The PATH is not in it: it
belongs to whatever the field edits — a node attr, a preference, a form — and duplicating it here
would be a second copy of a value that changes.

## Layout

The icon button is sized from `interact_size.y`, so field and button read as one control instead of
a field with a chip beside it. The text part takes the remaining width unless `desired_width` says
otherwise.

## TODO

- [ ] Drag-and-drop a file onto the field.
- [ ] Optional validity tint (path exists / does not) — needs a rule for sequences, where the mask
      itself is never a file that exists.
