# UX specification

The window has a persistent left sidebar, routed center content, optional queue/device drawer, and
full-width bottom player. Minimum content size is 1024×680 logical pixels. Closing the window exits.

Navigation includes Home, Search, Liked Songs, Albums, Artists, playlists, create playlist, and
Settings. Search debounces by 300 ms. Long lists are virtualized. Offline views may show stale
metadata, but playback and mutations are disabled.

Themes are System, Pastel Light, and Ink Dark. Every workflow must remain keyboard operable with a
visible focus indicator and logical tab order. All visible copy is localized in en-US and pt-PT.
Queue is append-only because Spotify does not expose removal or reordering.

