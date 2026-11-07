//! Image media types recognised from content, never from a file name.

/// An image format accepted as model input on vision routes.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ImageMediaType {
    /// Portable Network Graphics.
    Png,
    /// JPEG.
    Jpeg,
    /// GIF, both the 87a and 89a versions.
    Gif,
    /// WebP in a RIFF container.
    Webp,
}

/// Bytes that must be present to recognise every supported format.
pub const IMAGE_HEADER_BYTES: usize = 12;

impl ImageMediaType {
    /// Returns the media type name used in data URLs.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::Webp => "image/webp",
        }
    }

    /// Recognises a supported format from the first bytes of a file, or returns `None`.
    #[must_use]
    pub fn from_header(header: &[u8]) -> Option<Self> {
        if header.starts_with(b"\x89PNG\r\n\x1a\n") {
            Some(Self::Png)
        } else if header.starts_with(b"\xff\xd8\xff") {
            Some(Self::Jpeg)
        } else if header.starts_with(b"GIF87a") || header.starts_with(b"GIF89a") {
            Some(Self::Gif)
        } else if header.len() >= IMAGE_HEADER_BYTES
            && header.starts_with(b"RIFF")
            && &header[8..12] == b"WEBP"
        {
            Some(Self::Webp)
        } else {
            None
        }
    }
}
