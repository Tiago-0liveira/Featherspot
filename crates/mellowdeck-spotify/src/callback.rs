use std::{
    io::{Read as _, Write as _},
    net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};

use mellowdeck_core::{AppError, ErrorKind, Result};
use url::Url;

use crate::{CALLBACK_HOST, CALLBACK_PORT, OAuthAttempt};

#[derive(Debug)]
pub struct CallbackServer {
    listener: TcpListener,
    timeout: Duration,
}

impl CallbackServer {
    /// Binds the fixed Spotify callback address on the IPv4 loopback interface.
    ///
    /// # Errors
    ///
    /// Returns an error when the callback port is occupied or the listener cannot be configured.
    pub fn bind(timeout: Duration) -> Result<Self> {
        let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, CALLBACK_PORT);
        let listener = TcpListener::bind(address).map_err(|error| {
            AppError::new(
                ErrorKind::Unavailable,
                format!("OAuth callback port {CALLBACK_PORT} is unavailable: {error}"),
            )
        })?;
        listener.set_nonblocking(true).map_err(|error| {
            AppError::new(
                ErrorKind::Unavailable,
                format!("OAuth callback listener could not be configured: {error}"),
            )
        })?;
        Ok(Self { listener, timeout })
    }

    /// Waits for one matching callback, responds to the browser, and consumes the server.
    ///
    /// Requests with an invalid origin, path, or state are rejected and do not consume the valid
    /// callback opportunity. The server stops after a valid callback or timeout.
    ///
    /// # Errors
    ///
    /// Returns an error on timeout, listener failure, denied consent, or a malformed callback.
    pub fn wait_for_code(self, attempt: &OAuthAttempt) -> Result<String> {
        let deadline = Instant::now() + self.timeout;
        loop {
            match self.listener.accept() {
                Ok((mut stream, _)) => match read_callback(&mut stream) {
                    Ok(callback) => match attempt.validate_callback(&callback) {
                        Ok(code) => {
                            respond(
                                &mut stream,
                                200,
                                "Authorization complete. You may close this window.",
                            );
                            return Ok(code);
                        }
                        Err(error) => {
                            respond(&mut stream, 400, "Authorization callback was rejected.");
                            if callback
                                .query_pairs()
                                .any(|(key, value)| key == "state" && value == attempt.state)
                            {
                                return Err(error);
                            }
                        }
                    },
                    Err(error) => {
                        respond(&mut stream, 400, "Malformed authorization callback.");
                        if Instant::now() >= deadline {
                            return Err(error);
                        }
                    }
                },
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() >= deadline {
                        return Err(AppError::new(
                            ErrorKind::Authentication,
                            "Spotify authorization callback timed out",
                        ));
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => {
                    return Err(AppError::new(
                        ErrorKind::Network,
                        format!("OAuth callback listener failed: {error}"),
                    ));
                }
            }
        }
    }
}

fn read_callback(stream: &mut TcpStream) -> Result<Url> {
    stream.set_read_timeout(Some(Duration::from_secs(2))).map_err(network_error)?;
    let mut buffer = [0_u8; 8_192];
    let count = stream.read(&mut buffer).map_err(network_error)?;
    let request = std::str::from_utf8(&buffer[..count])
        .map_err(|_| AppError::new(ErrorKind::InvalidInput, "OAuth callback was not UTF-8"))?;
    let request_line = request
        .lines()
        .next()
        .ok_or_else(|| AppError::new(ErrorKind::InvalidInput, "OAuth callback was empty"))?;
    let mut parts = request_line.split_ascii_whitespace();
    let method = parts.next();
    let target = parts.next();
    let protocol = parts.next();
    if method != Some("GET") || protocol.is_none_or(|value| !value.starts_with("HTTP/")) {
        return Err(AppError::new(ErrorKind::InvalidInput, "OAuth callback request was invalid"));
    }
    let target = target.ok_or_else(|| {
        AppError::new(ErrorKind::InvalidInput, "OAuth callback target was missing")
    })?;
    Url::parse(&format!("http://{CALLBACK_HOST}:{CALLBACK_PORT}{target}"))
        .map_err(|_| AppError::new(ErrorKind::InvalidInput, "OAuth callback URL was invalid"))
}

fn respond(stream: &mut TcpStream, status: u16, message: &str) {
    let reason = if status == 200 { "OK" } else { "Bad Request" };
    let body =
        format!("<!doctype html><meta charset=utf-8><title>Mellowdeck</title><p>{message}</p>");
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
}

fn network_error(error: std::io::Error) -> AppError {
    let message = error.to_string();
    drop(error);
    AppError::new(ErrorKind::Network, message)
}

#[cfg(test)]
mod tests {
    use std::{net::TcpListener, sync::Mutex};

    use super::*;

    static CALLBACK_TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn occupied_callback_port_is_reported() {
        let _guard = CALLBACK_TEST_LOCK.lock().unwrap();
        let occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, CALLBACK_PORT)).unwrap();
        let error = CallbackServer::bind(Duration::from_millis(1)).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Unavailable);
        drop(occupied);
    }

    #[test]
    fn callback_times_out_without_a_request() {
        let _guard = CALLBACK_TEST_LOCK.lock().unwrap();
        let server = CallbackServer::bind(Duration::from_millis(1)).unwrap();
        let attempt = OAuthAttempt {
            verifier: "verifier".into(),
            challenge: "challenge".into(),
            state: "state".into(),
        };
        let error = server.wait_for_code(&attempt).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Authentication);
    }
}
