"""Regenerates the embedded built-in configs and the PyOpenColorIO references for them.

Run from anywhere with the PyOpenColorIO version recorded in the JSON (2.5.1). It writes:

- ../../src/builtin/data/<name>.ocio: OCIO's own built-in config text for every entry of
  `BuiltinConfigRegistry` (the ACES CG and Studio configs of each ACES release), exactly as
  PyOpenColorIO hands it out (`BuiltinConfigRegistry()[name]`), line endings normalised to LF.
  These are the files `vfx_ocio::builtin` embeds: this script is how they are kept in sync.
- ocio_configs/<name>_pyocio.ocio: PyOpenColorIO's `serialize()` of each built-in config, the
  reference `Config::serialize` is compared with; only where it differs from OCIO's text (for
  the v4 configs the text already is its own serialize(), so the embedded file is the reference,
  "pyocio_serialize": null).
- every_key_pyocio.ocio: PyOpenColorIO's `serialize()` of every_key.ocio (a hand-written config
  that uses every key OCIO's YAML reader knows, with non-default values).
- ocio_configs_pyocio.json:
  - "builtin_registry": OCIO's registry (name, UI name, recommended, default) in OCIO's order,
    with the length and FNV-1a 64 hash of each config text, and what `ResolveConfigPath` makes of
    the special URIs;
  - "cg_subset": for each CG config, the colour spaces, roles, displays/views and named
    transforms it defines. The script asserts, with PyOpenColorIO, that the Studio config of
    the same version has each of them with the same definition;
  - "configs": PyOpenColorIO CPU results on three probes for every colour space to and from the
    reference, every display/view from the scene-linear role, and every named transform both
    ways, plus each colour space's aliases and interop ID. A config is either a built-in
    ("builtin": its name) or a fixture file ("file").

The Studio all-views config vfx-ocio also embeds (src/builtin/data/studio-config-all-views-*) is
not an OCIO built-in; it is PyOpenColorIO's serialize() of the OpenColorIO-Config-ACES all-views
release, so its serialize reference is the embedded file itself (no copy under ocio_configs/).
"""
import json
import os
import PyOpenColorIO as O

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "ocio_configs")
DATA = os.path.normpath(os.path.join(HERE, "..", "..", "src", "builtin", "data"))
PROBES = [[0.18, 0.18, 0.18], [0.5, 0.4, 0.3], [0.02, 0.3, 0.9]]
ALL_VIEWS = "studio-config-all-views-v4.0.0_aces-v2.0_ocio-v2.5"
ALL_CS = (O.SEARCH_REFERENCE_SPACE_ALL, O.COLORSPACE_ALL)


