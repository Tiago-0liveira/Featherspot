use std::sync::Arc;

use gpui_http_client::{AsyncBody, HttpClient, Response, Url, http};

pub fn spotify_image_client() -> Arc<dyn HttpClient> {
    Arc::new(SpotifyImageClient {
        client: reqwest::blocking::Client::new(),
        user_agent: http::HeaderValue::from_static(concat!(
            "Mellowdeck/",
            env!("CARGO_PKG_VERSION")
        )),
    })
}

struct SpotifyImageClient {
    client: reqwest::blocking::Client,
    user_agent: http::HeaderValue,
}

impl HttpClient for SpotifyImageClient {
    fn type_name(&self) -> &'static str {
        "MellowdeckSpotifyImageClient"
    }

    fn user_agent(&self) -> Option<&http::HeaderValue> {
        Some(&self.user_agent)
    }

    fn send(
        &self,
        request: http::Request<AsyncBody>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = gpui_http_client::Result<Response<AsyncBody>>>
                + Send
                + 'static,
        >,
    > {
        let client = self.client.clone();
        let (sender, receiver) = futures_channel::oneshot::channel();
        std::thread::spawn(move || {
            let result = send_image_request(&client, &request);
            let _ = sender.send(result);
        });
        Box::pin(async move {
            receiver.await.map_err(|_| {
                gpui_http_client::anyhow!("Spotify artwork request worker stopped unexpectedly")
            })?
        })
    }

    fn proxy(&self) -> Option<&Url> {
        None
    }
}

fn send_image_request(
    client: &reqwest::blocking::Client,
    request: &http::Request<AsyncBody>,
) -> gpui_http_client::Result<Response<AsyncBody>> {
    if request.method() != http::Method::GET {
        return Err(gpui_http_client::anyhow!("image client only supports GET"));
    }
    let url = request.uri().to_string();
    let parsed = reqwest::Url::parse(&url)?;
    let allowed = parsed.scheme() == "https"
        && parsed.host_str().is_some_and(|host| {
            host == "scdn.co"
                || host.ends_with(".scdn.co")
                || host == "spotifycdn.com"
                || host.ends_with(".spotifycdn.com")
        });
    if !allowed {
        return Err(gpui_http_client::anyhow!("image host is not allowed"));
    }
    let response = client.get(parsed).send()?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.bytes()?.to_vec();
    let mut builder = Response::builder().status(status);
    if let Some(target) = builder.headers_mut() {
        *target = headers;
    }
    Ok(builder.body(AsyncBody::from(body))?)
}
