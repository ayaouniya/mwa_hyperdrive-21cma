// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use std::{fs::File, io::BufReader, path::PathBuf};

use approx::assert_abs_diff_eq;
use clap::Parser;

use super::SrclistByBeamArgs;
use crate::{
    cli::common::BeamArgs,
    srclist::{hyperdrive::source_list_from_json, read::read_source_list_file},
};

#[test]
fn test_srclist_by_beam() {
    let sl_path = PathBuf::from(
        "test_files/1090008640/srclist_pumav3_EoR0aegean_EoR1pietro+ForA_1090008640_peel100.txt",
    );
    let (sl, _) = read_source_list_file(&sl_path, None).unwrap();
    let n = 5;

    let temp = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
    SrclistByBeamArgs {
        input_source_list: sl_path,
        output_source_list: Some(temp.path().to_path_buf()),
        input_type: None,
        output_type: None,
        metafits: Some(PathBuf::from("test_files/1090008640/1090008640.metafits")),
        data: None,
        telescope: None,
        array_position: None,
        lst_rad: None,
        phase_centre: None,
        freqs_hz: None,
        number: n,
        source_dist_cutoff: None,
        veto_threshold: None,
        elevation_limit: None,
        filter_points: false,
        filter_gaussians: false,
        filter_shapelets: false,
        collapse_into_single_source: false,
        rts_base_source: None,
        beam_args: BeamArgs {
            beam_type: Some("fee".to_string()),
            no_beam: false,
            delays: None,
            unity_dipole_gains: false,
            beam_file: None,
        },
    }
    .run()
    .unwrap();

    let f = File::open(temp.path()).unwrap();
    let mut f = BufReader::new(f);
    let new_sl = source_list_from_json(&mut f).unwrap();
    assert_eq!(new_sl.len(), n);
    for i in 0..n {
        assert_abs_diff_eq!(sl[i], new_sl[i]);
    }
}

#[test]
fn collapse_keeps_brightness_order_after_removing_base() {
    let dir = tempfile::tempdir().unwrap();
    let sky = dir.path().join("sky.json");
    let output = dir.path().join("collapsed.json");
    let mut sources = serde_json::Map::new();
    for (name, ra, flux) in [("bright", 0., 10.), ("middle", 10., 5.), ("faint", 20., 1.)] {
        sources.insert(
            name.into(),
            serde_json::json!([{
                "ra": ra, "dec": 88., "comp_type": "point",
                "flux_type": {"power_law": {"si": 0., "fd": {"freq": 150e6, "i": flux}}}
            }]),
        );
    }
    std::fs::write(&sky, serde_json::to_vec(&sources).unwrap()).unwrap();
    SrclistByBeamArgs::parse_from([
        "srclist-by-beam",
        sky.to_str().unwrap(),
        output.to_str().unwrap(),
        "--lst",
        "0",
        "--phase-centre",
        "0",
        "90",
        "--freqs",
        "150000000",
        "--array-position",
        "86.7",
        "42.9",
        "2500",
        "--beam-type",
        "none",
        "--collapse-into-single-source",
        "--number",
        "1",
    ])
    .run()
    .unwrap();
    let (collapsed, _) = read_source_list_file(&output, None).unwrap();
    let components = &collapsed["bright"].components;
    assert_eq!(components.len(), 2);
    assert_abs_diff_eq!(components[1].radec.ra.to_degrees(), 10., epsilon = 1e-10);
}
