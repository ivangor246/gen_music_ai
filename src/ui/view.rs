//! The application view and its reusable panel helpers.

use iced::widget::{
    Space, button, checkbox, column, container, pick_list, progress_bar, row, scrollable, slider,
    text, text_input,
};
use iced::{Alignment, Element, Length};

use crate::services::generation::KEY_SIGNATURES;
use crate::services::model_store::LocalModelState;
use crate::settings::AUTO_VALUE;

use super::instrument_browser;
use super::message::{FormMsg, Message};
use super::results_view;
use super::state::{CONTEXT_WINDOWS, DRUM_KITS, ModelState, State, TIME_SIGNATURES};
use super::theme;

const WIDE_LAYOUT_THRESHOLD: f32 = 960.0;
const WIDE_HEADER_THRESHOLD: f32 = 1_080.0;
const COMBO_WIDTH: f32 = 175.0;

pub fn view(state: &State) -> Element<'_, Message> {
    let wide = state.viewport_width >= WIDE_LAYOUT_THRESHOLD;
    let wide_header = state.viewport_width >= WIDE_HEADER_THRESHOLD;
    let header: Element<'_, Message> = if wide_header {
        row![
            container(model_panel(state)).width(Length::FillPortion(1)),
            container(presets_panel(state, wide_header)).width(Length::FillPortion(1)),
        ]
        .align_y(Alignment::Center)
        .height(Length::Shrink)
        .spacing(theme::SPACE_MD)
        .into()
    } else {
        column![model_panel(state), presets_panel(state, wide_header)]
            .spacing(theme::SPACE_MD)
            .into()
    };
    let content = column![
        header,
        input_panel(state, wide),
        params_panel(state, wide),
        results_panel(state),
    ]
    .spacing(theme::SPACE_MD)
    .padding(theme::SPACE_MD)
    .width(Length::Fill);

    scrollable(content).width(Length::Fill).into()
}

fn section<'a>(title: &'a str, body: Element<'a, Message>) -> Element<'a, Message> {
    sized_section(title, body, Length::Shrink)
}

/// Decoding the SoundFont takes seconds, so every panel that starts audio says
/// so while it runs: a quiet Play button otherwise reads as a broken one.
pub(super) fn audio_notice(state: &State) -> Option<Element<'_, Message>> {
    state.player_loading.then(|| {
        text("Preparing audio…")
            .size(12)
            .style(iced::widget::text::secondary)
            .into()
    })
}

fn sized_section<'a>(
    title: &'a str,
    body: Element<'a, Message>,
    height: Length,
) -> Element<'a, Message> {
    container(column![text(title).size(20), body].spacing(theme::SPACE_SM))
        .padding(theme::SPACE_MD)
        .width(Length::Fill)
        .height(height)
        .style(theme::card)
        .into()
}

