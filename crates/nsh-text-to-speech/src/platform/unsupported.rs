//! A target with no speech backend: `Speech::new` reports that speech is not available.

use crate::{backend::Backend, error::SpeechError};

/// The answer a target without any speech backend gives: no backend at all.
pub(crate) fn no_backend() -> Result<Box<dyn Backend>, SpeechError> {
    Err(SpeechError::new(crate::error::ErrorKind::NotAvailable))
}
