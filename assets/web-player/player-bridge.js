(() => {
  "use strict";

  const VERSION = 1;
  let accessToken = null;
  let player = null;
  let connectRequested = false;
  let nextEventId = 1;

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
      name: "Mellowdeck",
      getOAuthToken: (callback) => callback(accessToken || ""),
      volume: 0.5,
    });

    player.addListener("ready", ({ device_id: deviceId }) => {
      emit({ type: "ready", device_id: deviceId });
    });
    player.addListener("not_ready", () => emit({ type: "unavailable", reason: "not_ready" }));
    player.addListener("initialization_error", ({ message }) => {
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
      const trackUri = state.track_window?.current_track?.uri;
      emit({
        type: "state_changed",
        playing: !state.paused,
        position_ms: state.position,
        duration_ms: state.duration,
        track_uri: typeof trackUri === "string" && trackUri.startsWith("spotify:track:") ? trackUri : null,
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
          connectRequested = true;
          await initialize();
          if (player) await player.connect();
          break;
        case "disconnect":
          requirePlayer().disconnect();
          break;
        case "play":
          await requirePlayer().resume();
          break;
        case "pause":
          await requirePlayer().pause();
          break;
        case "seek":
          await requirePlayer().seek(Math.max(0, Number(payload.position_ms) || 0));
          break;
        case "volume":
          await requirePlayer().setVolume(Math.min(1, Math.max(0, Number(payload.value_milli) / 1000)));
          break;
        case "shutdown":
          if (player) player.disconnect();
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
    if (connectRequested && player) await player.connect();
  };
  window.addEventListener("message", (event) => {
    if (event.source === window) command(event.data);
  });
})();
