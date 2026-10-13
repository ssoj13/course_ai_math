"""Regenerates ocio_builtins_pyocio.json: every style in OCIO's BuiltinTransformRegistry,
forward CPU result on five probes. Run with the PyOpenColorIO version recorded in the file."""
import json
import PyOpenColorIO as O

PROBES = [[0.18, 0.18, 0.18], [0.5, 0.4, 0.3], [0.02, 0.3, 0.9], [1.5, 0.8, 0.05], [-0.05, 0.1, 0.25]]
cfg = O.Config.CreateRaw()
rows = []
for style, _desc in O.BuiltinTransformRegistry().getBuiltins():
    proc = cfg.getProcessor(O.BuiltinTransform(style)).getDefaultCPUProcessor()
    rows.append({"style": style, "forward": [proc.applyRGB(list(p)) for p in PROBES]})
json.dump(
    {
        "generator": "PyOpenColorIO %s, BuiltinTransformRegistry, CPU processor (default optimization)" % O.__version__,
        "probes": PROBES,
        "builtins": rows,
    },
    open("ocio_builtins_pyocio.json", "w"),
    indent=1,
)
