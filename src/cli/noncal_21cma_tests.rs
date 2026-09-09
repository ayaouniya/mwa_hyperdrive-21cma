// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! End-to-end non-calibration checks on a small single-XX observation with
//! fractional frequencies, irregular timestamps, gaps and partial final blocks.
use std::{collections::HashSet, num::NonZeroUsize, path::Path};

use approx::assert_abs_diff_eq;
use clap::Parser;
use crossbeam_channel::unbounded;
use crossbeam_utils::atomic::AtomicCell;
use hifitime::{Duration, Epoch};
use marlu::{Jones, LatLngHeight, RADec, XyzGeodetic};
use ndarray::Array2;
use serial_test::serial;
use tempfile::tempdir;
use vec1::{vec1, Vec1};

use super::{peel::PeelArgs, vis_simulate::VisSimulateArgs, vis_subtract::VisSubtractArgs};
use crate::{
    averaging::{channels_to_chanblocks, chunked_timestamps_to_timeblocks},
    context::{Polarisations, Telescope},
    io::{
        read::{MsReader, VisRead},
        write::{write_vis, VisOutputType, VisTimestep},
    },
    math::TileBaselineFlags,
};

pub(super) fn template(path: &Path) {
    let one = NonZeroUsize::new(1).unwrap();
    let times = Vec1::try_from_vec(
        [0., 3.5, 7., 10., 40., 43.5, 47.5]
            .iter()
            .map(|t| Epoch::from_gpst_seconds(1_090_008_640. + t))
            .collect(),
    )
    .unwrap();
    let freqs: Vec<_> = (0..10)
        .map(|i| 150_012_207.031_25 + i as f64 * 24_414.062_5)
        .collect();
    let spws = channels_to_chanblocks(&freqs, 24_414.062_5, one, &HashSet::new());
    let xyzs = [
        XyzGeodetic {
            x: 0.,
            y: 0.,
            z: 0.,
        },
        XyzGeodetic {
            x: 0.,
            y: 200.,
            z: 0.,
        },
        XyzGeodetic {
            x: -100.,
            y: 0.,
            z: 200.,
        },
        XyzGeodetic {
            x: -180.,
            y: 300.,
            z: 360.,
        },
    ];
    let names = ["E00".into(), "E01".into(), "N01".into(), "N02".into()];
    let pairs = [(0, 1), (0, 2), (0, 3), (1, 2), (1, 3), (2, 3)];
    let (tx, rx) = unbounded();
    for &timestamp in &times {
        tx.send(VisTimestep {
            cross_data_fb: Array2::from_elem((10, 6), Jones::identity()).into_shared(),
            cross_weights_fb: Array2::ones((10, 6)).into_shared(),
            autos: None,
            timestamp,
            output_timestamp: timestamp,
        })
        .unwrap();
    }
    drop(tx);
    write_vis(
        &vec1![(path.to_owned(), VisOutputType::MeasurementSet)],
        LatLngHeight {
            longitude_rad: 86.67_f64.to_radians(),
            latitude_rad: 42.93_f64.to_radians(),
            height_metres: 1500.,
        },
        RADec::from_degrees(0., 90.),
        None,
        &xyzs,
        &names,
        None,
        &chunked_timestamps_to_timeblocks(&times, one),
        Duration::from_seconds(3.5),
        Duration::default(),
        &spws[0],
        &pairs,
        one,
        one,
        None,
        false,
        Telescope::Cma21,
        Polarisations::XX,
        rx,
        &AtomicCell::new(false),
        None,
    )
    .unwrap();
}

struct Snapshot {
    polarisations: Polarisations,
    timestamps: Vec<Epoch>,
    fine_chan_freqs: Vec<f64>,
    tile_names: Vec<String>,
    telescope_name: String,
}

type VisibilitySnapshots = (Snapshot, Vec<Array2<Jones<f32>>>, Vec<Array2<f32>>);

