mod density_canvas;
mod instrument_browser;
mod message;
mod results_view;
mod state;
mod tasks;
mod theme;
mod update;
mod view;

use std::time::Duration;

use iced::{Size, Subscription, window};

use message::Message;
use state::State;

const APP_ID: &str = "io.github.ivangor246.gen_music_ai";
const APP_TITLE: &str = "Gen Music AI";
const ICON_SIZE: u32 = 128;
pub(super) const INITIAL_WINDOW_WIDTH: f32 = 1_100.0;

fn subscription(state: &State) -> Subscription<Message> {
    let playback = if state.playing || state.preview_patch.is_some() {
        iced::time::every(Duration::from_millis(50)).map(|_| Message::Tick)
    } else {
        Subscription::none()
    };
    let resize = window::resize_events().map(|(_, size)| Message::WindowResized(size.width));
    Subscription::batch([playback, resize])
}

pub fn run() -> iced::Result {
    iced::application(APP_TITLE, update::update, view::view)
        .settings(iced::Settings {
            id: Some(APP_ID.to_string()),
            antialiasing: true,
            ..iced::Settings::default()
        })
        .window(window::Settings {
            size: Size::new(INITIAL_WINDOW_WIDTH, 800.0),
            min_size: Some(Size::new(640.0, 600.0)),
            icon: window_icon(),
            ..window::Settings::default()
        })
        .centered()
        .theme(|_| theme::application())
        .subscription(subscription)
        .run_with(|| {
            let mut state = State::new();
            let boot = update::boot(&mut state);
            (state, boot)
        })
}

fn window_icon() -> Option<window::Icon> {
    window::icon::from_rgba(
        include_bytes!("../../assets/icons/app-icon.rgba").to_vec(),
        ICON_SIZE,
        ICON_SIZE,
    )
    .ok()
}
