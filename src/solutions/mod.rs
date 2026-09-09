// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Code to read and write calibration solutions.
//!
//! See for more info:
//! <https://mwatelescope.github.io/mwa_hyperdrive/defs/cal_sols.html>

pub(crate) mod ao;
mod error;
pub(crate) mod hyperdrive;
mod rts;
#[cfg(test)]
mod tests;

pub(crate) use error::*;

use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

use hifitime::Epoch;
use itertools::Itertools;
use log::debug;
use marlu::Jones;
use ndarray::prelude::*;
use strum::IntoEnumIterator;
use strum_macros::{Display, EnumIter, EnumString};
use vec1::Vec1;

use crate::{io::read::RawDataCorrections, HyperdriveError};

lazy_static::lazy_static! {
    pub(crate) static ref CAL_SOLUTION_EXTENSIONS: String = CalSolutionType::iter().join(", ");
}

#[derive(Debug, Display, EnumIter, EnumString)]
pub(crate) enum CalSolutionType {
    /// hyperdrive's preferred format.
    #[strum(serialize = "fits")]
    Fits,

    /// The "André Offringa" format used by mwa-reduce.
    #[strum(serialize = "bin")]
    Bin,
}

#[derive(Default)]
pub struct CalibrationSolutions {
    /// The direction-independent calibration solutions. This has dimensions of
    /// (num_timeblocks, total_num_tiles, total_num_chanblocks). Note that this
    /// potentially includes flagged data; other struct members help to
    /// determine what is flagged. These Jones matrices, when applied to data
    /// Jones matrices, should approximate the model Jones matrices used in
    /// calibration.
    pub di_jones: Array3<Jones<f64>>,

    /// The indices of flagged tiles before calibration. Note that there may
    /// appear to be more flagged tiles in the solutions; this might happen if
    /// an unflagged has no power. The indices are zero indexed.
    pub flagged_tiles: Vec<usize>,

    /// Which chanblocks are flagged? Zero indexed.
    pub flagged_chanblocks: Vec<u16>,

    /// All chanblock frequencies (i.e. flagged and unflagged).
    pub chanblock_freqs: Option<Vec1<f64>>,

    /// The MWA observation ID. Allowed to be optional as not all formats
    /// provide it.
    pub obsid: Option<u32>,

    /// The start timestamps (centroids) of each timeblock used to produce these
    /// calibration solutions. It may have a different length to the first
    /// dimension of `di_jones` due to inadequate information.
    pub start_timestamps: Option<Vec1<Epoch>>,

    /// The end timestamps (centroids) of each timeblock used to produce these
    /// calibration solutions. It may have a different length to the first
    /// dimension of `di_jones` due to inadequate information.
    pub end_timestamps: Option<Vec1<Epoch>>,

    /// The average timestamps of each timeblock used to produce these
    /// calibration solutions. It may have a different length to the first
    /// dimension of `di_jones` due to inadequate information.
    pub average_timestamps: Option<Vec1<Epoch>>,

    /// The maximum allowed number of iterations during calibration.
    pub max_iterations: Option<u32>,

    /// The stop threshold used during calibration.
    pub stop_threshold: Option<f64>,

    /// The minimum threshold used during calibration.
    pub min_threshold: Option<f64>,

    /// The raw data corrections applied to the visibilities before calibration.
    pub raw_data_corrections: Option<RawDataCorrections>,

    /// The names of all of the tiles in the observation, in the order that
    /// they're presented.
    pub tile_names: Option<Vec1<String>>,

    /// Gains of each of the MWA dipoles used in calibration. The rows are tiles
    /// and there are 32 columns (one per dipole; the first 16 are X dipoles and
    /// second 16 are Y dipoles).
    pub dipole_gains: Option<ArcArray<f64, Dim<[usize; 2]>>>,

    /// Delays of each of the MWA dipoles used in calibration. The rows are
    /// tiles and there are 16 columns (one per bowtie).
    pub dipole_delays: Option<ArcArray<u32, Dim<[usize; 2]>>>,

    /// The beam file used for beam calculations.
    pub beam_file: Option<PathBuf>,

    /// The precision of the calibration for these results. The first dimension
    /// is timeblock, the second is chanblock.
    pub calibration_results: Option<Array2<f64>>,

    /// The baseline weights for all baselines (even flagged ones, which have
    /// NaN values).
    pub baseline_weights: Option<Vec1<f64>>,

    /// The minimum UVW cutoff used in calibration \[metres\].
    pub uvw_min: Option<f64>,

    /// The maximum UVW cutoff used in calibration \[metres\].
    pub uvw_max: Option<f64>,

    /// The centroid frequency of the observation used to convert UVW cutoffs
    /// specified in lambdas to metres \[Hz\].
    pub freq_centroid: Option<f64>,

    /// What was used to model the visibilities?
    pub modeller: Option<String>,
}

impl CalibrationSolutions {
    /// Read in calibration solutions from a file. The format of the file is
    /// determined by the file's extension (e.g. ".fits"). If the file is
    /// actually a directory, we attempt to read RTS DI calibration solution
    /// files from the directory.
    pub fn read_solutions_from_ext<P: AsRef<Path>, P2: AsRef<Path>>(
        file: P,
        metafits: Option<P2>,
    ) -> Result<CalibrationSolutions, HyperdriveError> {
        Self::read_solutions_from_ext_inner(file.as_ref(), metafits.as_ref().map(|f| f.as_ref()))
            .map_err(HyperdriveError::from)
    }

