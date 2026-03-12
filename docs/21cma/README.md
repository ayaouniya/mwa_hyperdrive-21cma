# 21CMA Route Notes

This fork contains an explicit 21CMA processing route on top of
`mwa_hyperdrive`.

The goal of the route is to make 21CMA measurement sets usable without
silently changing the legacy MWA default behaviour.

## Status

The main non-beam parts are in place and have been regression-tested:

- fractional-Hz channel centres are preserved
- non-MWA inputs no longer get fake MWA coarse-channel metadata
- 21CMA `MS` `WEIGHT` handling is fixed for 1D per-correlation layouts
- irregular 21CMA timestamps are preserved through the `MS` write path
- an explicit `--telescope 21cma` route isolates the 21CMA behaviour

The remaining clearly provisional part is the beam model.

Current 21CMA beam options:

- `none`
- `cma21-stub`
- `cma21-gaussian`

`cma21-stub` is an identity placeholder.
`cma21-gaussian` is a temporary NCP-centred Gaussian beam for development and
early imaging checks.

## Main changes relative to upstream

### Explicit telescope route

New CLI selector:

```bash
--telescope 21cma
```

This keeps the standard path unchanged unless the 21CMA route is requested
explicitly.

### Frequency handling

21CMA channel centres are not integer Hz. The 21CMA route preserves
fractional-Hz frequencies instead of rounding them away.

### Non-MWA handling

MWA-specific coarse-channel assumptions are now kept behind MWA-like handling.
21CMA and other non-MWA inputs no longer inherit fake `1.28 MHz`
coarse-channel metadata.

### Measurement-set fixes

- fixed expansion of 1D `WEIGHT` columns in 21CMA-like `MS` layouts
- improved cadence inference for irregular timestamps
- preserved native fractional channel spacing

### Irregular timestamps

The 21CMA route separates modelling time from output placement time and
preserves native `MS` timestamps on `MS` output instead of snapping them to an
ideal regular grid.

## Usage

### Basic no-beam calibration

```bash
hyperdrive di-calibrate \
  --data /path/to/21cma.ms \
  --telescope 21cma \
  --beam-type none \
  --source-list /path/to/21cma_model.yaml
```

### Provisional Gaussian beam calibration

```bash
hyperdrive di-calibrate \
  --data /path/to/21cma.ms \
  --telescope 21cma \
  --beam-type cma21-gaussian \
  --source-list /path/to/21cma_model.yaml
```

### Apply solutions

```bash
hyperdrive solutions-apply \
  --data /path/to/21cma.ms \
  --telescope 21cma \
  --solutions hyperdrive_solutions.fits \
  --outputs calibrated.ms
```

### Quick preflight

```bash
python scripts/21cma_ms_preflight.py /path/to/21cma.ms
```

## Validation summary

This branch passed:

- `cargo test --lib`
- `cargo test --test integration_tests`

Real-data validation completed on 21CMA subsets for:

- `vis-convert`
- `di-calibrate`
- `solutions-apply`
- `wsclean` imaging

Longer time-span imaging tests also showed that the earlier
stripe-dominated appearance was strongly reduced once the route was used with
better sky-model choices, longer time coverage, and proper `CLEAN`.

## Caveat

The current 21CMA beam implementation is still provisional. The non-beam route
is in much better shape than before, but final scientific work should use a
more realistic 21CMA beam model.
