#!/usr/bin/env python3
"""Check calibration and application on a disposable real 21CMA MS subset.

Requires numpy, python-casacore and astropy. The input MS is only read. Uses a
known gain injection for numerical correctness and separately checks real-data
calibration; neither check establishes final scientific image quality.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import time

import numpy as np
from astropy.io import fits
from astropy.time import Time
from casacore.tables import table

from validate_21cma_noncal import norm, read_ms, same_grid


def copy_ms(source, destination):
    with table(str(source), ack=False) as tab:
        copied = tab.copy(str(destination), deep=True, valuecopy=True)
        copied.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--hyperdrive', required=True, type=Path)
    parser.add_argument('--input-ms', required=True, type=Path)
    parser.add_argument('--source-list', required=True, type=Path)
    parser.add_argument('--beam-file', required=True, type=Path)
    parser.add_argument('--output-dir', required=True, type=Path)
    parser.add_argument('--baseline-hyperdrive', type=Path)
    parser.add_argument('--wsclean', type=Path)
    parser.add_argument('--cpu', action='store_true')
    args = parser.parse_args()
    binary = args.hyperdrive.resolve()
    out = args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    commands = []
    env = os.environ.copy()
    env.setdefault('RAYON_NUM_THREADS', '4')

    def run(label, *argv, executable=None):
        cmd = [str(executable or binary), *map(str, argv)]
        if args.cpu and argv[0] in ('di-calibrate', 'vis-simulate'):
            cmd.append('--cpu')
        start = time.monotonic()
        with (out / (label + '.log')).open('w') as log:
            result = subprocess.run(cmd, stdout=log, stderr=subprocess.STDOUT,
                                    env=env, check=False, timeout=600)
        commands.append({'label': label, 'argv': cmd, 'exit_code': result.returncode,
                         'elapsed_seconds': time.monotonic() - start})
        (out / 'commands.json').write_text(json.dumps(commands, indent=2) + '\n')
        if result.returncode:
            raise RuntimeError(f'{label} failed; see {out / (label + ".log")}')
        print(f'{label}: passed', flush=True)

    route = ['--telescope', '21cma']
    # Extract rows using the scalar TIME column before invoking Hyperdrive.
    # A native full-day MS can have tens of millions of rows; its initial flag
    # scan is unnecessary for this bounded test and can dominate the runtime.
    subset = out / 'native_subset.MS'
    with table(str(args.input_ms.resolve()), ack=False) as tab:
        parent_times = tab.getcol('TIME')
        selected_times = np.unique(parent_times)[[0, 1, 2, 3, 10, 11, 12]]
        rows = np.flatnonzero(np.isin(parent_times, selected_times))
        selected = tab.selectrows(rows)
        copied = selected.copy(str(subset), deep=True, valuecopy=True)
        copied.close()
        selected.close()
    sample = out / 'sample.MS'
    run('select', 'vis-convert', '--data', subset, *route,
        '--use-all-timesteps', '--outputs', sample)
    observation = read_ms(sample)
    times = np.unique(observation['TIME'])
    assert len(times) == 7
    np.testing.assert_allclose(times, selected_times, rtol=0, atol=1e-6)
    with table(str(sample / 'ANTENNA'), ack=False) as tab:
        num_antennas = tab.nrows()
    with table(str(sample / 'FIELD'), ack=False) as tab:
        ra, dec = np.rad2deg(tab.getcell('PHASE_DIR', 0)[0])
    # A flat-spectrum point at the phase centre has no time/frequency smearing,
    # allowing an exact check of reader averaging and gain recovery.
    sky = out / 'phase_centre.json'
    sky.write_text(json.dumps({'test': [{'ra': float(ra), 'dec': float(dec),
        'comp_type': 'point', 'flux_type': {'power_law': {'si': 0.0,
        'fd': {'freq': 150e6, 'i': 10.0}}}}]}))
    model = out / 'model.ms'
    run('simulate', 'vis-simulate', '--data', sample, *route,
        '--source-list', sky, '--beam-type', 'none', '--output-model-files', model)
    expected = read_ms(model)
    same_grid(observation, expected)
    num_channels = len(expected['freq'])
    indices = np.searchsorted(times, expected['TIME'])
    antennas = np.arange(num_antennas)[None, :, None]
    channels = np.arange(num_channels)[None, None, :] // 2
    blocks = np.arange(2)[:, None, None]
    gains = ((1.1 + 0.08 * blocks + 0.002 * antennas + 0.0001 * channels)
             * np.exp(1j * (0.03 + 0.02 * blocks) * antennas))
    corruption = (gains[indices // 4, expected['ANTENNA1']]
                  * gains[indices // 4, expected['ANTENNA2']].conj())[:, :, None]
    injected = out / 'injected.ms'
    copy_ms(model, injected)
    with table(str(injected), readonly=False, ack=False) as tab:
        tab.putcol('DATA', expected['DATA'] * corruption)
    known_sols = out / 'known.fits'
    average = ['--time-average', '2', '--freq-average', '2',
               '--timesteps-per-timeblock', '4', '--max-iterations', '100']
    run('calibrate_injected', 'di-calibrate', '--data', injected, *route,
        '--source-list', sky, '--beam-type', 'none', *average,
        '--uvw-min', '0m', '--outputs', known_sols)
    with fits.open(known_sols) as hdus:
        solution_shape = list(hdus['SOLUTIONS'].data.shape)
        assert solution_shape == [2, num_antennas, num_channels // 2, 8]
        timeblocks = hdus['TIMEBLOCKS'].data
        gps = Time(times / 86400.0, format='mjd', scale='utc').gps
        np.testing.assert_allclose(timeblocks['Start'], gps[[0, 4]], rtol=0, atol=1e-6)
        np.testing.assert_allclose(timeblocks['End'], gps[[3, 6]], rtol=0, atol=1e-6)
        np.testing.assert_allclose(timeblocks['Average'], [gps[:4].mean(), gps[4:].mean()], rtol=0, atol=1e-6)
    corrected = out / 'corrected.ms'
    run('apply_injected', 'solutions-apply', '--data', injected, *route,
        '--solutions', known_sols, '--outputs', corrected)
    recovered = read_ms(corrected)
    same_grid(expected, recovered)
    np.testing.assert_array_equal(recovered['FLAG'], expected['FLAG'])
    gain_error = norm(recovered['DATA'] - expected['DATA']) / norm(expected['DATA'])
    assert gain_error < 1e-4, f'known gain calibration/application error {gain_error}'

    # Verify that missing AO time metadata does not silently select block zero.
    ao = out / 'three_blocks.bin'
    corrections = np.zeros((3, num_antennas, num_channels, 8), dtype='<f8')
    corrections[:, :, :, 0] = np.arange(1, 4)[:, None, None]
    corrections[:, :, :, 6] = 1.0
    ao.write_bytes(b'MWAOCAL\0' + struct.pack('<6I2d', 0, 0, 3,
                   num_antennas, num_channels, 4, 0.0, 0.0) + corrections.tobytes())
    ao_fits = out / 'three_blocks.fits'
    run('convert_ao', 'solutions-convert', ao, ao_fits)
    fraction = (expected['TIME'] - times[0]) / (times[-1] - times[0])
    block_index = np.minimum((fraction * 3).astype(int), 2)
    expected_ao = expected['DATA'] * ((block_index + 1) ** 2)[:, None, None]
    ao_errors = {}
    for extension, solution in [('bin', ao), ('fits', ao_fits)]:
        output = out / ('applied_' + extension + '.ms')
        run('apply_' + extension, 'solutions-apply', '--data', model, *route,
            '--solutions', solution, '--outputs', output)
        actual = read_ms(output)
        same_grid(expected, actual)
        error = norm(actual['DATA'] - expected_ao) / norm(expected_ao)
        assert error < 1e-6
        ao_errors[extension] = error

    # Separately check real-data calibration with the actual sky and FEKO beam.
    real_sols = out / 'real.fits'
    real_args = ['di-calibrate', '--data', sample, *route, '--source-list',
                 args.source_list.resolve(), '--num-sources', '100', '--beam-type',
                 'cma21-feko-cube', '--beam-file', args.beam_file.resolve(),
                 *average, '--uvw-min', '150lambda']
    run('calibrate_real', *real_args, '--outputs', real_sols)
    with fits.open(real_sols) as hdus:
        real_xx = hdus['SOLUTIONS'].data[..., 0:2].copy()
    finite_fraction = float(np.isfinite(real_xx).all(axis=-1).mean())
    assert finite_fraction > 0.5, f'too few finite real calibration solutions: {finite_fraction}'
    real_output = out / 'real_calibrated.ms'
    run('apply_real', 'solutions-apply', '--data', sample, *route,
        '--solutions', real_sols, '--outputs', real_output)
    actual = read_ms(real_output)
    same_grid(observation, actual)
    valid = ~actual['FLAG']
    assert np.count_nonzero(valid) > 0
    baseline_difference = None
    if args.baseline_hyperdrive:
        baseline_sols = out / 'baseline.fits'
        run('baseline_calibrate_real', *real_args, '--outputs', baseline_sols,
            executable=args.baseline_hyperdrive.resolve())
        with fits.open(baseline_sols) as hdus:
            baseline_xx = hdus['SOLUTIONS'].data[..., 0:2]
            np.testing.assert_array_equal(np.isfinite(real_xx), np.isfinite(baseline_xx))
            both = np.isfinite(real_xx) & np.isfinite(baseline_xx)
            baseline_difference = norm((real_xx - baseline_xx)[both]) / norm(real_xx[both])
    if args.wsclean:
        prefix = out / 'image'
        run('image_readback', '-quiet', '-j', '4', '-abs-mem', '1', '-size', '512', '512',
            '-scale', '2amin', '-niter', '0', '-pol', 'xx', '-data-column', 'DATA',
            '-no-update-model-required', '-name', prefix, real_output,
            executable=args.wsclean.resolve())
        images = list(out.glob('image*dirty.fits'))
        assert images and np.isfinite(fits.getdata(images[0])).all()
    report = {'passed': True, 'binary': str(binary),
              'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'cpu': args.cpu, 'input_ms': str(args.input_ms.resolve()),
              'rows': len(observation['TIME']), 'channels': num_channels,
              'solution_shape': solution_shape, 'known_gain_relative_rms_error': gain_error,
              'ao_apply_relative_rms_errors': ao_errors,
              'real_solution_finite_fraction': finite_fraction,
              'real_calibrated_unflagged_rms_jy': norm(actual['DATA'][valid]),
              'baseline_solution_relative_rms_difference': baseline_difference,
              'baseline_binary_sha256': (hashlib.sha256(args.baseline_hyperdrive.read_bytes()).hexdigest()
                                         if args.baseline_hyperdrive else None),
              'commands': commands,
              'scope': 'Gain recovery, metadata and interoperability; no image-quality claim.'}
    (out / 'report.json').write_text(json.dumps(report, indent=2, allow_nan=False) + '\n')
    print(json.dumps({k: v for k, v in report.items() if k != 'commands'}, indent=2))


if __name__ == '__main__':
    main()
