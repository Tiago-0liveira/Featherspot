#![forbid(unsafe_code)]

pub mod error;
pub mod ids;
pub mod models;
pub mod ports;
pub mod state;

pub use error::{AppError, ErrorKind, Result};
pub use ids::{
    AlbumId, ArtistId, DeviceId, PlaylistId, SpotifyItemKind, SpotifyUri, TrackId, UserId,
};
pub use models::*;
pub use ports::*;
pub use state::*;
