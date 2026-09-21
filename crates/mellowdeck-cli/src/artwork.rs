use std::{
    collections::{HashMap, HashSet, VecDeque},
    fmt,
    sync::mpsc::{self, Receiver, SyncSender},
    thread,
    time::{Duration, Instant},
};

use image::DynamicImage;
use mellowdeck_core::CliArtworkPreference;
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
};
use ratatui_image::{
    Resize, StatefulImage,
    picker::{Picker, ProtocolType},
    thread::{ResizeRequest, ResizeResponse, ThreadProtocol},
};

const CACHE_CAPACITY: usize = 32;

struct Downloaded {
    url: String,
    image: Result<DynamicImage, String>,
}

/// Bounded artwork cache with independent download/decode and resize/encode workers.
/// Failures remain local to the image and never enter the browsing error state.
pub struct ArtworkManager {
    picker: Picker,
    mode: CliArtworkPreference,
    fetch: SyncSender<String>,
    downloaded: Receiver<Downloaded>,
    pending: HashSet<String>,
    cache: HashMap<String, DynamicImage>,
    order: VecDeque<String>,
    slots: HashMap<ArtworkPlacement, ImageSlot>,
    failed: HashMap<String, Instant>,
}

struct ImageSlot {
    resize_results: Receiver<Result<ResizeResponse, ratatui_image::errors::Errors>>,
    protocol: ThreadProtocol,
    current: Option<String>,
    has_image: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ArtworkPlacement {
    Header,
    Preview,
    Player,
}

impl fmt::Debug for ArtworkManager {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ArtworkManager")
            .field("mode", &self.mode)
            .field("cache_items", &self.cache.len())
            .field("pending", &self.pending.len())
            .finish_non_exhaustive()
    }
}

impl ArtworkManager {
    /// Must be called after entering the alternate screen and before terminal event polling.
    pub fn new(mode: CliArtworkPreference) -> Self {
        let mut mode = mode;
        let mut picker =
            Picker::from_query_stdio().unwrap_or_else(|_| Picker::from_fontsize((8, 16)));
        if mode == CliArtworkPreference::Blocks {
            picker.set_protocol_type(ProtocolType::Halfblocks);
        }
        let (fetch_tx, fetch_rx) = mpsc::sync_channel::<String>(64);
        let (download_tx, download_rx) = mpsc::sync_channel(64);
        if let Err(error) =
            thread::Builder::new().name("mellowdeck-artwork".into()).spawn(move || {
                let client = reqwest::blocking::Client::builder()
                    .user_agent(concat!("Mellowdeck/", env!("CARGO_PKG_VERSION")))
                    .connect_timeout(Duration::from_secs(5))
                    .timeout(Duration::from_secs(10))
                    .build();
                while let Ok(url) = fetch_rx.recv() {
                    let result = client
                        .as_ref()
                        .map_err(ToString::to_string)
                        .and_then(|client| {
                            client
                                .get(&url)
                                .send()
                                .and_then(reqwest::blocking::Response::error_for_status)
                                .map_err(|error| error.to_string())
                        })
                        .and_then(|response| response.bytes().map_err(|error| error.to_string()))
                        .and_then(|bytes| {
                            image::load_from_memory(&bytes).map_err(|error| error.to_string())
                        });
                    if download_tx.send(Downloaded { url, image: result }).is_err() {
                        break;
                    }
                }
            })
        {
            tracing::error!(%error, "failed to spawn artwork download thread; disabling artwork");
            mode = CliArtworkPreference::Off;
        }

        let slots = [ArtworkPlacement::Header, ArtworkPlacement::Preview, ArtworkPlacement::Player]
            .into_iter()
            .map(|placement| (placement, image_slot(placement)))
            .collect();
        Self {
            picker,
            mode,
            fetch: fetch_tx,
            downloaded: download_rx,
            pending: HashSet::new(),
            cache: HashMap::new(),
            order: VecDeque::new(),
            slots,
            failed: HashMap::new(),
        }
    }