fn model_panel(state: &State) -> Element<'_, Message> {
    let selected = state.selected_model();
    let names: Vec<String> = state
        .model_catalog
        .iter()
        .map(|model| model.name.clone())
        .collect();
    let picker = pick_list(
        names,
        selected.map(|model| model.name.clone()),
        Message::SelectModel,
    )
    .placeholder("No supported models")
    .style(theme::selection)
    .width(Length::Fill);

    let local_state = state.selected_model_state();
    let (status_text, status_kind) = model_status(state, local_state);
    let status = match status_kind {
        ModelStatusKind::Normal => text(status_text)
            .size(13)
            .style(iced::widget::text::secondary),
        ModelStatusKind::Working => text(status_text)
            .size(13)
            .style(iced::widget::text::primary),
        ModelStatusKind::Ready => text(status_text)
            .size(13)
            .style(iced::widget::text::success),
        ModelStatusKind::Error => text(status_text).size(13).style(iced::widget::text::danger),
    }
    .width(Length::Fill);

    let action: Element<'_, Message> = match &state.model {
        ModelState::Downloading { .. } => button(text("Pause Download"))
            .on_press(Message::CancelModelDownload)
            .style(theme::secondary_button)
            .into(),
        ModelState::Cancelling => button(text("Pausing…"))
            .style(theme::secondary_button)
            .into(),
        ModelState::Loading => button(text("Loading…")).style(theme::primary_button).into(),
        ModelState::Removing => button(text("Removing…")).style(theme::danger_button).into(),
        _ if state.selected_model_is_active() => button(text("✓  Loaded"))
            .style(theme::primary_button)
            .into(),
        _ => {
            let label = match local_state {
                LocalModelState::NotInstalled => "↓  Download & Load",
                LocalModelState::Partial => "↓  Resume & Load",
                LocalModelState::Installed => "↓  Load Model",
            };
            let action = button(text(label)).style(theme::primary_button);
            if selected.is_some() && !state.generating {
                action.on_press(Message::LoadModel).into()
            } else {
                action.into()
            }
        }
    };

    let remove = button(text("×  Remove")).style(theme::danger_button);
    let remove: Element<'_, Message> = if local_state != LocalModelState::NotInstalled
        && !state.generating
        && !state.model.is_busy()
    {
        remove.on_press(Message::RequestModelRemoval).into()
    } else {
        remove.into()
    };

    let controls: Element<'_, Message> =
        if state.confirming_model_remove && !state.generating && !state.model.is_busy() {
            column![
                text("Remove the selected model from this device?").size(13),
                row![
                    button(text("Cancel"))
                        .on_press(Message::CancelModelRemoval)
                        .style(theme::secondary_button),
                    button(text("Remove Model"))
                        .on_press(Message::ConfirmModelRemoval)
                        .style(theme::danger_button),
                ]
                .spacing(theme::SPACE_SM)
                .wrap(),
            ]
            .spacing(theme::SPACE_SM)
            .into()
        } else {
            row![action, remove].spacing(theme::SPACE_SM).wrap().into()
        };

    // Switching precision rebuilds the weights, so it has to wait for any load
    // or generation already in flight.
    let precision = checkbox("Half Precision (f16)", state.app_settings.half_precision())
        .size(16)
        .text_size(13);
    let precision = if state.generating || state.model.is_busy() {
        precision
    } else {
        precision.on_toggle(Message::ToggleHalfPrecision)
    };

    let mut body = column![
        labeled("Available Model", picker),
        selected
            .map(|model| {
                text(format!(
                    "{} · {} · {}",
                    format_bytes(model.download_size()),
                    model.license,
                    model.description
                ))
                .size(12)
                .width(Length::Fill)
                .style(iced::widget::text::secondary)
            })
            .unwrap_or_else(|| text("The built-in catalog could not be loaded.").size(12)),
        status,
    ]
    .spacing(theme::SPACE_SM);

    if let ModelState::Downloading { downloaded, total } = &state.model {
        let fraction = if *total == 0 {
            0.0
        } else {
            *downloaded as f32 / *total as f32
        };
        body = body.push(
            progress_bar(0.0..=1.0, fraction.clamp(0.0, 1.0))
                .style(theme::progress)
                .width(Length::Fill),
        );
    }
    if let Some(active) = &state.active_model
        && active.id != state.selected_model_id
    {
        let name = state
            .model_catalog
            .iter()
            .find(|model| model.id == active.id)
            .map_or(active.id.as_str(), |model| model.name.as_str());
        body = body.push(
            text(format!(
                "Currently loaded: {name}. Load the selection to switch."
            ))
            .size(12)
            .width(Length::Fill)
            .style(iced::widget::text::secondary),
        );
    }
    body = body.push(controls).push(precision).push(
        text("Half precision halves model memory; speed gain depends on the CPU.")
            .size(12)
            .width(Length::Fill)
            .style(iced::widget::text::secondary),
    );

    sized_section("Model", body.into(), Length::Shrink)
}

