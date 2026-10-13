"""Regenerates ocio_cpu_tables_pyocio.json (ROADMAP D4): OpenColorIO's CPU result for
FileTransforms on OCIO's own LUT fixtures (every interpolation the format takes, forward and
inverse), ExposureContrast in its three styles and LogCamera on both sides of the break.

Run from this directory with the PyOpenColorIO version recorded in the file. The fixtures are
the copies in crates/ocio/vfx-lut/tests/data/ocio (from OpenColorIO tests/data/files)."""
import json
import os

import PyOpenColorIO as O

HERE = os.path.dirname(os.path.abspath(__file__))
FIXTURES = os.path.normpath(os.path.join(HERE, '..', '..', '..', 'vfx-lut', 'tests', 'data', 'ocio'))

# Chromatic, off-grid probes; negatives and values above 1 where a LUT clamps or extrapolates.
PROBES = [
    [0.18, 0.18, 0.18],
    [0.5, 0.4, 0.3],
    [0.02, 0.3, 0.9],
    [0.93, 0.07, 0.61],
    [0.333, 0.777, 0.111],
    [0.71, 0.52, 0.29],
    [1.2, 0.8, -0.05],
    [0.0, 1.0, 0.5],
]

cfg = O.Config.CreateRaw()
cfg.setSearchPath(FIXTURES)


def cpu(t):
    return cfg.getProcessor(t).getDefaultCPUProcessor()


def run(t):
    p = cpu(t)
    return [p.applyRGB(list(v)) for v in PROBES]


def run_exact(t):
    # OCIO's default (VERY_GOOD) includes OPTIMIZATION_LUT_INV_FAST, which replaces an inverse
    # LUT by a baked forward approximation (OpenColorTypes.h:711-722). vfx-ocio inverts
    # exactly, as OCIO does without that flag: compare against LOSSLESS.
    p = cfg.getProcessor(t).getOptimizedCPUProcessor(O.OPTIMIZATION_LOSSLESS)
    return [p.applyRGB(list(v)) for v in PROBES]


INTERP = {
    'linear': O.INTERP_LINEAR,
    'tetrahedral': O.INTERP_TETRAHEDRAL,
    'best': O.INTERP_BEST,
}
DIRS = {'forward': O.TRANSFORM_DIR_FORWARD, 'inverse': O.TRANSFORM_DIR_INVERSE}

files = []
for name, interps in [
    ('cpf.spi1d', ['linear']),
    ('iridas_1d.cube', ['linear']),
    ('iridas_3d.cube', ['linear', 'tetrahedral']),
    ('discreet-3d-lut.3dl', ['linear', 'tetrahedral']),
    ('lut3d_bizarre.spi3d', ['linear', 'tetrahedral']),
    ('nuke_3d.vf', ['linear', 'tetrahedral']),
    ('resolve_1d3d.cube', ['linear', 'tetrahedral']),
    ('camera_to_aces.spimtx', ['linear']),
]:
    for interp in interps:
        for d in ('forward', 'inverse'):
            t = O.FileTransform(src=name, interpolation=INTERP[interp], direction=DIRS[d])
            files.append({'file': name, 'interpolation': interp, 'direction': d, 'out': run_exact(t)})

ec = []
for style, sname in [
    (O.EXPOSURE_CONTRAST_LINEAR, 'linear'),
    (O.EXPOSURE_CONTRAST_VIDEO, 'video'),
    (O.EXPOSURE_CONTRAST_LOGARITHMIC, 'log'),
]:
    for d in ('forward', 'inverse'):
        t = O.ExposureContrastTransform(
            style=style, exposure=0.7, contrast=1.3, gamma=1.1, pivot=0.18,
            logExposureStep=0.088, logMidGray=0.435, direction=DIRS[d])
        ec.append({'style': sname, 'exposure': 0.7, 'contrast': 1.3, 'gamma': 1.1, 'pivot': 0.18,
                   'log_exposure_step': 0.088, 'log_mid_gray': 0.435, 'direction': d,
                   'out': run(t)})

# LogCamera, ARRI-LogC3-like parameters with a linear segment below the break; the probes
# 0.0, 0.002 and 0.005 sit under it.
LOGCAM = dict(base=10.0, logSideSlope=[0.247190] * 3, logSideOffset=[0.385537] * 3,
              linSideSlope=[5.555556] * 3, linSideOffset=[0.052272] * 3,
              linSideBreak=[0.010591] * 3)
logcam = []
for d in ('forward', 'inverse'):
    t = O.LogCameraTransform(linSideBreak=LOGCAM['linSideBreak'], base=LOGCAM['base'],
                             logSideSlope=LOGCAM['logSideSlope'],
                             logSideOffset=LOGCAM['logSideOffset'],
                             linSideSlope=LOGCAM['linSideSlope'],
                             linSideOffset=LOGCAM['linSideOffset'], direction=DIRS[d])
    probes = [[0.0, 0.002, 0.005], [0.01, 0.0106, 0.02], [0.18, 0.5, 1.0], [-0.01, 0.3, 2.0]]
    p = cpu(t)
    logcam.append({'direction': d, 'probes': probes, 'out': [p.applyRGB(v) for v in probes]})

json.dump(
    {
        'generator': 'PyOpenColorIO %s, raw config, CPU processor (LUT files: OPTIMIZATION_LOSSLESS, '
                     'exact inverses; the rest: default optimization); '
                     'crates/ocio/vfx-ocio/tests/data/gen_ocio_cpu_tables.py' % O.__version__,
        'probes': PROBES,
        'files': files,
        'exposure_contrast': ec,
        'log_camera': {'params': LOGCAM, 'rows': logcam},
    },
    open(os.path.join(HERE, 'ocio_cpu_tables_pyocio.json'), 'w'),
    indent=1,
)
print(len(files), 'file rows,', len(ec), 'EC rows,', len(logcam), 'LogCamera rows')