    pub fn set_mode(&mut self, mode: CliArtworkPreference) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        if mode == CliArtworkPreference::Blocks {
            self.picker.set_protocol_type(ProtocolType::Halfblocks);
        }
        self.clear_placement();
    }

    pub fn clear_placement(&mut self) {
        for slot in self.slots.values_mut() {
            slot.protocol.empty_protocol();
            slot.current = None;
            slot.has_image = false;
        }
    }

    pub fn prefetch(&mut self, url: &str) {
        if self.mode == CliArtworkPreference::Off
            || self.cache.contains_key(url)
            || self.pending.contains(url)
        {
            return;
        }
        if self.failed.get(url).is_none_or(|retry| Instant::now() >= *retry)
            && self.fetch.try_send(url.to_owned()).is_ok()
        {
            self.pending.insert(url.to_owned());
        }
    }

    #[must_use]
    pub fn is_cached(&self, url: &str) -> bool {
        self.cache.contains_key(url)
    }

    #[must_use]
    pub fn font_size(&self) -> (u16, u16) {
        self.picker.font_size()
    }

    #[must_use]
    pub fn square_cell_size(&self, max_width: u16, max_height: u16) -> (u16, u16) {
        let (font_w, font_h) = self.picker.font_size();
        square_cell_size_with_font(max_width, max_height, font_w, font_h)
    }

    pub fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        placement: ArtworkPlacement,
        url: Option<&str>,
        placeholder: &str,
    ) {
        self.poll();
        if self.mode == CliArtworkPreference::Off || area.width < 4 || area.height < 2 {
            placeholder_widget(frame, area, placeholder);
            return;
        }
        let Some(url) = url else {
            if let Some(slot) = self.slots.get_mut(&placement) {
                slot.protocol.empty_protocol();
                slot.current = None;
                slot.has_image = false;
            }
            if placement != ArtworkPlacement::Preview {
                placeholder_widget(frame, area, placeholder);
            }
            return;
        };
        let fetch_url = {
            let Some(slot) = self.slots.get_mut(&placement) else {
                placeholder_widget(frame, area, placeholder);
                return;
            };
            if slot.current.as_deref() != Some(url) {
                slot.current = Some(url.to_owned());
                if let Some(image) = self.cache.get(url) {
                    slot.protocol.replace_protocol(self.picker.new_resize_protocol(image.clone()));
                    slot.has_image = true;
                    self.touch(url);
                    None
                } else {
                    Some(url.to_owned())
                }
            } else if slot.has_image {
                self.touch(url);
                None
            } else {
                None
            }
        };
        if let Some(fetch_url) = fetch_url
            && !self.pending.contains(&fetch_url)
            && self.failed.get(&fetch_url).is_none_or(|retry| Instant::now() >= *retry)
            && self.fetch.try_send(fetch_url.clone()).is_ok()
        {
            self.pending.insert(fetch_url);
        }
        self.poll();
        if let Some(slot) = self.slots.get_mut(&placement) {
            if slot.has_image {
                let render_area =
                    if let Some(size) = slot.protocol.size_for(Resize::Fit(None), area) {
                        let w = size.width.min(area.width);
                        let h = size.height.min(area.height);
                        let x = area.x.saturating_add((area.width.saturating_sub(w)) / 2);
                        let y = area.y.saturating_add((area.height.saturating_sub(h)) / 2);
                        Rect { x, y, width: w, height: h }
                    } else {
                        area
                    };
                frame.render_stateful_widget(
                    StatefulImage::new().resize(Resize::Fit(None)),
                    render_area,
                    &mut slot.protocol,
                );
            } else {
                placeholder_widget(frame, area, placeholder);
            }
        } else {
            placeholder_widget(frame, area, placeholder);
        }
    }

    fn touch(&mut self, url: &str) {
        if self.cache.contains_key(url) {
            self.order.retain(|u| u != url);
            self.order.push_back(url.to_owned());
        }
    }

    fn poll(&mut self) {
        while let Ok(downloaded) = self.downloaded.try_recv() {
            self.pending.remove(&downloaded.url);
            if let Ok(image) = downloaded.image {
                if !self.cache.contains_key(&downloaded.url)
                    && self.cache.len() >= CACHE_CAPACITY
                    && let Some(oldest) = self.order.pop_front()
                {
                    self.cache.remove(&oldest);
                }
                self.order.retain(|url| url != &downloaded.url);
                self.order.push_back(downloaded.url.clone());
                for slot in self
                    .slots
                    .values_mut()
                    .filter(|slot| slot.current.as_deref() == Some(downloaded.url.as_str()))
                {
                    self.failed.remove(&downloaded.url);
                    slot.protocol.replace_protocol(self.picker.new_resize_protocol(image.clone()));
                    slot.has_image = true;
                }
                self.cache.insert(downloaded.url, image);
            } else {
                self.failed
                    .insert(downloaded.url.clone(), Instant::now() + Duration::from_secs(10));
                for slot in self
                    .slots
                    .values_mut()
                    .filter(|slot| slot.current.as_deref() == Some(downloaded.url.as_str()))
                {
                    slot.protocol.empty_protocol();
                    slot.has_image = false;
                }
            }
        }
        for slot in self.slots.values_mut() {
            while let Ok(result) = slot.resize_results.try_recv() {
                if let Ok(response) = result {
                    slot.protocol.update_resized_protocol(response);
                }
            }
        }
    }
}

