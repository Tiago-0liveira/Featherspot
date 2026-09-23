# Reference-led UI and vinyl interaction plan

## Visual direction

lspotify will use the reference's calm, editorial structure without copying its branding:

- a soft neutral canvas and rounded, floating surfaces;
- a compact icon rail instead of a wide text-heavy sidebar;
- a large featured card paired with a concise top-artists list;
- artwork-led listening sections with generous spacing;
- a detached player capsule spanning the lower content area;
- lspotify's peach, lavender, sage, cream, and ink tokens throughout.

The authenticated shell is implemented in stages. Placeholder artwork remains abstract until Spotify
metadata and compliant artwork loading are connected.

## Startup and authentication (current implementation stage)

1. Load versioned settings and check Windows Credential Manager for an existing refresh token.
2. If no usable session exists, show an onboarding surface before the music shell.
3. Ask for a Spotify **Client ID**, not an API key or Client Secret. The Client ID is a public
   identifier and can be stored in settings.
4. Explain that the Spotify developer application must register exactly
   `http://127.0.0.1:43821/callback`.
5. Start Authorization Code with PKCE, open the system browser, accept one matching loopback
   callback, exchange the code, and fetch the user's profile.
6. Keep access tokens only in process memory and store only the refresh token in Windows Credential
   Manager. On later starts, refresh the session automatically.
7. Show the authenticated shell only after the session is confirmed. Premium is presented as a
   requirement for local Web Playback, not for browsing metadata.

## Vinyl scrubber interaction

The vinyl is a half-revealed disc behind the floating player capsule. It is both a playback indicator
and an optional direct-manipulation seek control.

- While playing, the disc rotates smoothly. Rotation is decorative and derived from playback state;
  it is not intended to simulate an exact physical RPM.
- Pointer-down captures the disc. Unwrapped angular movement maps to a clamped preview position, so
  dragging clockwise seeks forward and counter-clockwise seeks backward without jumping at the
  `-pi/pi` boundary.
- The preview updates locally at display cadence. Network/SDK seeks are coalesced; Connect playback
  commits on release, while the embedded player may receive a throttled preview seek where reliable.
- Releasing, cancelling, losing the target device, or receiving a newer authoritative player state
  ends the gesture and reconciles the displayed position.
- A short locally synthesized scratch cue can follow drag velocity. It is an interaction sound only:
  lspotify does not sample, record, mix, or alter Spotify audio. Interaction sounds have their own
  mute setting and stop immediately when the gesture ends.
- A conventional seek slider, keyboard seek commands, and accessible elapsed/remaining labels remain
  available. Reduced-motion mode stops idle rotation, and the vinyl never becomes the only way to seek.

## Delivery sequence

1. Persisted onboarding, PKCE login, refresh-token restoration, and signed-in identity.
2. Reference-led shell, responsive rail, cards, player capsule, and static vinyl visual.
3. Real Home/search/library data with loading, error, empty, cache, and offline states.
4. Connect playback controls and the local Web Playback feasibility adapter.
5. Vinyl gesture math, seek command coalescing, local scratch cue, reduced-motion behavior, and
   keyboard/accessibility coverage.

Acceptance tests for the vinyl cover angle wrapping, clamping, gesture cancellation, authoritative
state reconciliation, reduced motion, and no more than one committed Connect seek per completed drag.
