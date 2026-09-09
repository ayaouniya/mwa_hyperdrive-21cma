#!/usr/bin/env python3
"""Compare completed CPU/CUDA non-calibration checks, including rejected fits.

Requires NumPy and python-casacore. Normalize output differences by input RMS so
that a successfully removed injected model does not create a near-zero divisor.
"""
import argparse
import json
from pathlib import Path

import numpy as np

from validate_21cma_noncal import norm, read_ms, same_grid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('cpu', type=Path)
    parser.add_argument('cuda', type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    reports = [json.loads((path / 'report.json').read_text()) for path in (args.cpu, args.cuda)]
    assert all(report['passed'] for report in reports)
    assert reports[0]['cpu'] and not reports[1]['cpu']
    for key in ('binary_sha256', 'source_sha256', 'beam_file', 'timesteps'):
        assert reports[0][key] == reports[1][key], key
    original = read_ms(args.cpu / 'sample.ms')
    other = read_ms(args.cuda / 'sample.ms')
    same_grid(original, other)
    np.testing.assert_array_equal(original['DATA'], other['DATA'])
    scale = max(norm(original['DATA'][~original['FLAG']]), 1e-20)
    differences = {}
    for name in ('model.ms', 'subtract.ms', 'peel_real.ms', 'peel_injected.ms', 'peel_preserved.ms'):
        cpu, cuda = [read_ms(path / name) for path in (args.cpu, args.cuda)]
        same_grid(cpu, cuda)
        np.testing.assert_array_equal(cpu['FLAG'], cuda['FLAG'])
        np.testing.assert_allclose(cpu['weights'], cuda['weights'], rtol=1e-6, atol=1e-6)
        valid = ~cpu['FLAG']
        difference = norm((cpu['DATA'] - cuda['DATA'])[valid]) / scale
        assert difference < 1e-5, f'{name}: CPU/CUDA difference / input RMS = {difference}'
        differences[name] = difference
    for name in ('iono_real.json', 'iono_injected.json'):
        cpu, cuda = [json.loads((path / name).read_text()) for path in (args.cpu, args.cuda)]
        assert cpu.keys() == cuda.keys()
        for source in cpu:
            for key in ('alphas', 'betas', 'gains', 'centroid_timestamps_gps_seconds'):
                tolerance = 1e-5 if key == 'gains' else (1e-6 if key.startswith('centroid') else 1e-8)
                np.testing.assert_allclose(cpu[source][key], cuda[source][key], rtol=0, atol=tolerance,
                                           err_msg=f'{name}/{source}/{key}')
    result = {'passed': True, 'binary_sha256': reports[0]['binary_sha256'],
              'cpu': str(args.cpu.resolve()), 'cuda': str(args.cuda.resolve()),
              'output_difference_relative_to_input_rms': differences,
              'fit_parameters_agree': True}
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