enum ModelStatusKind {
    Normal,
    Working,
    Ready,
    Error,
}

fn model_status(state: &State, local: LocalModelState) -> (String, ModelStatusKind) {
    match &state.model {
        ModelState::Downloading { downloaded, total } if downloaded >= total => (
            "Verifying the downloaded model…".to_string(),
            ModelStatusKind::Working,
        ),
        ModelState::Downloading { downloaded, total } => (
            format!(
                "Downloading: {} / {}",
                format_bytes(*downloaded),
                format_bytes(*total)
            ),
            ModelStatusKind::Working,
        ),
        ModelState::Cancelling => (
            "Pausing the download…".to_string(),
            ModelStatusKind::Working,
        ),
        ModelState::Loading => ("Loading into memory…".to_string(), ModelStatusKind::Working),
        ModelState::Removing => (
            "Removing local model files…".to_string(),
            ModelStatusKind::Working,
        ),
        ModelState::Failed(error) => (format!("Error: {error}"), ModelStatusKind::Error),
        ModelState::Idle if state.selected_model_is_active() => {
            ("Loaded: CPU inference".to_string(), ModelStatusKind::Ready)
        }
        ModelState::Idle => match local {
            LocalModelState::NotInstalled => (
                "Not downloaded; network access starts only after pressing the button.".to_string(),
                ModelStatusKind::Normal,
            ),
            LocalModelState::Partial => (
                "Download incomplete; it can be resumed.".to_string(),
                ModelStatusKind::Normal,
            ),
            LocalModelState::Installed => (
                "Downloaded and verified; not loaded into memory.".to_string(),
                ModelStatusKind::Normal,
            ),
        },
    }
}

fn format_bytes(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    format!("{:.1} MiB", bytes as f64 / MIB)
}

fn presets_panel(state: &State, wide: bool) -> Element<'_, Message> {
    let names: Vec<String> = state
        .preset_store
        .all()
        .into_iter()
        .map(|p| p.name)
        .collect();
    let picker = pick_list(names, state.selected_preset.clone(), Message::SelectPreset)
        .placeholder("Select preset")
        .style(theme::selection)
        .text_size(14)
        .handle(pick_list::Handle::Arrow {
            size: Some(10.0.into()),
        })
        .width(Length::Fixed(220.0));
    let name_input = text_input("Preset name", &state.new_preset_name)
        .on_input(Message::PresetNameInput)
        .style(theme::input)
        .width(Length::Fixed(130.0));
    let delete = button(text("×  Delete")).style(theme::danger_button);
    let delete = if state
        .selected_preset
        .as_deref()
        .is_some_and(|name| state.preset_store.is_user(name))
    {
        delete.on_press(Message::DeletePreset)
    } else {
        delete
    };
    sized_section(
        "Presets",
        row![
            picker,
            name_input,
            button(text("+  Save Current"))
                .on_press(Message::SavePreset)
                .style(theme::secondary_button),
            delete,
        ]
        .align_y(Alignment::Center)
        .spacing(theme::SPACE_SM)
        .wrap()
        .into(),
        if wide { Length::Fill } else { Length::Shrink },
    )
}

fn input_panel(state: &State, wide: bool) -> Element<'_, Message> {
    section("Track", composition_form(state, wide))
}

fn composition_form(state: &State, wide: bool) -> Element<'_, Message> {
    let instruments = instrument_selector(state);
    let controls = track_controls(state);

    if wide {
        row![
            container(instruments).width(Length::FillPortion(2)),
            container(controls).width(Length::FillPortion(3)),
        ]
        .align_y(Alignment::Center)
        .height(Length::Shrink)
        .spacing(theme::SPACE_MD)
        .into()
    } else {
        column![instruments, controls]
            .spacing(theme::SPACE_MD)
            .into()
    }
}

fn instrument_selector(state: &State) -> Element<'_, Message> {
    instrument_browser::view(state)
}

