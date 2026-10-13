//! Who runs the workspace's data-parallel CPU work: [`Runner`].
//!
//! # Why a trait
//!
//! The CPU paths (this crate's [`crate::resample`], vfx-compute's CPU backend, vfx-stmap's map
//! generation) split their work into independent jobs - usually bands of rows. Applications and
//! the CLI run those jobs on rayon's global pool ([`Rayon`], the default). An OpenFX plug-in must
//! not: a plug-in that starts its own threads from a host thread (a rayon pool is started lazily
//! on first use) leaves thread-locals and threads behind in the plug-in image, which the host may
//! unload, and competes with the host's own scheduling. OpenFX gives plug-ins the host's
//! MultiThread suite for this; ofx-rs implements [`Runner`] over it and hands it to these crates.
//! So the work is described once, as jobs, and whoever embeds the crate decides who runs them.
//!
//! # Contract
//!
//! [`Runner::run`] calls `job(i)` once for every `i` in `0..jobs`, in any order and possibly
//! concurrently, and returns after the last call returned. Every caller in this workspace makes
//! its jobs write disjoint output, so results are identical for every runner and thread count
//! (each crate pins that with a test).
//!
//! # Features
//!
//! - `rayon` (default): [`Rayon`] exists and is [`default_runner`]. Without it the default is
//!   [`Serial`] and rayon is not in the dependency graph at all - what an OpenFX plug-in builds.

use std::fmt;
use std::sync::{Mutex, PoisonError};

/// A runner could not run its jobs (an OpenFX host's MultiThread suite refused, a worker died).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerError(pub String);

impl fmt::Display for RunnerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "parallel jobs failed: {}", self.0)
    }
}

impl std::error::Error for RunnerError {}

/// Runs the independent jobs of one operation (module docs).
pub trait Runner: Send + Sync {
    /// Call `job(i)` once for every `i` in `0..jobs`, possibly concurrently, and return after the
    /// last call returned.
    ///
    /// # Errors
    ///
    /// [`RunnerError`] when the jobs could not be run; the caller's output is then incomplete.
    fn run(&self, jobs: u32, job: &(dyn Fn(u32) + Sync)) -> Result<(), RunnerError>;
}

/// Every job on the calling thread, in order: the default without the `rayon` feature, and the
/// reference the parallel runners are tested against.
#[derive(Debug, Clone, Copy, Default)]
pub struct Serial;

impl Runner for Serial {
    fn run(&self, jobs: u32, job: &(dyn Fn(u32) + Sync)) -> Result<(), RunnerError> {
        (0..jobs).for_each(job);
        Ok(())
    }
}

/// The jobs on rayon's global pool: the default for applications and the CLI.
#[cfg(feature = "rayon")]
#[derive(Debug, Clone, Copy, Default)]
pub struct Rayon;

#[cfg(feature = "rayon")]
impl Runner for Rayon {
    fn run(&self, jobs: u32, job: &(dyn Fn(u32) + Sync)) -> Result<(), RunnerError> {
        use rayon::prelude::*;
        (0..jobs).into_par_iter().for_each(job);
        Ok(())
    }
}

/// The runner a function uses when its caller names none: [`Rayon`] with the `rayon` feature,
/// [`Serial`] without.
#[must_use]
pub fn default_runner() -> &'static dyn Runner {
    #[cfg(feature = "rayon")]
    {
        &Rayon
    }
    #[cfg(not(feature = "rayon"))]
    {
        &Serial
    }
}

/// Run `f(row_index, row)` for every `row_len`-long row of `data` through `runner`, one job per
/// row (as the rayon loops it replaces did: `par_chunks_mut(row_len)`). Rows are disjoint, so the
/// result does not depend on the runner. A trailing partial row is passed as is.
///
/// # Errors
///
/// The runner's [`RunnerError`].
pub fn rows<T: Send>(
    data: &mut [T],
    row_len: usize,
    runner: &dyn Runner,
    f: &(dyn Fn(usize, &mut [T]) + Sync),
) -> Result<(), RunnerError> {
    if row_len == 0 || data.is_empty() {
        return Ok(());
    }
    // A Mutex per row hands each job its row through the shared `Fn`; every row is locked once.
    let rows: Vec<Mutex<&mut [T]>> = data.chunks_mut(row_len).map(Mutex::new).collect();
    let jobs = u32::try_from(rows.len())
        .map_err(|_| RunnerError(format!("{} rows exceed the job count", rows.len())))?;
    runner.run(jobs, &|j| {
        let i = j as usize;
        if let Some(row) = rows.get(i) {
            let mut row = row.lock().unwrap_or_else(PoisonError::into_inner);
            f(i, &mut row);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Jobs in reverse order: no job may rely on another having run.
    struct Reverse;

    impl Runner for Reverse {
        fn run(&self, jobs: u32, job: &(dyn Fn(u32) + Sync)) -> Result<(), RunnerError> {
            (0..jobs).rev().for_each(job);
            Ok(())
        }
    }

    #[test]
    fn rows_visits_every_row_once_whatever_the_runner() {
        let fill = |r: &dyn Runner| {
            let mut data = vec![0u32; 7 * 5 + 3];
            rows(&mut data, 7, r, &|i, row| {
                for (x, v) in row.iter_mut().enumerate() {
                    *v += (i * 100 + x) as u32 + 1;
                }
            })
            .unwrap();
            data
        };
        let serial = fill(&Serial);
        assert_eq!(serial[0], 1);
        assert_eq!(serial[7 * 5 + 2], 503, "the partial last row");
        assert_eq!(serial, fill(&Reverse));
        assert_eq!(serial, fill(default_runner()));
    }
}
