"""Regenerates ocio_viewing_pipeline_pyocio.json: PyOpenColorIO's LegacyViewingPipeline on the
fixture config viewing_pipeline.ocio and on the built-in ocio://default config (ACEScct input),
RGBA probes through the CPU processor (OPTIMIZATION_LOSSLESS, so no fast pow/exp
approximation), plus the exact text of the errors the pipeline raises.

Run from this directory with the PyOpenColorIO version recorded in the file. Each case is a
plain description (no OCIO objects) that tests/legacy_viewing_pipeline.rs rebuilds with vfx-ocio's
own types, so both sides construct the pipeline from the same numbers."""
import json
import os

import PyOpenColorIO as O

HERE = os.path.dirname(os.path.abspath(__file__))
FIXTURE = os.path.join(HERE, 'viewing_pipeline.ocio')

# Chromatic RGBA probes: below/above mid grey, a saturated one, a negative, alpha != 1.
PROBES = [
    [0.18, 0.18, 0.18, 1.0],
    [0.5, 0.4, 0.3, 0.7],
    [0.02, 0.3, 0.9, 0.25],
    [1.5, 0.8, 0.05, 1.0],
    [0.05, 0.1, 0.25, 0.5],
]

DIRS = {'forward': O.TRANSFORM_DIR_FORWARD, 'inverse': O.TRANSFORM_DIR_INVERSE}
STYLES = {'linear': O.EXPOSURE_CONTRAST_LINEAR, 'video': O.EXPOSURE_CONTRAST_VIDEO}


def ec(spec):
    """An ExposureContrastTransform from {style, exposure, contrast, gamma, pivot, dynamic}."""
    t = O.ExposureContrastTransform(
        style=STYLES[spec.get('style', 'linear')], exposure=spec.get('exposure', 0.0),
        contrast=spec.get('contrast', 1.0), gamma=spec.get('gamma', 1.0),
        pivot=spec.get('pivot', 0.18))
    for p in spec.get('dynamic', []):
        {'exposure': t.makeExposureDynamic, 'contrast': t.makeContrastDynamic,
         'gamma': t.makeGammaDynamic}[p]()
    return t


def pipeline(case):
    lvp = O.LegacyViewingPipeline()
    dvt = O.DisplayViewTransform(
        src=case['src'], display=case['display'], view=case['view'],
        looksBypass=case.get('looks_bypass', False), dataBypass=case.get('data_bypass', True),
        direction=DIRS[case.get('direction', 'forward')])
    lvp.setDisplayViewTransform(dvt)
    if case.get('linear_cc'):
        lvp.setLinearCC(ec(case['linear_cc']))
    if case.get('color_timing_cc'):
        lvp.setColorTimingCC(ec(case['color_timing_cc']))
    if case.get('display_cc'):
        lvp.setDisplayCC(ec(case['display_cc']))
    if case.get('channel_view'):
        cv = case['channel_view']
        if 'matrix' in cv:  # an explicit matrix: the alpha-entry cases
            m = O.MatrixTransform(matrix=cv['matrix'])
        else:
            m = O.MatrixTransform.View(channelHot=cv['hot'], lumaCoef=[0.2126, 0.7152, 0.0722])
        lvp.setChannelView(m)
        cv['matrix'] = list(m.getMatrix())  # what vfx-ocio gets, so both start from one matrix
    if case.get('looks_override') is not None:
        lvp.setLooksOverride(case['looks_override'])
        lvp.setLooksOverrideEnabled(True)
    return lvp


def run(cfg, case):
    proc = pipeline(case).getProcessor(cfg)
    cpu = proc.getOptimizedCPUProcessor(O.OPTIMIZATION_LOSSLESS)
    if 'set_exposure' in case:
        # Dynamic exposure moved on ONE processor: OCIO's reference for the CPU path.
        prop = cpu.getDynamicProperty(O.DYNAMIC_PROPERTY_EXPOSURE)
        outs = []
        for ev in case['set_exposure']:
            prop.setDouble(ev)
            outs.append([cpu.applyRGBA(list(p)) for p in PROBES])
        return {'out_by_exposure': outs}
    return {'out': [cpu.applyRGBA(list(p)) for p in PROBES]}