fn image_slot(placement: ArtworkPlacement) -> ImageSlot {
    let (resize_tx, resize_rx) = mpsc::channel::<ResizeRequest>();
    let (result_tx, result_rx) = mpsc::sync_channel(16);
    if let Err(error) =
        thread::Builder::new().name(format!("mellowdeck-artwork-{placement:?}")).spawn(move || {
            while let Ok(request) = resize_rx.recv() {
                if result_tx.send(request.resize_encode()).is_err() {
                    break;
                }
            }
        })
    {
        tracing::error!(%error, "failed to spawn artwork resize thread for {placement:?}");
    }
    ImageSlot {
        resize_results: result_rx,
        protocol: ThreadProtocol::new(resize_tx, None),
        current: None,
        has_image: false,
    }
}

fn placeholder_widget(frame: &mut Frame<'_>, area: Rect, placeholder: &str) {
    if area.width < 4 || area.height < 3 {
        return;
    }
    frame.render_widget(
        Paragraph::new(placeholder)
            .centered()
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL)),
        area,
    );
}

#[must_use]
pub fn square_cell_size_with_font(
    max_width: u16,
    max_height: u16,
    font_w: u16,
    font_h: u16,
) -> (u16, u16) {
    let font_w = u32::from(font_w.max(1));
    let font_h = u32::from(font_h.max(1));
    let avail_pixels_wide = u32::from(max_width) * font_w;
    let avail_pixels_high = u32::from(max_height) * font_h;
    let side_px = avail_pixels_wide.min(avail_pixels_high);
    if side_px == 0 {
        return (0, 0);
    }
    let cell_w = u16::try_from(side_px.div_ceil(font_w)).unwrap_or(max_width).min(max_width);
    let cell_h = u16::try_from(side_px.div_ceil(font_h)).unwrap_or(max_height).min(max_height);
    (cell_w, cell_h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefetch_skips_when_off() {
        let mut manager = ArtworkManager::new(CliArtworkPreference::Off);
        manager.prefetch("https://example.com/image.jpg");
        assert!(!manager.is_cached("https://example.com/image.jpg"));
        assert!(!manager.pending.contains("https://example.com/image.jpg"));
    }

    #[test]
    fn prefetch_marks_pending_when_active() {
        let mut manager = ArtworkManager::new(CliArtworkPreference::Auto);
        manager.prefetch("https://example.com/image.jpg");
        assert!(manager.pending.contains("https://example.com/image.jpg"));
        // Second prefetch is a no-op
        manager.prefetch("https://example.com/image.jpg");
        assert!(manager.pending.contains("https://example.com/image.jpg"));
    }

    #[test]
    fn clear_placement_resets_slot_state() {
        let mut manager = ArtworkManager::new(CliArtworkPreference::Auto);
        if let Some(slot) = manager.slots.get_mut(&ArtworkPlacement::Player) {
            slot.has_image = true;
            slot.current = Some("https://example.com/art.jpg".into());
        }
        manager.clear_placement();
        let slot = manager.slots.get(&ArtworkPlacement::Player).unwrap();
        assert!(!slot.has_image);
        assert!(slot.current.is_none());
    }

    #[test]
    fn touch_promotes_cached_url() {
        let mut manager = ArtworkManager::new(CliArtworkPreference::Auto);
        let img = image::DynamicImage::new_rgb8(1, 1);
        manager.cache.insert("https://example.com/1.jpg".into(), img.clone());
        manager.cache.insert("https://example.com/2.jpg".into(), img);
        manager.order.push_back("https://example.com/1.jpg".into());
        manager.order.push_back("https://example.com/2.jpg".into());

        manager.touch("https://example.com/1.jpg");
        assert_eq!(manager.order.back().unwrap(), "https://example.com/1.jpg");
        assert_eq!(manager.order.front().unwrap(), "https://example.com/2.jpg");
    }

    #[test]
    fn square_cell_size_preserves_square_aspect_ratio_and_bounds() {
        // Standard 1:2 cell aspect ratio (8x16 font)
        assert_eq!(square_cell_size_with_font(40, 9, 8, 16), (18, 9));
        assert_eq!(square_cell_size_with_font(40, 10, 8, 16), (20, 10));

        // When constrained by width (narrow terminal / pane)
        assert_eq!(square_cell_size_with_font(14, 10, 8, 16), (14, 7));

        // 10x20 font
        assert_eq!(square_cell_size_with_font(36, 9, 10, 20), (18, 9));

        // Zero dimensions
        assert_eq!(square_cell_size_with_font(0, 9, 8, 16), (0, 0));
        assert_eq!(square_cell_size_with_font(40, 0, 8, 16), (0, 0));
    }
}
