//! Background work for Meson: the Boson zip task and the Chronon scan sweeper.

#[cfg(feature = "scan-chronon")]
mod sweeper;
mod zip;

pub(crate) use self::zip::enqueue_zip;
pub use self::zip::{MesonZipFiles, MesonZipFilesParams};
#[cfg(feature = "scan-chronon")]
pub use sweeper::meson_virus_scan_sweeper;