fixture = O.Config.CreateFromFile(FIXTURE)
aces = O.Config.CreateFromBuiltinConfig('ocio://default')
# Same fixture with only one active view: OCIO's getView does not filter by the active list.
fixture_active = O.Config.CreateFromStream(open(FIXTURE).read().replace(
    'active_views: [Standard, Looked, Raw, Flat]', 'active_views: [Standard]'))
CFG = {'fixture': fixture, 'default': aces, 'fixture_active': fixture_active}

EV1 = {'exposure': 1.0}
cases = [
    # linear input: exposure in scene_linear == a gain on the input
    dict(name='lin_ev1', cfg='fixture', src='lin', display='Disp', view='Standard', linear_cc=EV1),
    # log input: the exposure must act in scene_linear, not on the log codes
    dict(name='log_ev1', cfg='fixture', src='logcs', display='Disp', view='Standard', linear_cc=EV1),
    dict(name='log_ev_neg', cfg='fixture', src='logcs', display='Disp', view='Standard',
         linear_cc={'exposure': -1.5}),
    dict(name='aces_cct_ev1_video', cfg='default', src='ACEScct', display='sRGB - Display',
         view='Video (colorimetric)', linear_cc=EV1),
    dict(name='aces_cct_plain_video', cfg='default', src='ACEScct', display='sRGB - Display',
         view='Video (colorimetric)'),
    dict(name='aces_cct_ev1_sdr', cfg='default', src='ACEScct', display='sRGB - Display',
         view='ACES 2.0 - SDR 100 nits (Rec.709)', linear_cc=EV1),
    dict(name='aces_srgb_tex_ev1', cfg='default', src='sRGB Encoded Rec.709 (sRGB)',
         display='sRGB - Display', view='Video (colorimetric)', linear_cc={'exposure': 0.5}),
    # color_timing role
    dict(name='ct_cc', cfg='fixture', src='lin', display='Disp', view='Standard',
         color_timing_cc={'exposure': 0.5, 'contrast': 1.2}),
    dict(name='lin_and_ct', cfg='fixture', src='logcs', display='Disp', view='Standard',
         linear_cc=EV1, color_timing_cc={'exposure': 0.5, 'contrast': 1.2}),
    # a no-op correction drops its conversion: output equals the bare display transform
    dict(name='noop_cc', cfg='fixture', src='logcs', display='Disp', view='Standard',
         linear_cc={'exposure': 0.0}, color_timing_cc={'exposure': 0.0}),
    dict(name='none_cc', cfg='fixture', src='logcs', display='Disp', view='Standard'),
    # looks: the view's, bypassed, overridden
    dict(name='view_looks', cfg='fixture', src='lin', display='Disp', view='Looked'),
    dict(name='view_looks_in_log', cfg='fixture', src='logcs', display='Disp', view='Looked',
         linear_cc=EV1),
    dict(name='view_looks_bypass', cfg='fixture', src='lin', display='Disp', view='Looked',
         looks_bypass=True, linear_cc=EV1),
    dict(name='override_cool_on_plain', cfg='fixture', src='lin', display='Disp', view='Standard',
         looks_override='cool'),
    dict(name='override_cool_on_looked', cfg='fixture', src='lin', display='Disp', view='Looked',
         looks_override='cool', linear_cc=EV1),
    dict(name='override_inverse_warm', cfg='fixture', src='lin', display='Disp', view='Standard',
         looks_override='-warm'),
    # OCIO's getView: case-insensitive names, not narrowed by active_views
    dict(name='view_not_active', cfg='fixture_active', src='lin', display='Disp', view='Looked'),
    dict(name='view_case_insensitive', cfg='fixture', src='lin', display='DISP', view='looked'),
    # '|' fall-back: the first option needs a file that does not exist, the second is used
    dict(name='override_fallback', cfg='fixture', src='lin', display='Disp', view='Standard',
         looks_override='gone | cool'),
    dict(name='override_beats_bypass', cfg='fixture', src='lin', display='Disp', view='Looked',
         looks_bypass=True, looks_override='warm, cool'),
    # channel view
    dict(name='cv_red', cfg='fixture', src='lin', display='Disp', view='Standard',
         channel_view={'hot': [1, 0, 0, 0]}, linear_cc=EV1),
    dict(name='cv_luma', cfg='fixture', src='logcs', display='Disp', view='Standard',
         channel_view={'hot': [0, 0, 0, 0]}, linear_cc=EV1),
    dict(name='cv_alpha_bypass', cfg='fixture', src='logcs', display='Disp', view='Looked',
         channel_view={'hot': [0, 0, 0, 1]}, linear_cc=EV1),
    dict(name='cv_alpha_no_bypass', cfg='fixture', src='logcs', display='Disp', view='Standard',
         channel_view={'hot': [0, 0, 0, 1]}, data_bypass=False),
    # Alpha entering R, G or B (matrix[3], [7], [11]) each alone switches the conversions off
    dict(name='cv_alpha_into_r', cfg='fixture', src='logcs', display='Disp', view='Looked',
         linear_cc=EV1, channel_view={'matrix': [1, 0, 0, 1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]}),
    dict(name='cv_alpha_into_g', cfg='fixture', src='logcs', display='Disp', view='Looked',
         linear_cc=EV1, channel_view={'matrix': [1, 0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 0, 0, 0, 0, 1]}),
    dict(name='cv_alpha_into_b', cfg='fixture', src='logcs', display='Disp', view='Looked',
         linear_cc=EV1, channel_view={'matrix': [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 1, 0, 0, 0, 1]}),
    # the same matrix with no alpha entry: conversions stay on
    dict(name='cv_no_alpha_entry', cfg='fixture', src='logcs', display='Disp', view='Looked',
         linear_cc=EV1, channel_view={'matrix': [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]}),
    # display_cc after the display transform
    dict(name='display_gamma', cfg='fixture', src='lin', display='Disp', view='Standard',
         display_cc={'style': 'video', 'gamma': 1.4}),
    dict(name='full_stack', cfg='fixture', src='logcs', display='Disp', view='Looked',
         linear_cc={'exposure': 0.7}, color_timing_cc={'exposure': -0.3, 'contrast': 1.1},
         channel_view={'hot': [0, 1, 0, 0]}, display_cc={'style': 'video', 'gamma': 1.2}),
    # data
    dict(name='data_input_bypass', cfg='fixture', src='raw', display='Disp', view='Standard',
         linear_cc=EV1, display_cc={'style': 'video', 'gamma': 1.3}),
    dict(name='data_input_nobypass', cfg='fixture', src='raw', display='Disp', view='Standard',
         linear_cc=EV1, data_bypass=False),
    dict(name='data_display_bypass', cfg='fixture', src='logcs', display='Disp', view='Raw',
         linear_cc=EV1),
    dict(name='data_override_looks', cfg='fixture', src='raw', display='Disp', view='Standard',
         looks_override='cool'),
    # direction
    dict(name='inverse_dir', cfg='fixture', src='lin', display='Disp', view='Standard',
         direction='inverse', linear_cc=EV1),
    dict(name='inverse_dir_looked', cfg='fixture', src='logcs', display='Disp', view='Looked',
         direction='inverse', linear_cc=EV1, display_cc={'style': 'video', 'gamma': 1.2}),
    # dynamic exposure moved on one processor
    dict(name='dyn_exposure', cfg='fixture', src='logcs', display='Disp', view='Standard',
         linear_cc={'exposure': 0.0, 'dynamic': ['exposure']}, set_exposure=[0.0, 1.0, -0.5, 2.0]),
    dict(name='dyn_exposure_aces', cfg='default', src='ACEScct', display='sRGB - Display',
         view='Video (colorimetric)', linear_cc={'exposure': 0.0, 'dynamic': ['exposure']},
         display_cc={'style': 'video', 'gamma': 1.1}, set_exposure=[0.0, 1.0, -1.0]),
    dict(name='dyn_exposure_inverse', cfg='fixture', src='lin', display='Disp', view='Standard',
         direction='inverse', linear_cc={'exposure': 0.0, 'dynamic': ['exposure'],
                                         'contrast': 1.3, 'gamma': 1.1},
         set_exposure=[0.0, 0.8, -0.6]),
]
for c in cases:
    c.update(run(CFG[c['cfg']], c))
    c['probes'] = PROBES


