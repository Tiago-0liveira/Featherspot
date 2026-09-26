(() => {
  "use strict";

  const VERSION = 1;
  let accessToken = null;
  let player = null;
  let connectRequested = false;
  let nextEventId = 1;
  let connecting = false;
  let connected = false;

  function resetConnectionState() {
    connecting = false;
    connected = false;
  }

  function emit(payload) {
    // Wry only accepts string web messages; posting an object is silently dropped.
    const message = JSON.stringify({ version: VERSION, id: nextEventId++, payload });
    if (window.ipc?.postMessage) {
      window.ipc.postMessage(message);
    } else if (window.chrome?.webview?.postMessage) {
      window.chrome.webview.postMessage(message);
    }
  }

  function cleanMessage(value) {
    return typeof value === "string" ? value.slice(0, 512) : "Unknown player error";
  }

  function requirePlayer() {
    if (!player) {
      throw new Error("player is not initialized");
    }
    return player;
  }

  async function initialize() {
    if (!window.Spotify || player) return;
    player = new window.Spotify.Player({
      name: "lspotify",
      getOAuthToken: (callback) => callback(accessToken || ""),
      volume: 0.5,
    });

    player.addListener("ready", ({ device_id: deviceId }) => {
      connecting = false;
      connected = true;
      emit({ type: "ready", device_id: deviceId });
    });
    player.addListener("not_ready", () => {
      resetConnectionState();
      emit({ type: "unavailable", reason: "not_ready" });
    });
    player.addListener("initialization_error", ({ message }) => {
      resetConnectionState();
      emit({ type: "unavailable", reason: cleanMessage(message) });
    });
    player.addListener("autoplay_failed", () => {
      emit({ type: "playback_error", message: "WebView2 blocked automatic audio playback" });
    });
    player.addListener("authentication_error", ({ message }) => {
      emit({ type: "authentication_error", message: cleanMessage(message) });
    });
    player.addListener("account_error", ({ message }) => {
      emit({ type: "account_error", message: cleanMessage(message) });
    });
    player.addListener("playback_error", ({ message }) => {
      emit({ type: "playback_error", message: cleanMessage(message) });
    });
    player.addListener("player_state_changed", (state) => {
      // A null state means playback moved to another device; the local device stays ready.
      if (!state) return;
      const currentTrack = state.track_window?.current_track;
      const trackUri = currentTrack?.uri;
      const title = currentTrack?.name;
      const artist = currentTrack?.artists?.[0]?.name;
      const album = currentTrack?.album?.name;
      const artworkUrl = currentTrack?.album?.images?.[0]?.url;
      emit({
        type: "state_changed",
        playing: !state.paused,
        position_ms: state.position,
        duration_ms: state.duration,
        track_uri: typeof trackUri === "string" && trackUri.startsWith("spotify:track:") ? trackUri : null,
        title: typeof title === "string" ? title : null,
        artist: typeof artist === "string" ? artist : null,
        album: typeof album === "string" ? album : null,
        artwork_url: typeof artworkUrl === "string" ? artworkUrl : null,
      });
    });
  }

  async function command(envelope) {
    if (!envelope || envelope.version !== VERSION || typeof envelope.payload?.type !== "string") {
      emit({ type: "playback_error", message: "Invalid command envelope" });
      return;
    }
    const payload = envelope.payload;
    try {
      switch (payload.type) {
        case "token_update":
          accessToken = typeof payload.access_token === "string" ? payload.access_token : null;
          break;
        case "connect":
          if (connecting || connected) {
            return;
          }
          connecting = true;
          connectRequested = true;
          await initialize();
          if (player) {
            try {
              const success = await player.connect();
              if (!success) {
                resetConnectionState();
              }
            } catch (err) {
              resetConnectionState();
              throw err;
            }
          }
          break;
        case "disconnect":
          connectRequested = false;
          resetConnectionState();
          if (player) {
            player.disconnect();
          }
          break;
        case "play":
          await requirePlayer().resume();
          break;
        case "pause":
          await requirePlayer().pause();
          break;
        case "previous":
          await requirePlayer().previousTrack();
          break;
        case "next":
          await requirePlayer().nextTrack();
          break;
        case "seek":
          await requirePlayer().seek(Math.max(0, Number(payload.position_ms) || 0));
          break;
        case "volume":
          await requirePlayer().setVolume(Math.min(1, Math.max(0, Number(payload.value_milli) / 1000)));
          break;
        case "shutdown":
          connectRequested = false;
          resetConnectionState();
          if (player) {
            player.disconnect();
          }
          player = null;
          accessToken = null;
          break;
        default:
          throw new Error("unknown command");
      }
    } catch (error) {
      emit({ type: "playback_error", message: cleanMessage(error?.message) });
    }
  }

  window.onSpotifyWebPlaybackSDKReady = async () => {
    await initialize();
    if (connectRequested && !connected && player) {
      connecting = true;
      try {
        const success = await player.connect();
        if (!success) {
          resetConnectionState();
        }
      } catch (err) {
        resetConnectionState();
        emit({ type: "playback_error", message: cleanMessage(err?.message) });
      }
    }
  };
  window.addEventListener("message", (event) => {
    if (event.source === window) command(event.data);
  });
})();
