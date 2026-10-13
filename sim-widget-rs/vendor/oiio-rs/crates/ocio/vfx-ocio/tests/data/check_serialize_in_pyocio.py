"""The reverse check of tests/ocio_config_yaml_parity.rs: PyOpenColorIO reads this port's
`Config::serialize` output.

    VFX_OCIO_DUMP_DIR=<dir> cargo test -p vfx-ocio --test ocio_config_yaml_parity dump_serialized
    python check_serialize_in_pyocio.py <dir>

For every embedded config (`<name>.ocio`, the original is OCIO's built-in or the embedded file)
and every fixture config: PyOCIO loads and validates our text, PyOCIO's serialize of it equals
PyOCIO's serialize of the original (so we write no key OCIO does not read and drop none it does),
and every colour space converts to and from the reference identically.
"""
import glob
import os
import shutil
import sys
import PyOpenColorIO as O

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, "ocio_configs")
EMBEDDED = os.path.normpath(os.path.join(HERE, "..", "..", "src", "builtin", "data"))
BUILTINS = {n for n, _ui, _rec, _default in O.BuiltinConfigRegistry().getBuiltinConfigs()}


def original(name):
    stem = name[: -len(".ocio")]
    if stem in BUILTINS:
        return O.Config.CreateFromBuiltinConfig(stem)
    if os.path.exists(os.path.join(EMBEDDED, name)):
        return O.Config.CreateFromFile(os.path.join(EMBEDDED, name))
    return O.Config.CreateFromFile(os.path.join(SRC, name))

out = sys.argv[1]
if os.path.isdir(os.path.join(SRC, "luts")) and not os.path.isdir(os.path.join(out, "luts")):
    shutil.copytree(os.path.join(SRC, "luts"), os.path.join(out, "luts"))
PX = [0.4, 0.5, 0.6, 0.02, 0.18, 0.9, 2.0, 0.01, 0.3]
failed = False
for f in sorted(glob.glob(os.path.join(out, "*.ocio"))):
    name = os.path.basename(f)
    if name == "every_key_pyocio.ocio":
        continue
    a = original(name)
    b = O.Config.CreateFromFile(f)
    b.validate()
    same = a.serialize() == b.serialize()
    ref = "lin" if name.startswith("every_key") else "ACES2065-1"
    n = bad = 0
    for cs in a.getColorSpaceNames(O.SEARCH_REFERENCE_SPACE_ALL, O.COLORSPACE_ALL):
        for args in ((cs, ref), (ref, cs)):
            try:
                pa = a.getProcessor(*args).getDefaultCPUProcessor()
            except Exception:  # noqa: BLE001 - what OCIO cannot build is not compared
                continue
            x, y = list(PX), list(PX)
            pa.applyRGB(x)
            b.getProcessor(*args).getDefaultCPUProcessor().applyRGB(y)
            n += 1
            bad += max(abs(p - q) for p, q in zip(x, y)) > 1e-6
    print("%s: serialize equal %s, %d processors, %d differ" % (name, same, n, bad))
    failed |= (not same) or bad > 0
sys.exit(1 if failed else 0)