# --- errors: the exact text OCIO raises -------------------------------------------------------
def err(fn):
    try:
        fn()
    except Exception as e:  # noqa: BLE001 - recording whatever OCIO raises
        return str(e)
    return None


text = open(FIXTURE).read()
no_sl = O.Config.CreateFromStream(text.replace('  scene_linear: lin\n', ''))
no_ct = O.Config.CreateFromStream(text.replace('  color_timing: logcs\n', ''))
errors = {
    'no_dvt': err(lambda: O.LegacyViewingPipeline().getProcessor(fixture)),
    'no_scene_linear': err(lambda: pipeline(dict(src='lin', display='Disp', view='Standard',
                                                linear_cc=EV1)).getProcessor(no_sl)),
    'no_color_timing': err(lambda: pipeline(dict(src='lin', display='Disp', view='Standard',
                                                color_timing_cc=EV1)).getProcessor(no_ct)),
    # a no-op correction never asks for the role
    'noop_without_role': err(lambda: pipeline(dict(src='lin', display='Disp', view='Standard',
                                                  linear_cc={'exposure': 0.0})).getProcessor(no_sl)),
    'unknown_input': err(lambda: pipeline(dict(src='nope', display='Disp',
                                              view='Standard')).getProcessor(fixture)),
    'empty_input': err(lambda: pipeline(dict(src='', display='Disp',
                                            view='Standard')).getProcessor(fixture)),
    # PyOpenColorIO validates in the DisplayViewTransform constructor, so these are the bare
    # DisplayViewTransform::validate texts (DisplayViewTransform.cpp:90-100); the pipeline's own
    # validate() wraps them (LegacyViewingPipeline.cpp:166-171).
    'empty_display': err(lambda: O.DisplayViewTransform(src='lin', display='', view='Standard')),
    'empty_view': err(lambda: O.DisplayViewTransform(src='lin', display='Disp', view='')),
    'unknown_override_look': err(lambda: pipeline(dict(src='lin', display='Disp', view='Standard',
                                                      looks_override='nolook')).getProcessor(fixture)),
}