fn read(path: &Path) -> VisibilitySnapshots {
    let reader = MsReader::new(path.to_owned(), None, None, None).unwrap();
    let obs = reader.get_obs_context();
    let flags = TileBaselineFlags::new(obs.get_total_num_tiles(), HashSet::new());
    let mut data = vec![];
    let mut weights = vec![];
    for t in 0..obs.timestamps.len() {
        let mut d = Array2::zeros((obs.fine_chan_freqs.len(), 6));
        let mut w = Array2::zeros(d.dim());
        reader
            .read_crosses(d.view_mut(), w.view_mut(), t, &flags, &HashSet::new())
            .unwrap();
        data.push(d);
        weights.push(w);
    }
    (
        Snapshot {
            polarisations: obs.polarisations,
            timestamps: obs.timestamps.to_vec(),
            fine_chan_freqs: obs.fine_chan_freqs.to_vec(),
            tile_names: obs.tile_names.to_vec(),
            telescope_name: marlu::rubbl_casatables::Table::open(
                path.join("OBSERVATION"),
                marlu::rubbl_casatables::TableOpenMode::Read,
            )
            .unwrap()
            .get_cell("TELESCOPE_NAME", 0)
            .unwrap(),
        },
        data,
        weights,
    )
}

#[test]
#[serial]
fn simulate_subtract_peel_21cma() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("template.ms");
    template(&input);
    let sky = dir.path().join("sky.yaml");
    std::fs::write(&sky, "bright:\n  - ra: 0.0\n    dec: 88.0\n    comp_type:\n      gaussian: {maj: 600.0, min: 300.0, pa: 30.0}\n    flux_type:\n      power_law:\n        si: -0.8\n        fd: {freq: 150000000.0, i: 10.0}\nfaint:\n  - ra: 90.0\n    dec: 87.0\n    comp_type: point\n    flux_type:\n      power_law:\n        si: -0.7\n        fd: {freq: 150000000.0, i: 1.0}\n").unwrap();
    let ranked = dir.path().join("ranked.json");
    super::srclist::SrclistByBeamArgs::parse_from([
        "srclist-by-beam",
        sky.to_str().unwrap(),
        ranked.to_str().unwrap(),
        "--data",
        input.to_str().unwrap(),
        "--telescope",
        "21cma",
        "--beam-type",
        "cma21-gaussian",
        "--number",
        "1",
    ])
    .run()
    .unwrap();
    let ranked: serde_json::Value =
        serde_json::from_slice(&std::fs::read(ranked).unwrap()).unwrap();
    assert_eq!(ranked.as_object().unwrap().len(), 1);
    assert!(ranked.get("bright").is_some());
    let model = dir.path().join("model.ms");
    VisSimulateArgs::parse_from([
        "vis-simulate",
        "--data",
        input.to_str().unwrap(),
        "--telescope",
        "21cma",
        "--source-list",
        sky.to_str().unwrap(),
        "--beam-type",
        "cma21-gaussian",
        "--output-autos",
        "--output-model-files",
        model.to_str().unwrap(),
    ])
    .run(false)
    .unwrap();
    let (obs, model_data, model_weights) = read(&model);
    let (original, _, _) = read(&input);
    assert_eq!(obs.polarisations, Polarisations::XX);
    assert_eq!(obs.telescope_name, "21CMA");
    assert_eq!(obs.timestamps, original.timestamps);
    assert_eq!(obs.fine_chan_freqs, original.fine_chan_freqs);
    assert_eq!(obs.tile_names, original.tile_names);
    assert!(model_data
        .iter()
        .flat_map(|a| a.iter())
        .any(|j| j[0].norm() > 1.));
    for j in model_data.iter().flat_map(|a| a.iter()) {
        assert_eq!(j[1].norm() + j[2].norm() + j[3].norm(), 0.);
    }
    // All-source subtraction and a fitted two-source peel must both close to zero.
    let sub = dir.path().join("subtract.ms");
    VisSubtractArgs::parse_from([
        "vis-subtract",
        "--data",
        model.to_str().unwrap(),
        "--telescope",
        "21cma",
        "--source-list",
        sky.to_str().unwrap(),
        "--beam-type",
        "cma21-gaussian",
        "--outputs",
        sub.to_str().unwrap(),
    ])
    .run(false)
    .unwrap();
    let peeled = dir.path().join("peel.ms");
    let json = dir.path().join("iono.json");
    PeelArgs::parse_from([
        "peel",
        "--data",
        model.to_str().unwrap(),
        "--telescope",
        "21cma",
        "--source-list",
        sky.to_str().unwrap(),
        "--beam-type",
        "cma21-gaussian",
        "--autos",
        "--iono-sub",
        "2",
        "--num-passes",
        "2",
        "--num-loops",
        "3",
        "--iono-time-average",
        "3",
        "--iono-freq-average",
        "6",
        "--uvw-min",
        "0m",
        "--outputs",
        peeled.to_str().unwrap(),
        json.to_str().unwrap(),
    ])
    .run(false)
    .unwrap();
    let model_reader = MsReader::new(model.clone(), None, None, None).unwrap();
    let peel_reader = MsReader::new(peeled.clone(), None, None, None).unwrap();
    assert!(peel_reader.get_obs_context().autocorrelations_present);
    let tile_flags = TileBaselineFlags::new(4, HashSet::new());
    for t in 0..obs.timestamps.len() {
        let mut expected = Array2::zeros((10, 4));
        let mut expected_weights = Array2::zeros((10, 4));
        let mut actual = expected.clone();
        let mut actual_weights = expected_weights.clone();
        model_reader
            .read_autos(
                expected.view_mut(),
                expected_weights.view_mut(),
                t,
                &tile_flags,
                &HashSet::new(),
            )
            .unwrap();
        peel_reader
            .read_autos(
                actual.view_mut(),
                actual_weights.view_mut(),
                t,
                &tile_flags,
                &HashSet::new(),
            )
            .unwrap();
        assert_eq!(expected, actual);
        assert_eq!(expected_weights, actual_weights);
    }
    for output in [sub, peeled] {
        let (out_obs, data, weights) = read(&output);
        assert_eq!(out_obs.timestamps, obs.timestamps);
        assert_eq!(out_obs.fine_chan_freqs, obs.fine_chan_freqs);
        assert_eq!(out_obs.polarisations, Polarisations::XX);
        for (got, expected) in weights.iter().zip(&model_weights) {
            assert_abs_diff_eq!(got, expected, epsilon = 1e-5);
        }
        for j in data.iter().flat_map(|a| a.iter()) {
            assert!(j.iter().all(|v| v.norm() < 1e-4), "residual {j:?}");
        }
    }
    let fits: serde_json::Value = serde_json::from_slice(&std::fs::read(json).unwrap()).unwrap();
    for source in ["bright", "faint"] {
        assert_eq!(fits[source]["gains"].as_array().unwrap().len(), 3);
        let centroids = fits[source]["centroid_timestamps_gps_seconds"]
            .as_array()
            .unwrap();
        for (got, times) in centroids.iter().zip(obs.timestamps.chunks(3)) {
            let expected =
                times.iter().map(|t| t.to_gpst_seconds()).sum::<f64>() / times.len() as f64;
            assert_abs_diff_eq!(got.as_f64().unwrap(), expected, epsilon = 1e-6);
        }
        for gain in fits[source]["gains"].as_array().unwrap() {
            assert_abs_diff_eq!(gain.as_f64().unwrap(), 1., epsilon = 1e-5);
        }
    }
}

