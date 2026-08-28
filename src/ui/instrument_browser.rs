//! Searchable General MIDI instrument browser.

use iced::widget::{button, checkbox, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Length};

use crate::core::midi::gm::{PATCH_FAMILIES, PATCH_NAMES};

use super::message::Message;
use super::state::{AudioRequest, MAX_INSTRUMENTS, State};
use super::theme;

pub fn view(state: &State) -> Element<'_, Message> {
    let selected_count = state.selected_instrument_count();
    let search = text_input("Search instruments…", &state.instrument_query)
        .on_input(Message::InstrumentQueryInput)
        .style(theme::input)
        .width(Length::Fill);
    let clear = button(text("×  Clear"))
        .on_press_maybe(
            (!state.instrument_query.is_empty())
                .then_some(Message::InstrumentQueryInput(String::new())),
        )
        .style(theme::secondary_button);

    column![
        row![
            text(format!("Instruments ({selected_count}/{MAX_INSTRUMENTS})")).size(15),
            iced::widget::Space::with_width(Length::Fill),
            text("Empty = model choice")
                .size(12)
                .style(iced::widget::text::secondary),
        ]
        .align_y(Alignment::Center)
        .spacing(theme::SPACE_SM),
        selected_tags(state),
        row![search, clear]
            .align_y(Alignment::Center)
            .spacing(theme::SPACE_SM),
        scrollable(instrument_list(state, selected_count)).height(Length::Fixed(240.0)),
    ]
    .push_maybe(super::view::audio_notice(state))
    .spacing(theme::SPACE_SM)
    .width(Length::Fill)
    .into()
}

fn selected_tags(state: &State) -> Element<'_, Message> {
    let mut tags = row![].align_y(Alignment::Center).spacing(theme::SPACE_XS);
    let mut has_selection = false;
    for (index, selected) in state.instruments.iter().copied().enumerate() {
        if selected {
            has_selection = true;
            tags = tags.push(
                button(text(format!("{}  ×", PATCH_NAMES[index])).size(12))
                    .on_press(Message::ToggleInstrument(index))
                    .padding([theme::SPACE_XS, theme::SPACE_SM])
                    .style(theme::tag_button),
            );
        }
    }

    if has_selection {
        tags.wrap().into()
    } else {
        container(
            text("No instruments selected")
                .size(12)
                .style(iced::widget::text::secondary),
        )
        .padding(theme::SPACE_XS)
        .into()
    }
}

fn instrument_list(state: &State, selected_count: usize) -> Element<'_, Message> {
    let query = state.instrument_query.trim().to_ascii_lowercase();
    let mut list = column![].spacing(theme::SPACE_XS);
    let mut match_count = 0;

    for family in PATCH_FAMILIES {
        let family_matches = family.name.to_ascii_lowercase().contains(&query);
        let matches: Vec<usize> = (family.start..family.end)
            .filter(|index| {
                query.is_empty()
                    || family_matches
                    || PATCH_NAMES[*index].to_ascii_lowercase().contains(&query)
            })
            .collect();
        if matches.is_empty() {
            continue;
        }

        match_count += matches.len();
        list = list.push(
            text(family.name)
                .size(13)
                .style(iced::widget::text::primary),
        );
        for index in matches {
            let instrument =
                checkbox(PATCH_NAMES[index], state.instruments[index]).style(theme::check);
            let instrument = if state.instruments[index] || selected_count < MAX_INSTRUMENTS {
                instrument.on_toggle(move |_| Message::ToggleInstrument(index))
            } else {
                instrument
            };
            let preview = button(text(preview_label(state, index)).size(12))
                .padding([theme::SPACE_XS, theme::SPACE_SM])
                .width(Length::Fixed(76.0))
                .style(theme::secondary_button);
            let preview = if state.generating {
                preview
            } else {
                preview.on_press(Message::PreviewInstrument(index))
            };
            list = list.push(
                row![container(instrument).width(Length::Fill), preview]
                    .align_y(Alignment::Center)
                    .spacing(theme::SPACE_SM)
                    .width(Length::Fill),
            );
        }
    }

    if match_count == 0 {
        container(text("No matching instruments").style(iced::widget::text::secondary))
            .padding(theme::SPACE_SM)
            .into()
    } else {
        // Keep the buttons clear of the scrollbar drawn inside the viewport.
        container(list)
            .padding(iced::Padding::ZERO.right(theme::SPACE_MD))
            .into()
    }
}

fn preview_label(state: &State, index: usize) -> &'static str {
    if state.preview_patch == Some(index) {
        "■ Stop"
    } else if state.pending_audio == Some(AudioRequest::Preview(index)) {
        "…"
    } else {
        "▶ Play"
    }
}
