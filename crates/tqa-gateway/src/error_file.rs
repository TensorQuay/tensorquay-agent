//! A refused handed-back file (ADR-0034 point 6): the file outcomes of [`GatewayError`], naming the file.
use super::GatewayError;

/// Why one handed-back file was refused: the file outcomes of [`GatewayError`], with their codes and statuses.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FileRefusal {
    /// A link, a directory or anything but a regular file (`path_invalid`).
    #[error("not a regular file inside the workspace")]
    PathInvalid,
    /// Missing, not in that report, or no longer stored (`not_found`).
    #[error("not found")]
    NotFound,
    /// Over `files.upload_bytes` (`upload_too_large`).
    #[error("larger than one file may be")]
    UploadTooLarge,
    /// Past `files.uploads_bytes` (`uploads_full`).
    #[error("no room left for uploads")]
    UploadsFull,
    /// The workspace can't be read or written now (`files_unavailable`).
    #[error("the workspace can't be reached now")]
    FilesUnavailable,
    /// Changed while it was read (`file_changed`, retryable).
    #[error("changed while it was read")]
    FileChanged,
}

impl FileRefusal {
    /// The refusal as the error it stands for.
    pub(crate) const fn error(self) -> GatewayError {
        match self {
            Self::PathInvalid => GatewayError::PathInvalid,
            Self::NotFound => GatewayError::NotFound,
            Self::UploadTooLarge => GatewayError::UploadTooLarge,
            Self::UploadsFull => GatewayError::Conflict("uploads_full"),
            Self::FilesUnavailable => GatewayError::FilesUnavailable,
            Self::FileChanged => GatewayError::FileChanged,
        }
    }

    pub(super) const fn code(self) -> &'static str {
        match self {
            Self::PathInvalid => "path_invalid",
            Self::NotFound => "not_found",
            Self::UploadTooLarge => "upload_too_large",
            Self::UploadsFull => "uploads_full",
            Self::FilesUnavailable => "files_unavailable",
            Self::FileChanged => "file_changed",
        }
    }
}

impl GatewayError {
    /// `error` about the handed-back file `path`: a file outcome names the file; any other error is left as it is.
    pub(crate) fn for_file(self, path: &str) -> Self {
        let refusal = match self {
            Self::PathInvalid => FileRefusal::PathInvalid,
            Self::NotFound => FileRefusal::NotFound,
            Self::UploadTooLarge => FileRefusal::UploadTooLarge,
            Self::Conflict("uploads_full") => FileRefusal::UploadsFull,
            Self::FilesUnavailable | Self::Conflict("sandbox_not_ready") => {
                FileRefusal::FilesUnavailable
            }
            Self::FileChanged => FileRefusal::FileChanged,
            other => return other,
        };
        Self::ForFile {
            path: path.to_owned(),
            refusal,
        }
    }
}

#[cfg(test)]
#[path = "error_file_tests.rs"]
mod tests;
