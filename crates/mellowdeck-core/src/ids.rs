use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum IdError {
    #[error("identifier is empty")]
    Empty,
    #[error("identifier is longer than 128 characters")]
    TooLong,
    #[error("identifier contains unsupported characters")]
    InvalidCharacters,
    #[error("unsupported Spotify URI: {0}")]
    InvalidUri(String),
}

fn validate_id(value: &str) -> Result<(), IdError> {
    if value.is_empty() {
        return Err(IdError::Empty);
    }
    if value.len() > 128 {
        return Err(IdError::TooLong);
    }
    if !value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(IdError::InvalidCharacters);
    }
    Ok(())
}

macro_rules! domain_id {
    ($name:ident) => {
        #[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Parses and validates a Spotify identifier.
            ///
            /// # Errors
            ///
            /// Returns [`IdError`] when the value is empty, too long, or contains characters
            /// outside Spotify's identifier alphabet.
            pub fn parse(value: impl Into<String>) -> Result<Self, IdError> {
                let value = value.into();
                validate_id(&value)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = IdError;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::parse(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl FromStr for $name {
            type Err = IdError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::parse(value)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

domain_id!(TrackId);
domain_id!(AlbumId);
domain_id!(ArtistId);
domain_id!(PlaylistId);
domain_id!(DeviceId);
domain_id!(UserId);

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SpotifyItemKind {
    Track,
    Album,
    Artist,
    Playlist,
}

impl SpotifyItemKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Track => "track",
            Self::Album => "album",
            Self::Artist => "artist",
            Self::Playlist => "playlist",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct SpotifyUri {
    kind: SpotifyItemKind,
    id: String,
}

impl SpotifyUri {
    /// Creates a validated Spotify URI from an item kind and identifier.
    ///
    /// # Errors
    ///
    /// Returns [`IdError`] when the identifier is empty, too long, or contains unsupported
    /// characters.
    pub fn new(kind: SpotifyItemKind, id: impl Into<String>) -> Result<Self, IdError> {
        let id = id.into();
        validate_id(&id)?;
        Ok(Self { kind, id })
    }

    pub fn kind(&self) -> SpotifyItemKind {
        self.kind
    }

    pub fn id(&self) -> &str {
        &self.id
    }
}

impl FromStr for SpotifyUri {
    type Err = IdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parts = value.split(':');
        let scheme = parts.next();
        let kind = parts.next();
        let id = parts.next();
        if scheme != Some("spotify") || id.is_none() || parts.next().is_some() {
            return Err(IdError::InvalidUri(value.to_owned()));
        }
        let kind = match kind {
            Some("track") => SpotifyItemKind::Track,
            Some("album") => SpotifyItemKind::Album,
            Some("artist") => SpotifyItemKind::Artist,
            Some("playlist") => SpotifyItemKind::Playlist,
            _ => return Err(IdError::InvalidUri(value.to_owned())),
        };
        Self::new(kind, id.unwrap_or_default())
    }
}

impl TryFrom<String> for SpotifyUri {
    type Error = IdError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<SpotifyUri> for String {
    fn from(value: SpotifyUri) -> Self {
        value.to_string()
    }
}

impl fmt::Display for SpotifyUri {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "spotify:{}:{}", self.kind.as_str(), self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_reject_untrusted_values() {
        assert_eq!(TrackId::parse(""), Err(IdError::Empty));
        assert_eq!(TrackId::parse("has spaces"), Err(IdError::InvalidCharacters));
        assert!(TrackId::parse("4uLU6hMCjMI75M1A2tKUQC").is_ok());
    }

    #[test]
    fn uri_round_trips() {
        let uri: SpotifyUri = "spotify:track:4uLU6hMCjMI75M1A2tKUQC".parse().unwrap();
        assert_eq!(uri.kind(), SpotifyItemKind::Track);
        assert_eq!(uri.to_string(), "spotify:track:4uLU6hMCjMI75M1A2tKUQC");
    }

    #[test]
    fn uri_rejects_unknown_types_and_extra_segments() {
        assert!("spotify:episode:abc".parse::<SpotifyUri>().is_err());
        assert!("spotify:track:abc:extra".parse::<SpotifyUri>().is_err());
    }
}