    pub(crate) fn read_solutions_from_ext_inner(
        file: &Path,
        metafits: Option<&Path>,
    ) -> Result<CalibrationSolutions, SolutionsReadError> {
        if file.is_dir() {
            debug!(
                "Got a directory '{}', looking for RTS solutions...",
                file.display()
            );
            let metafits = metafits.ok_or(SolutionsReadError::RtsMetafitsRequired)?;
            rts::read(file, metafits).map_err(SolutionsReadError::from)
        } else {
            match file.extension().and_then(|s| s.to_str()) {
                Some("fits") => hyperdrive::read(file),
                Some("bin") => ao::read(file),
                s => {
                    let ext = s.unwrap_or("<no extension>").to_string();
                    Err(SolutionsReadError::UnsupportedExt { ext })
                }
            }
        }
    }

    /// From the target file extension, write out the appropriately-formatted
    /// solutions.
    ///
    /// It is generally preferable to use [hyperdrive::write] for
    /// hyperdrive-style files, because that allows more metadata to be written.
    pub fn write_solutions_from_ext<P: AsRef<Path>>(&self, file: P) -> Result<(), HyperdriveError> {
        Self::write_solutions_from_ext_inner(self, file.as_ref()).map_err(HyperdriveError::from)
    }

    pub(crate) fn write_solutions_from_ext_inner(
        sols: &CalibrationSolutions,
        file: &Path,
    ) -> Result<(), SolutionsWriteError> {
        let ext = file.extension().and_then(|e| e.to_str());
        match ext.and_then(|s| CalSolutionType::from_str(s).ok()) {
            Some(CalSolutionType::Fits) => hyperdrive::write(sols, file),
            Some(CalSolutionType::Bin) => ao::write(sols, file),
            None => Err(SolutionsWriteError::UnsupportedExt {
                ext: ext.unwrap_or("<no extension>").to_string(),
            }),
        }?;

        Ok(())
    }

    /// Given a timestamp, get the timeblock of solutions that best correspond
    /// to it. If necessary, the "timestamp fraction" is used; this is a 0-to-1
    /// number that (hopefully) represents how far this timestamp is into the
    /// observation, e.g. timestep 15 is 0.75 into an observation with 20
    /// timesteps.
    pub(crate) fn get_timeblock(
        &self,
        timestamp: Epoch,
        timestamp_fraction: f64,
    ) -> ArrayView2<'_, Jones<f64>> {
        let num_timeblocks = self.di_jones.len_of(Axis(0));
        // If there's only timeblock, well...
        if num_timeblocks == 1 {
            debug!(
                "Using solutions timeblock 0 for timestamp {}",
                timestamp.to_gpst_seconds()
            );
            return self.di_jones.slice(s![0, .., ..]);
        }

        // Only per-block metadata can index the Jones time axis. AO binaries
        // may instead contain one overall start/end/average for many blocks.
        let bounds = self
            .start_timestamps
            .as_ref()
            .zip(self.end_timestamps.as_ref())
            .filter(|(s, e)| s.len() == num_timeblocks && e.len() == num_timeblocks);
        if let Some((starts, ends)) = bounds {
            if let Some(index) = starts
                .iter()
                .zip(ends.iter())
                .position(|(&start, &end)| timestamp >= start && timestamp <= end)
            {
                return self.di_jones.slice(s![index, .., ..]);
            }
        }

        // Gaps and timestamps outside the fitted interval use the nearest
        // centroid, including when start/end columns are unavailable.
        if let Some(averages) = self
            .average_timestamps
            .as_ref()
            .filter(|a| a.len() == num_timeblocks)
        {
            let index = averages
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    (**a - timestamp)
                        .to_seconds()
                        .abs()
                        .total_cmp(&(**b - timestamp).to_seconds().abs())
                })
                .expect("at least one solution timeblock")
                .0;
            return self.di_jones.slice(s![index, .., ..]);
        }

        if let Some((starts, ends)) = bounds {
            let distance = |start: Epoch, end: Epoch| {
                if timestamp < start {
                    (start - timestamp).to_seconds()
                } else {
                    (timestamp - end).to_seconds()
                }
            };
            let index = starts
                .iter()
                .zip(ends.iter())
                .enumerate()
                .min_by(|(_, (sa, ea)), (_, (sb, eb))| {
                    distance(**sa, **ea).total_cmp(&distance(**sb, **eb))
                })
                .expect("at least one solution timeblock")
                .0;
            return self.di_jones.slice(s![index, .., ..]);
        }

        // No usable per-block times: assume equal intervals over the input
        // observation. Clamp the endpoint so fraction=1 selects the last block.
        let fraction = if timestamp_fraction.is_finite() {
            timestamp_fraction.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let index = ((fraction * num_timeblocks as f64).floor() as usize).min(num_timeblocks - 1);
        debug!(
            "Using solutions timeblock {index} for timestamp {} by observation fraction; per-block times unavailable",
            timestamp.to_gpst_seconds()
        );
        self.di_jones.slice(s![index, .., ..])
    }
}