#[test]
fn simulation_rejects_ambiguous_template_parameters() {
    for option in [
        "--time-res",
        "--num-timesteps",
        "--freq-res",
        "--ra",
        "--middle-freq",
    ] {
        let result =
            VisSimulateArgs::parse_from(["vis-simulate", "--data", "example.ms", option, "1"])
                .run(true);
        assert!(result.unwrap_err().to_string().contains("--data supplies"));
    }
    let result =
        VisSimulateArgs::parse_from(["vis-simulate", "--data", "example.uvfits"]).run(true);
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("must be a MeasurementSet"));
}

#[test]
#[serial]
fn simulation_selects_and_averages_template_times() {
    let dir = tempdir().unwrap();
    // Production 21CMA input directories also use the uppercase suffix.
    let input = dir.path().join("template.MS");
    template(&input);
    let sky = dir.path().join("sky.yaml");
    std::fs::write(&sky, "ncp:\n  - ra: 0.0\n    dec: 90.0\n    comp_type: point\n    flux_type:\n      power_law:\n        si: 0.0\n        fd: {freq: 150000000.0, i: 10.0}\n").unwrap();
    let output = dir.path().join("model.MS");
    VisSimulateArgs::parse_from([
        "vis-simulate",
        "--data",
        input.to_str().unwrap(),
        "--telescope",
        "21cma",
        "--timesteps",
        "1",
        "3",
        "6",
        "--source-list",
        sky.to_str().unwrap(),
        "--beam-type",
        "none",
        "--output-model-time-average",
        "2",
        "--output-model-freq-average",
        "2",
        "--output-model-files",
        output.to_str().unwrap(),
    ])
    .run(false)
    .unwrap();
    let (obs, data, _) = read(&output);
    assert_eq!(obs.timestamps.len(), 2);
    assert_abs_diff_eq!(
        obs.timestamps[0].to_gpst_seconds(),
        1_090_008_646.75,
        epsilon = 1e-6
    );
    assert_abs_diff_eq!(
        obs.timestamps[1].to_gpst_seconds(),
        1_090_008_687.5,
        epsilon = 1e-6
    );
    assert_eq!(obs.fine_chan_freqs.len(), 5);
    assert_abs_diff_eq!(obs.fine_chan_freqs[0], 150_024_414.062_5, epsilon = 1e-6);
    for j in data.iter().flat_map(|a| a.iter()) {
        assert_abs_diff_eq!(j[0].re, 10., epsilon = 1e-5);
        assert_abs_diff_eq!(j[0].im, 0., epsilon = 1e-5);
    }
}