fn track_controls(state: &State) -> Element<'_, Message> {
    let key_options: Vec<String> = std::iter::once(AUTO_VALUE.to_string())
        .chain(KEY_SIGNATURES.iter().map(|s| s.to_string()))
        .collect();

    let musical = row![
        combo("Drum Kit", DRUM_KITS.to_vec(), &state.drum_kit, |v| {
            Message::Form(FormMsg::DrumKit(v))
        }),
        combo(
            "Time Signature",
            TIME_SIGNATURES.to_vec(),
            &state.time_signature,
            |v| { Message::Form(FormMsg::TimeSignature(v)) }
        ),
        combo_owned("Key Signature", key_options, &state.key_signature, |v| {
            Message::Form(FormMsg::KeySignature(v))
        }),
    ]
    .align_y(Alignment::Center)
    .spacing(theme::SPACE_SM)
    .wrap();

    let dimensions = row![
        number("Tempo (BPM)", &state.bpm, |v| Message::Form(FormMsg::Bpm(
            v
        ))),
        number("Length (bars)", &state.bars, |v| Message::Form(
            FormMsg::Bars(v)
        )),
        number("Event Budget per Bar", &state.events_per_bar, |v| {
            Message::Form(FormMsg::EventsPerBar(v))
        }),
    ]
    .align_y(Alignment::Center)
    .spacing(theme::SPACE_SM)
    .wrap();

    column![
        text("Track Settings").size(15),
        container(musical)
            .padding(theme::SPACE_SM)
            .width(Length::Fill)
            .style(theme::inset_card),
        container(dimensions)
            .padding(theme::SPACE_SM)
            .width(Length::Fill)
            .style(theme::inset_card),
        text(state.length_label())
            .size(13)
            .style(iced::widget::text::secondary),
    ]
    .spacing(theme::SPACE_SM)
    .width(Length::Fill)
    .into()
}

fn params_panel(state: &State, wide: bool) -> Element<'_, Message> {
    let sampling = column![
        text("Sampling").size(15),
        row![
            labeled_value(
                "Temperature",
                format!("{:.2}", state.temperature),
                slider(0.1..=1.2, state.temperature, |v| Message::Form(
                    FormMsg::Temperature(v)
                ))
                .step(0.01)
                .width(Length::Fixed(160.0)),
            ),
            labeled_value(
                "Probability Threshold",
                format!("{:.2}", state.top_p),
                slider(0.1..=1.0, state.top_p, |v| Message::Form(FormMsg::TopP(v)))
                    .step(0.01)
                    .width(Length::Fixed(160.0)),
            ),
            number("Top-k Candidates", &state.top_k, |v| Message::Form(
                FormMsg::TopK(v)
            )),
        ]
        .align_y(Alignment::Center)
        .spacing(theme::SPACE_SM)
        .wrap(),
    ]
    .spacing(theme::SPACE_SM);

    let output = column![
        text("Results and Memory").size(15),
        row![
            number("Result Count", &state.batch, |v| Message::Form(
                FormMsg::Batch(v)
            )),
            number("Seed", &state.seed, |v| Message::Form(FormMsg::Seed(v))),
        ]
        .align_y(Alignment::Center)
        .spacing(theme::SPACE_SM)
        .wrap(),
        row![
            checkbox("Random Seed", state.random_seed)
                .on_toggle(|v| Message::Form(FormMsg::RandomSeed(v)))
                .style(theme::check),
            checkbox("Allow MIDI Control Changes", state.allow_cc)
                .on_toggle(|v| Message::Form(FormMsg::AllowControlChanges(v)))
                .style(theme::check),
        ]
        .align_y(Alignment::Center)
        .spacing(theme::SPACE_SM)
        .wrap(),
        combo_owned(
            "Musical Memory",
            CONTEXT_WINDOWS.iter().map(|s| s.to_string()).collect(),
            &state.context_window,
            |v| Message::Form(FormMsg::ContextWindow(v)),
        ),
    ]
    .spacing(theme::SPACE_SM);

    let sampling = container(sampling)
        .padding(theme::SPACE_SM)
        .width(Length::Fill)
        .style(theme::inset_card);
    let output = container(output)
        .padding(theme::SPACE_SM)
        .width(Length::Fill)
        .style(theme::inset_card);

    let settings: Element<'_, Message> = if wide {
        row![
            sampling.width(Length::FillPortion(3)),
            output.width(Length::FillPortion(2)),
        ]
        .align_y(Alignment::Center)
        .height(Length::Shrink)
        .spacing(theme::SPACE_MD)
        .into()
    } else {
        column![sampling, output].spacing(theme::SPACE_SM).into()
    };

    let generate = if state.generating {
        button(text("■  Stop Generation"))
            .on_press(Message::CancelGeneration)
            .style(theme::danger_button)
    } else {
        let generate = button(text("▶  Generate Tracks")).style(theme::primary_button);
        if state.selected_model_is_active() && !state.model.is_busy() {
            generate.on_press(Message::Generate)
        } else {
            generate
        }
    };
    let status = generation_status(state);
    let controls = column![
        row![
            generate.padding([theme::SPACE_SM, theme::SPACE_MD]),
            progress_bar(0.0..=1.0, state.progress)
                .style(theme::progress)
                .width(Length::Fill),
        ]
        .align_y(Alignment::Center)
        .spacing(theme::SPACE_SM),
        status,
    ]
    .spacing(theme::SPACE_SM);

    section(
        "Generation",
        column![settings, controls].spacing(theme::SPACE_SM).into(),
    )
}

