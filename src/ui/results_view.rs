//! Generated result cards, export actions, and playback controls.

use iced::widget::{Space, button, canvas, column, container, row, slider, text};
use iced::{Alignment, Element, Length};

use super::density_canvas::DensityCanvas;
use super::message::Message;
use super::state::State;
use super::theme;

const RESULT_CARD_WIDTH: f32 = 224.0;

pub fn view(state: &State) -> Element<'_, Message> {
    let cards = if state.results.is_empty() {
        empty_results()
    } else {
        result_cards(state)
    };

    column![cards, result_toolbar(state), player_panel(state)]
        .spacing(theme::SPACE_SM)
        .into()
}

fn result_cards(state: &State) -> Element<'_, Message> {
    let mut cards = row![].align_y(Alignment::Center).spacing(theme::SPACE_SM);
    for index in 0..state.results.len() {
        cards = cards.push(result_card(state, index));
    }
    cards.wrap().into()
}

fn result_card(state: &State, index: usize) -> Element<'_, Message> {
    let selected = state.selected_result == Some(index);
    let duration = state
        .result_durations
        .get(index)
        .copied()
        .unwrap_or_default();
    let marker = if selected { "ACTIVE" } else { "READY" };
    let marker_style = if selected {
        iced::widget::text::primary
    } else {
        iced::widget::text::secondary
    };

    let preview = button(text(if selected {
        "✓  Selected"
    } else {
        "▶  Preview"
    }))
    .on_press(Message::SelectResult(index));
    let preview = if selected {
        preview.style(theme::primary_button)
    } else {
        preview.style(theme::secondary_button)
    };

    let content = column![
        row![
            text(format!("Result {}", index + 1)).size(16),
            Space::with_width(Length::Fill),
            text(marker).size(11).style(marker_style),
        ]
        .align_y(Alignment::Center)
        .spacing(theme::SPACE_SM),
        text(format!("Duration  {}", time_label(duration)))
            .size(13)
            .style(iced::widget::text::secondary),
        text(format!("Seed  {}", state.seed_used))
            .size(13)
            .style(iced::widget::text::secondary),
        preview.width(Length::Fill),
        row![
            button(text("↓  MIDI").size(12))
                .on_press(Message::SaveResultMidi(index))
                .style(theme::secondary_button),
            button(text("↓  WAV").size(12))
                .on_press(Message::SaveResultWav(index))
                .style(theme::secondary_button),
        ]
        .align_y(Alignment::Center)
        .spacing(theme::SPACE_XS),
    ]
    .spacing(theme::SPACE_SM);

    let card = container(content)
        .padding(theme::SPACE_SM)
        .width(Length::Fixed(RESULT_CARD_WIDTH));
    if selected {
        card.style(theme::selected_card).into()
    } else {
        card.style(theme::inset_card).into()
    }
}

fn empty_results<'a>() -> Element<'a, Message> {
    container(
        column![
            text("No generated tracks yet").size(16),
            text("Configure the track and run generation to create up to four results.")
                .size(13)
                .style(iced::widget::text::secondary),
        ]
        .spacing(theme::SPACE_XS),
    )
    .padding(theme::SPACE_MD)
    .width(Length::Fill)
    .style(theme::inset_card)
    .into()
}

fn result_toolbar(state: &State) -> Element<'_, Message> {
    if state.confirming_cache_clear && !state.generating {
        container(
            row![
                text("Delete all generated cache data?"),
                Space::with_width(Length::Fill),
                button(text("Cancel"))
                    .on_press(Message::CancelCacheClear)
                    .style(theme::secondary_button),
                button(text("Delete Cache"))
                    .on_press(Message::ConfirmCacheClear)
                    .style(theme::danger_button),
            ]
            .align_y(Alignment::Center)
            .spacing(theme::SPACE_SM),
        )
        .padding(theme::SPACE_SM)
        .style(theme::inset_card)
        .into()
    } else {
        row![
            button(text("↗  Save Folder"))
                .on_press(Message::OpenSaveDirectory)
                .style(theme::secondary_button),
            Space::with_width(Length::Fill),
            button(text("×  Clear Cache"))
                .on_press_maybe((!state.generating).then_some(Message::RequestCacheClear))
                .style(theme::danger_button),
        ]
        .align_y(Alignment::Center)
        .spacing(theme::SPACE_SM)
        .into()
    }
}

fn player_panel(state: &State) -> Element<'_, Message> {
    let fraction = if state.duration > 0.0 {
        (state.position / state.duration) as f32
    } else {
        0.0
    };
    let visualization = canvas(DensityCanvas {
        cache: &state.density_cache,
        density: &state.density,
        position_fraction: fraction,
    })
    .width(Length::Fill)
    .height(Length::Fixed(76.0));
    let seek = slider(0.0..=1.0, fraction, Message::Seek).step(0.001);

    let play_message = if state.playing {
        Message::Pause
    } else {
        Message::Play
    };
    let has_timeline = state.timeline.is_some();
    let controls = row![
        maybe_button(
            button(text(if state.playing {
                "Ⅱ  Pause"
            } else {
                "▶  Play"
            }))
            .style(theme::primary_button),
            has_timeline,
            play_message,
        ),
        maybe_button(
            button(text("■  Stop")).style(theme::secondary_button),
            has_timeline,
            Message::StopPlayback,
        ),
        Space::with_width(Length::Fill),
        text(format!(
            "{} / {}",
            time_label(state.position),
            time_label(state.duration)
        )),
    ]
    .align_y(Alignment::Center)
    .spacing(theme::SPACE_SM);

    container(column![visualization, seek, controls].spacing(theme::SPACE_SM))
        .padding(theme::SPACE_SM)
        .width(Length::Fill)
        .style(theme::inset_card)
        .into()
}

fn maybe_button<'a>(
    button: iced::widget::Button<'a, Message>,
    enabled: bool,
    message: Message,
) -> Element<'a, Message> {
    button.on_press_maybe(enabled.then_some(message)).into()
}

fn time_label(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}