#[test]
fn ms_default_time_range_keeps_data_after_flagged_gaps() {
    use marlu::rubbl_casatables::{Table, TableOpenMode};
    let dir = tempdir().unwrap();
    let input = dir.path().join("flagged.ms");
    template(&input);
    {
        let mut tab = Table::open(&input, TableOpenMode::ReadWrite).unwrap();
        for timestep in [0, 3, 6] {
            for row in timestep * 6..(timestep + 1) * 6 {
                tab.put_cell("FLAG", row, &Array2::from_elem((10, 1), true))
                    .unwrap();
            }
        }
    }
    let reader = MsReader::new(input, None, None, None).unwrap();
    assert_eq!(
        reader.get_obs_context().unflagged_timesteps,
        vec![1, 2, 3, 4, 5]
    );
}

#[test]
#[serial]
fn calibration_preserves_native_centroids_with_reader_averaging() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("calibration.ms");
    template(&input);
    let sky = dir.path().join("ncp.json");
    std::fs::write(
        &sky,
        serde_json::json!({"ncp": [{
            "ra": 0., "dec": 90., "comp_type": "point",
            "flux_type": {"power_law": {"si": 0., "fd": {"freq": 150e6, "i": 10.}}}
        }]})
        .to_string(),
    )
    .unwrap();
    let output = dir.path().join("solutions.fits");
    super::di_calibrate::DiCalArgs::parse_from([
        "di-calibrate",
        "--data",
        input.to_str().unwrap(),
        "--telescope",
        "21cma",
        "--source-list",
        sky.to_str().unwrap(),
        "--beam-type",
        "none",
        "--time-average",
        "2",
        "--freq-average",
        "2",
        "--timesteps-per-timeblock",
        "4",
        "--uvw-min",
        "0m",
        "--outputs",
        output.to_str().unwrap(),
    ])
    .run(false)
    .unwrap();
    let sols =
        crate::solutions::CalibrationSolutions::read_solutions_from_ext(&output, None::<&Path>)
            .unwrap();
    assert_eq!(sols.di_jones.dim(), (2, 4, 5));
    let averages = sols.average_timestamps.unwrap();
    assert_abs_diff_eq!(
        averages[0].to_gpst_seconds(),
        1_090_008_645.125,
        epsilon = 1e-6
    );
    assert_abs_diff_eq!(
        averages[1].to_gpst_seconds(),
        1_090_008_640. + 131. / 3.,
        epsilon = 1e-6
    );
}
