//! Gamut boundary detection (`cmssm.c`).
//!
//! Ports lcms2's gamut-boundary descriptor built with Jan Morovic's segment-maxima
//! method. See [`gbd`] for the algorithm; [`Gbd`] is the public handle
//! (`cmsGBDAlloc`/`cmsGDBAddPoint`/`cmsGDBCheckPoint`/`cmsGDBCompute`).

pub mod gbd;

pub use gbd::Gbd;