# --- ExposureContrast dynamic flags from YAML: a missing key is dynamic (OCIOYaml.cpp:1273) -----
def ec_flags(name):
    t = fixture.getColorSpace(name).getTransform(O.COLORSPACE_DIR_FROM_REFERENCE)
    return {'exposure': t.isExposureDynamic(), 'contrast': t.isContrastDynamic(),
            'gamma': t.isGammaDynamic()}


ec_yaml = {}
for name in ('ecdyn', 'ecstatic'):
    # The serialized transform line: which keys OCIO writes (dynamic values are not saved).
    line = [l for l in fixture.serialize().splitlines()
            if 'ExposureContrastTransform' in l and ('video' in l if name == 'ecdyn' else 'linear' in l)][0]
    ec_yaml[name] = {'flags': ec_flags(name), 'serialized': line.strip()}

json.dump(
    {
        'generator': 'PyOpenColorIO %s, LegacyViewingPipeline, CPU processor '
                     '(OPTIMIZATION_LOSSLESS); crates/ocio/vfx-ocio/tests/data/'
                     'gen_ocio_viewing_pipeline.py' % O.__version__,
        'cases': cases,
        'errors': errors,
        'ec_yaml': ec_yaml,
    },
    open(os.path.join(HERE, 'ocio_viewing_pipeline_pyocio.json'), 'w'),
    indent=1,
)
print(len(cases), 'cases;', {k: (v or '')[:70] for k, v in errors.items()})