def fnv1a64(data):
    h = 0xCBF29CE484222325
    for b in data:
        h ^= b
        h = (h * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return "%016x" % h


def write(path, text):
    with open(path, "w", newline="\n", encoding="utf-8") as f:
        f.write(text)


def run(proc):
    cpu = proc.getDefaultCPUProcessor()
    return [cpu.applyRGB(list(p)) for p in PROBES]


def reference_rows(cfg, ref):
    rows = []
    names = cfg.getColorSpaceNames(*ALL_CS)
    for cs in names:
        for src, dst in ((cs, ref), (ref, cs)):
            try:
                rows.append({"src": src, "dst": dst, "out": run(cfg.getProcessor(src, dst))})
            except Exception as e:  # noqa: BLE001 - recorded, the Rust side must fail too
                rows.append({"src": src, "dst": dst, "error": str(e)})
    scene = cfg.getRoleColorSpace("scene_linear") or ref
    for display in cfg.getDisplaysAll():
        for view in cfg.getViews(display):
            try:
                out = run(cfg.getProcessor(scene, display, view, O.TRANSFORM_DIR_FORWARD))
                rows.append({"src": scene, "display": display, "view": view, "out": out})
            except Exception as e:  # noqa: BLE001
                rows.append({"src": scene, "display": display, "view": view, "error": str(e)})
    for nt in cfg.getNamedTransformNames():
        for d, name in ((O.TRANSFORM_DIR_FORWARD, "forward"), (O.TRANSFORM_DIR_INVERSE, "inverse")):
            try:
                rows.append({"named": nt, "direction": name, "out": run(cfg.getProcessor(cfg.getNamedTransform(nt), d))})
            except Exception as e:  # noqa: BLE001
                rows.append({"named": nt, "direction": name, "error": str(e)})
    return rows


def colorspaces(cfg):
    out = []
    for n in cfg.getColorSpaceNames(*ALL_CS):
        cs = cfg.getColorSpace(n)
        out.append({"name": n, "aliases": list(cs.getAliases()), "interop_id": cs.getInteropID()})
    return out


def views(cfg, display):
    return [
        {
            "view": v,
            "colorspace": cfg.getDisplayViewColorSpaceName(display, v),
            "view_transform": cfg.getDisplayViewTransformName(display, v),
            "looks": cfg.getDisplayViewLooks(display, v),
        }
        for v in cfg.getViews(display)
    ]


def cg_subset(cg, studio):
    """What `cg` defines, asserting `studio` defines each item the same way."""
    out = {"colorspaces": [], "roles": [], "displays": [], "named_transforms": []}
    for n in cg.getColorSpaceNames(*ALL_CS):
        s = studio.getColorSpace(n)
        assert s is not None and s.getName() == n and str(s) == str(cg.getColorSpace(n)), n
        out["colorspaces"].append(n)
    studio_roles = dict(studio.getRoles())
    for role, cs in cg.getRoles():
        assert studio_roles.get(role) == cs, role
        out["roles"].append([role, cs])
    for d in cg.getDisplaysAll():
        mine, theirs = views(cg, d), views(studio, d)
        for v in mine:
            assert v in theirs, (d, v)
        # The default view (the first) is the same in both.
        assert mine[0] == theirs[0], d
        out["displays"].append({"display": d, "views": [v["view"] for v in mine]})
    for n in cg.getNamedTransformNames(O.NAMEDTRANSFORM_ALL):
        s = studio.getNamedTransform(n)
        assert s is not None and str(s) == str(cg.getNamedTransform(n)), n
        out["named_transforms"].append(n)
    for vt in cg.getViewTransforms():
        s = studio.getViewTransform(vt.getName())
        assert s is not None and str(s) == str(vt), vt.getName()
    for look in cg.getLookNames():
        assert str(studio.getLook(look)) == str(cg.getLook(look)), look
    return out


registry = []
configs = []
subset = []
reg = O.BuiltinConfigRegistry()
for name, ui, recommended, default in reg.getBuiltinConfigs():
    text = reg[name].replace("\r\n", "\n")
    write(os.path.join(DATA, name + ".ocio"), text)
    raw = text.encode("utf-8")
    registry.append({
        "name": name, "ui_name": ui, "recommended": recommended, "default": default,
        "len": len(raw), "fnv1a64": fnv1a64(raw),
    })
    cfg = O.Config.CreateFromBuiltinConfig(name)
    fixture = os.path.join(OUT, name + "_pyocio.ocio")
    if cfg.serialize() == text:
        # OCIO's text is already its own serialize() (the v4 configs): no second copy.
        serialized = None
        if os.path.exists(fixture):
            os.remove(fixture)
    else:
        serialized = name + "_pyocio.ocio"
        write(fixture, cfg.serialize())
    configs.append(({"builtin": name, "pyocio_serialize": serialized}, cfg, "ACES2065-1"))
    if name.startswith("cg-config-"):
        studio_name = "studio-config-" + name[len("cg-config-"):]
        sub = cg_subset(cfg, O.Config.CreateFromBuiltinConfig(studio_name))
        sub.update({"cg": name, "studio": studio_name})
        subset.append(sub)

embedded = open(os.path.join(DATA, ALL_VIEWS + ".ocio"), encoding="utf-8").read()
cfg = O.Config.CreateFromFile(os.path.join(DATA, ALL_VIEWS + ".ocio"))
# The embedded all-views file is PyOpenColorIO's own serialize() of itself.
assert cfg.serialize() == embedded, "the embedded all-views config is not PyOCIO's serialize()"
configs.append(({"builtin": ALL_VIEWS, "pyocio_serialize": None}, cfg, "ACES2065-1"))

cfg = O.Config.CreateFromFile(os.path.join(OUT, "every_key.ocio"))
write(os.path.join(OUT, "every_key_pyocio.ocio"), cfg.serialize())
configs.append(({"file": "every_key.ocio", "pyocio_serialize": "every_key_pyocio.ocio"}, cfg, "lin"))

uris = ["ocio://default", "ocio://cg-config-latest", "ocio://studio-config-latest",
        "ocio://DEFAULT", "ocio://Studio-Config-Latest", "ocio://" + registry[0]["name"],
        "ocio://no-such-config", "config.ocio", ""]
json.dump(
    {
        "generator": "PyOpenColorIO %s, BuiltinConfigRegistry, Config.serialize and default CPU processors" % O.__version__,
        "probes": PROBES,
        "builtin_registry": registry,
        "resolve_config_path": [[u, O.ResolveConfigPath(u)] for u in uris],
        "cg_subset": subset,
        "configs": [
            dict(meta, reference=ref, colorspaces=colorspaces(c), rows=reference_rows(c, ref))
            for meta, c, ref in configs
        ],
    },
    open(os.path.join(HERE, "ocio_configs_pyocio.json"), "w", newline="\n"),
    indent=0,
)
