//! The platform backends. Each target picks its own in [`platform_backend`]; a target with none
//! gets `Err` and every call reports that speech is not available.

#[cfg(target_os = "android")]
mod android;
#[cfg(target_vendor = "apple")]
mod apple;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(not(any(
    target_vendor = "apple",
    target_os = "android",
    target_os = "windows",
    target_os = "linux",
    all(target_arch = "wasm32", target_os = "unknown")
)))]
mod unsupported;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod web;
#[cfg(target_os = "windows")]
mod windows;

use crate::{backend::Backend, error::SpeechError};

/// The backend of the target this build runs on, or `Err` where speech cannot run at all.
/// The web backend is for `wasm32-unknown-unknown` alone: wasip1 and emscripten get none.
pub fn platform_backend() -> Result<Box<dyn Backend>, SpeechError> {
    #[cfg(target_vendor = "apple")]
    return Ok(Box::new(apple::NativeBackend));
    #[cfg(target_os = "android")]
    return Ok(Box::new(android::NativeBackend));
    #[cfg(target_os = "windows")]
    return Ok(Box::new(windows::NativeBackend));
    #[cfg(target_os = "linux")]
    return Ok(Box::new(linux::NativeBackend));
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    return Ok(Box::new(web::NativeBackend));
    #[cfg(not(any(
        target_vendor = "apple",
        target_os = "android",
        target_os = "windows",
        target_os = "linux",
        all(target_arch = "wasm32", target_os = "unknown")
    )))]
    return unsupported::no_backend();
}

#[cfg(test)]
mod tests {
    use std::{future::Future, time::Duration};

    use super::*;
    use crate::error::ErrorKind;

    fn run<F: Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    #[cfg(target_vendor = "apple")]
    fn this_host_gets_the_apple_backend() {
        assert!(platform_backend().is_ok());
    }

    #[test]
    fn the_host_backend_settles_without_audio() {
        let backend = match platform_backend() {
            Ok(backend) => backend,
            // A target with no speech service reports exactly that, coded, and settles.
            Err(error) => {
                assert_eq!(error.kind(), ErrorKind::NotAvailable);
                return;
            }
        };
        // Queries alone: neither plays audio, and both settle within the bound.
        let bound = Duration::from_secs(30);
        let available = run(async { tokio::time::timeout(bound, backend.is_available()).await });
        assert!(available.is_ok(), "is_available settles");
        let init = run(async { tokio::time::timeout(bound, backend.init()).await });
        assert!(init.is_ok(), "init settles");
    }
}
