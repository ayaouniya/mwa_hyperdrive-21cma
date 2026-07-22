# 21CMA PR Notes

## Title

`feat: add explicit 21cma measurement set route`

## Summary

This branch adds an explicit 21CMA processing route to the fork without
changing the legacy MWA default path.

It covers the main non-beam changes needed to make 21CMA measurement sets
usable in practice:

- preserve fractional-Hz channel centres instead of rounding them to integer
  Hz
- remove incorrect MWA coarse-channel assumptions from non-MWA inputs
- fix 21CMA measurement-set weight expansion for 1D per-correlation weights
- support irregular 21CMA timestamps in the read, average, and write path
- treat 21CMA data as single XX throughout reading, calibration, modelling,
  peeling, and MS/UVFITS output
- fix XX-only ionospheric fitting and the `num-passes`/`num-loops` mapping
- optionally restore sky-model sources not selected by `--iono-sub`
- add `--telescope 21cma` to isolate the 21CMA route explicitly
- add 21CMA beam plumbing with `cma21-stub`, `cma21-gaussian`, and
  `cma21-feko-cube`
- add tests and a small `MS` preflight helper

## Validation

Passed:

- `cargo test --lib` (309 tests)
- `cargo test --test integration_tests`
- CUDA single-source, XX-only single-source, and multi-source peel tests

Real-data checks completed on 21CMA `MS` subsets:

- `vis-convert`
- `di-calibrate`
- `solutions-apply`
- `wsclean` imaging

## Remaining caveat

The 21CMA beam is still provisional. The route is much more usable than the
original local patch set, but the final scientific beam model still needs a
more realistic implementation.
