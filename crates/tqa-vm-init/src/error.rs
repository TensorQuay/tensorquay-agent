use nix::errno::Errno;

/// Guest init failures; messages are static so no configuration value, and never the token, is echoed.
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    /// The host-provided guest configuration is unusable.
    #[error("guest configuration is invalid: {0}")]
    Config(&'static str),
    /// The image configuration recorded by the rootfs pipeline is unusable.
    #[error("image configuration is invalid: {0}")]
    Image(&'static str),
    /// A system call failed.
    #[error("{step} failed: {errno}")]
    System {
        /// The setup step that failed.
        step: &'static str,
        /// The kernel's error number.
        errno: Errno,
    },
    /// A step failed without an error number (capability or filter library).
    #[error("{0} failed")]
    Step(&'static str),
}

/// Maps a nix error to the failing step.
pub fn sys(step: &'static str) -> impl FnOnce(Errno) -> InitError {
    move |errno| InitError::System { step, errno }
}

/// Maps a standard I/O error to the failing step, keeping its error number when there is one.
pub fn io(step: &'static str) -> impl FnOnce(std::io::Error) -> InitError {
    move |error| InitError::System {
        step,
        errno: error.raw_os_error().map_or(Errno::EIO, Errno::from_raw),
    }
}
