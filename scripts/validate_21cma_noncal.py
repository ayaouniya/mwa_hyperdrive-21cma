#!/usr/bin/env python3
"""Reproducible CLI checks on a bounded, disposable subset of a real 21CMA MS.

Requires numpy and python-casacore. Never modifies the input MS. Each run requires
an unused output directory; command logs and a machine-readable report are kept.
These are engineering closure tests, not an assessment of scientific image quality.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

import numpy as np
from casacore.tables import table


def read_ms(path):
    with table(str(path), ack=False) as tab:
        result = {name: tab.getcol(name) for name in
                  ('DATA', 'FLAG', 'TIME', 'ANTENNA1', 'ANTENNA2', 'UVW')}
        result['weights'] = (tab.getcol('WEIGHT_SPECTRUM') if 'WEIGHT_SPECTRUM' in tab.colnames()
                             else np.broadcast_to(tab.getcol('WEIGHT')[:, None, :], result['DATA'].shape).copy())
    with table(str(path / 'SPECTRAL_WINDOW'), ack=False) as tab:
        result['freq'] = tab.getcell('CHAN_FREQ', 0)
    with table(str(path / 'POLARIZATION'), ack=False) as tab:
        result['corr'] = tab.getcell('CORR_TYPE', 0)
    with table(str(path / 'OBSERVATION'), ack=False) as tab:
        result['telescope'] = tab.getcell('TELESCOPE_NAME', 0)
    return result


def norm(values):
    return float(np.sqrt(np.mean(np.abs(values.astype(np.complex128)) ** 2)))


def same_grid(a, b):
    for name in ('TIME', 'ANTENNA1', 'ANTENNA2', 'freq', 'corr'):
        np.testing.assert_allclose(a[name], b[name], rtol=0, atol=1e-6, err_msg=name)
    assert b['telescope'] == '21CMA'
    assert list(b['corr']) == [9], 'output must be genuine single XX'
    assert b['DATA'].shape[-1] == 1
    assert np.isfinite(b['DATA'][~b['FLAG']]).all(), 'nonfinite unflagged data'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--hyperdrive', required=True, type=Path)
    parser.add_argument('--input-ms', required=True, type=Path, help='Calibrated 21CMA MS')
    parser.add_argument('--source-list', required=True, type=Path)
    parser.add_argument('--beam-file', required=True, type=Path)
    parser.add_argument('--output-dir', required=True, type=Path)
    parser.add_argument('--timesteps', nargs='+', type=int, default=[0, 1, 2, 3, 10, 11, 12])
    parser.add_argument('--solutions', type=Path, help='Optional small 21CMA solutions FITS to convert and plot')
    parser.add_argument('--cpu', action='store_true', help='Force CPU on a CUDA-capable binary')
    args = parser.parse_args()
    binary = args.hyperdrive.resolve()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=False)
    commands = []
    environment = os.environ.copy()
    environment.setdefault('RAYON_NUM_THREADS', '4')

    def run(label, *argv):
        cmd = [str(binary), '--no-progress-bars']
        cmd += [str(arg) for arg in argv]
        if args.cpu and argv[0] in ('vis-simulate', 'vis-subtract', 'peel'):
            cmd.append('--cpu')
        start = time.monotonic()
        with (output / (label + '.log')).open('w') as log:
            result = subprocess.run(cmd, stdout=log, stderr=subprocess.STDOUT,
                                    env=environment, timeout=600, check=False)
        commands.append({'label': label, 'argv': cmd, 'exit_code': result.returncode,
                         'elapsed_seconds': time.monotonic() - start})
        (output / 'commands.json').write_text(json.dumps(commands, indent=2) + '\n')
        if result.returncode:
            raise RuntimeError(f'{label} failed; see {output / (label + ".log")}')
        print(f'{label}: passed', flush=True)

    sample = output / 'sample.ms'
    run('convert', 'vis-convert', '--data', args.input_ms.resolve(), '--telescope', '21cma',
        '--timesteps', *args.timesteps, '--outputs', sample)
    original = read_ms(sample)
    assert list(original['corr']) == [9]
    assert len(np.unique(original['TIME'])) == len(args.timesteps)
    # Read only the small TIME column from the parent to independently verify selection.
    with table(str(args.input_ms.resolve()), ack=False) as parent:
        parent_times = np.unique(parent.getcol('TIME'))
    np.testing.assert_allclose(np.unique(original['TIME']), parent_times[sorted(args.timesteps)], rtol=0, atol=1e-6)
    beam = ['--beam-type', 'cma21-feko-cube', '--beam-file', args.beam_file.resolve()]
    sky = ['--source-list', args.source_list.resolve(), '--num-sources', '3']
    route = ['--telescope', '21cma']
    model = output / 'model.ms'
    run('simulate', 'vis-simulate', '--data', sample, *route, *beam, *sky, '--output-model-files', model)
    model_data = read_ms(model)
    same_grid(original, model_data)
    sub = output / 'subtract.ms'
    run('subtract', 'vis-subtract', '--data', sample, *route, *beam, *sky, '--outputs', sub)
    sub_data = read_ms(sub)
    same_grid(original, sub_data)
    np.testing.assert_array_equal(sub_data['FLAG'], original['FLAG'])
    valid = ~original['FLAG']
    np.testing.assert_allclose(sub_data['weights'][valid], original['weights'][valid], rtol=1e-6)
    difference = sub_data['DATA'] + model_data['DATA'] - original['DATA']
    closure = norm(difference[valid]) / max(norm(original['DATA'][valid]), 1e-20)
    assert closure < 1e-5, f'data - model subtraction does not close: {closure}'

    averaging = ['--iono-time-average', '3', '--iono-freq-average', '7',
                 '--num-passes', '3', '--num-loops', '10', '--uvw-min', '0m']
    real_peel = output / 'peel_real.ms'
    run('peel_real', 'peel', '--data', sample, *route, *beam, *sky, *averaging,
        '--iono-sub', '2', '--outputs', real_peel, output / 'iono_real.json')
    peeled = read_ms(real_peel)
    same_grid(original, peeled)
    np.testing.assert_array_equal(peeled['FLAG'], original['FLAG'])
    np.testing.assert_allclose(peeled['weights'][valid], original['weights'][valid], rtol=1e-6)
    real_fits = json.loads((output / 'iono_real.json').read_text())
    expected_blocks = (len(args.timesteps) + 2) // 3
    for fit in real_fits.values():
        for key in ('alphas', 'betas', 'gains', 'centroid_timestamps_gps_seconds'):
            assert len(fit[key]) == expected_blocks
            assert np.isfinite(fit[key]).all()

    # Inject a known 10% gain error on the brightest source on the real array/time/frequency grid.
    bright = output / 'bright.ms'
    run('simulate_bright', 'vis-simulate', '--data', sample, *route, *beam,
        '--source-list', args.source_list.resolve(), '--num-sources', '1', '--output-model-files', bright)
    bright_data = read_ms(bright)
    injected = output / 'injected.ms'
    with table(str(model), ack=False) as tab:
        copied = tab.copy(str(injected), deep=True, valuecopy=True)
        copied.close()
    with table(str(injected), readonly=False, ack=False) as tab:
        tab.putcol('DATA', model_data['DATA'] + 0.1 * bright_data['DATA'])
    injected_peel = output / 'peel_injected.ms'
    run('peel_injected', 'peel', '--data', injected, *route, *beam, *sky, *averaging,
        '--iono-sub', '1', '--outputs', injected_peel, output / 'iono_injected.json')
    injected_residual = read_ms(injected_peel)
    same_grid(model_data, injected_residual)
    ratio = norm(injected_residual['DATA']) / max(norm(0.1 * bright_data['DATA']), 1e-20)
    assert ratio < 0.05, f'peel failed to recover injected gain: residual ratio {ratio}'
    fits = json.loads((output / 'iono_injected.json').read_text())
    for fit in fits.values():
        np.testing.assert_allclose(fit['gains'], 1.1, rtol=0, atol=0.005)

    preserved = output / 'peel_preserved.ms'
    run('peel_preserve', 'peel', '--data', injected, *route, *beam, *sky, *averaging,
        '--iono-sub', '1', '--preserve-non-iono-sources', '--outputs', preserved)
    restored = read_ms(preserved)
    same_grid(model_data, restored)
    preserve_error = norm(restored['DATA'] - injected_residual['DATA'] -
                          (model_data['DATA'] - bright_data['DATA']))
    assert preserve_error / max(norm(model_data['DATA']), 1e-20) < 1e-5

    averaged = output / 'averaged.ms'
    run('convert_average', 'vis-convert', '--data', sample, *route,
        '--output-vis-time-average', '3', '--output-vis-freq-average', '2', '--outputs', averaged)
    averaged_data = read_ms(averaged)
    times = np.unique(original['TIME'])
    expected_times = [np.mean(times[i:i+3]) for i in range(0, len(times), 3)]
    np.testing.assert_allclose(np.unique(averaged_data['TIME']), expected_times, rtol=0, atol=1e-6)
    assert averaged_data['DATA'].shape[1] == (original['DATA'].shape[1] + 1) // 2
    selected_sky = output / 'brightest.json'
    run('srclist_by_beam', 'srclist-by-beam', args.source_list.resolve(), selected_sky,
        '--data', sample, *route, *beam, '--number', '3')
    assert len(json.loads(selected_sky.read_text())) == 3
    run('srclist_verify', 'srclist-verify', args.source_list.resolve())
    converted_sky = output / 'sky.json'
    run('srclist_convert', 'srclist-convert', args.source_list.resolve(), converted_sky)
    run('srclist_verify_converted', 'srclist-verify', converted_sky)
    beam_output = output / 'beam.tsv'
    run('beam', 'beam', 'cma21-feko-cube', '--beam-file', args.beam_file.resolve(),
        '--freq-mhz', float(np.mean(original['freq']) / 1e6), '--latitude-deg', '42.93',
        '--step', '10', '--output', beam_output)
    beam_values = np.loadtxt(beam_output)
    assert beam_values.shape[1] == 3 and np.isfinite(beam_values).all()
    shifts = output / 'shifts.json'
    shifts.write_text(json.dumps({next(iter(real_fits)): {'ra': 0.001, 'dec': 0.0}}))
    shifted_sky = output / 'shifted.json'
    run('srclist_shift', 'srclist-shift', args.source_list.resolve(), shifts, shifted_sky)
    assert len(json.loads(shifted_sky.read_text())) == 1
    if args.solutions:
        converted_sols = output / 'solutions.bin'
        roundtrip_sols = output / 'solutions.fits'
        run('solutions_convert', 'solutions-convert', args.solutions.resolve(), converted_sols)
        run('solutions_convert_back', 'solutions-convert', converted_sols, roundtrip_sols)
        roundtrip_binary = output / 'solutions_roundtrip.bin'
        run('solutions_check_payload', 'solutions-convert', roundtrip_sols, roundtrip_binary)
        first, second = converted_sols.read_bytes(), roundtrip_binary.read_bytes()
        # AO stores dimensions at bytes 16..32 and Jones values after byte 48.
        # Its two summary timestamps cannot preserve the per-block FITS metadata.
        assert first[16:32] == second[16:32] and first[48:] == second[48:]
        run('solutions_plot', 'solutions-plot', roundtrip_sols, '--output-directory', output / 'plots', '--no-ref-tile')
        assert list((output / 'plots').glob('*.png'))
    report = {
        'binary': str(binary), 'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
        'input_ms': str(args.input_ms.resolve()), 'source_list': str(args.source_list.resolve()),
        'source_sha256': hashlib.sha256(args.source_list.read_bytes()).hexdigest(),
        'beam_file': str(args.beam_file.resolve()), 'cpu': args.cpu,
        'timesteps': args.timesteps, 'num_rows': len(original['TIME']),
        'num_channels': len(original['freq']), 'freq_range_hz': [float(original['freq'][0]), float(original['freq'][-1])],
        'subtraction_relative_closure_rms': closure,
        'injected_gain': 1.1, 'injected_peel_relative_residual_rms': ratio,
        'preserve_non_iono_absolute_closure_rms': preserve_error,
        'real_input_rms_jy': norm(original['DATA'][valid]),
        'real_subtract_rms_jy': norm(sub_data['DATA'][valid]),
        'real_peel_rms_jy': norm(peeled['DATA'][valid]),
        'iono_timeblocks': expected_blocks,
        'injected_fit': fits,
        'solutions': str(args.solutions.resolve()) if args.solutions else None,
        'commands': commands, 'passed': True,
        'scope': 'Engineering closure and metadata checks; no claim of scientific image improvement.',
    }
    (output / 'report.json').write_text(json.dumps(report, indent=2, allow_nan=False) + '\n')
    print(json.dumps({k: v for k, v in report.items() if k != 'commands'}, indent=2))


if __name__ == '__main__':
    main()