fn results_panel(state: &State) -> Element<'_, Message> {
    section("Results", results_view::view(state))
}

// --- helpers ---

fn generation_status(state: &State) -> Element<'_, Message> {
    let message = text(&state.status).size(13);
    let status = state.status.to_ascii_lowercase();
    if status.contains("fail")
        || status.contains("error")
        || status.contains("invalid")
        || status.contains("unavailable")
        || status.contains("could not")
    {
        message.style(iced::widget::text::danger).into()
    } else if state.generating || state.model.is_busy() {
        message.style(iced::widget::text::primary).into()
    } else if status.contains("complete")
        || status.contains("ready")
        || status.contains("saved")
        || status.contains("opened")
    {
        message.style(iced::widget::text::success).into()
    } else {
        message.style(iced::widget::text::secondary).into()
    }
}

fn labeled<'a>(label: &'a str, widget: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![text(label).size(13), widget.into()]
        .spacing(theme::SPACE_XS)
        .into()
}

fn labeled_value<'a>(
    label: &'a str,
    value: String,
    widget: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    column![
        row![
            text(label).size(13),
            Space::with_width(Length::Fill),
            text(value).size(13)
        ]
        .align_y(Alignment::Center),
        widget.into(),
    ]
    .spacing(theme::SPACE_XS)
    .width(Length::Fixed(180.0))
    .into()
}

fn number<'a>(
    label: &'a str,
    value: &'a str,
    on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    labeled(
        label,
        text_input("", value)
            .on_input(on_input)
            .style(theme::input)
            .width(Length::Fixed(120.0)),
    )
}

fn combo<'a>(
    label: &'a str,
    options: Vec<&'static str>,
    selected: &str,
    on_select: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    combo_owned(
        label,
        options.into_iter().map(|s| s.to_string()).collect(),
        selected,
        on_select,
    )
}

fn combo_owned<'a>(
    label: &'a str,
    options: Vec<String>,
    selected: &str,
    on_select: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    labeled(
        label,
        pick_list(options, Some(selected.to_string()), on_select)
            .style(theme::selection)
            .width(Length::Fixed(COMBO_WIDTH)),
    )
}
