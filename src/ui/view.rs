//! The iced `view`: panels mirroring the Tkinter layout (CPU-only, so no
//! execution-mode selector).

use iced::widget::{
    Space, button, canvas, checkbox, column, container, pick_list, progress_bar, row, scrollable,
    slider, text, text_input,
};
use iced::{Element, Length};

use crate::core::midi::gm::PATCH_NAMES;
use crate::services::generation::KEY_SIGNATURES;
use crate::settings::AUTO_VALUE;

use super::density_canvas::DensityCanvas;
use super::gm_names_ru::instrument_label;
use super::message::{FormMsg, Message, Tab};
use super::state::{CONTEXT_WINDOWS, DRUM_KITS, ModelState, State, TIME_SIGNATURES};

pub fn view(state: &State) -> Element<'_, Message> {
    let content = column![
        model_panel(state),
        presets_panel(state),
        tabs_panel(state),
        params_panel(state),
        results_panel(state),
    ]
    .spacing(10)
    .padding(12);

    scrollable(content).into()
}

fn section<'a>(title: &'a str, body: Element<'a, Message>) -> Element<'a, Message> {
    container(column![text(title).size(18), body].spacing(8))
        .padding(10)
        .width(Length::Fill)
        .into()
}

fn model_panel(state: &State) -> Element<'_, Message> {
    let status = match &state.model {
        ModelState::NotLoaded => "Будет загружена по запросу".to_string(),
        ModelState::Loading => "Загрузка…".to_string(),
        ModelState::Ready(_) => "Загружена: вычисления на CPU".to_string(),
        ModelState::Failed(error) => format!("Ошибка: {error}"),
    };
    let load = button(text("Загрузить модель")).on_press(Message::LoadModel);
    section(
        "Модель",
        row![
            text("Универсальная модель MIDI (tv2o-medium)"),
            Space::with_width(Length::Fill),
            load,
        ]
        .spacing(10)
        .push(text(status))
        .into(),
    )
}

fn presets_panel(state: &State) -> Element<'_, Message> {
    let names: Vec<String> = state.preset_store.all().into_iter().map(|p| p.name).collect();
    let picker = pick_list(names, state.selected_preset.clone(), Message::SelectPreset)
        .placeholder("Выберите пресет");
    let name_input = text_input("Название пресета", &state.new_preset_name)
        .on_input(Message::PresetNameInput)
        .width(Length::Fixed(200.0));
    section(
        "Пресеты",
        row![
            picker,
            name_input,
            button(text("Сохранить текущий")).on_press(Message::SavePreset),
            button(text("Удалить")).on_press(Message::DeletePreset),
        ]
        .spacing(8)
        .into(),
    )
}

fn tabs_panel(state: &State) -> Element<'_, Message> {
    let tab_button = |label: &'static str, tab: Tab, active: bool| {
        let b = button(text(label)).on_press(Message::SelectTab(tab));
        if active { b } else { b.style(button::secondary) }
    };
    let bar = row![
        tab_button("Новая композиция", Tab::NewComposition, matches!(state.tab, Tab::NewComposition)),
        tab_button("Продолжить MIDI-файл", Tab::ContinueMidi, matches!(state.tab, Tab::ContinueMidi)),
        tab_button("Продолжить результат", Tab::ContinueResult, matches!(state.tab, Tab::ContinueResult)),
    ]
    .spacing(6);

    let body: Element<Message> = match state.tab {
        Tab::NewComposition => composition_tab(state),
        Tab::ContinueMidi => text(
            "Продолжение MIDI-файла появится в следующей версии порта.",
        )
        .into(),
        Tab::ContinueResult => text(
            "Продолжение результата появится в следующей версии порта.",
        )
        .into(),
    };

    section("Ввод", column![bar, body].spacing(8).into())
}

fn composition_tab(state: &State) -> Element<'_, Message> {
    let mut list = column![].spacing(2);
    for index in 0..PATCH_NAMES.len() {
        list = list.push(
            checkbox(instrument_label(index), state.instruments[index])
                .on_toggle(move |_| Message::ToggleInstrument(index)),
        );
    }
    let instruments = column![
        text("Инструменты (до 15, пустой список — выбор модели)"),
        scrollable(list).height(Length::Fixed(220.0)),
    ]
    .spacing(4)
    .width(Length::FillPortion(1));

    let key_options: Vec<String> = std::iter::once(AUTO_VALUE.to_string())
        .chain(KEY_SIGNATURES.iter().map(|s| s.to_string()))
        .collect();

    let controls = column![
        combo("Ударная установка", DRUM_KITS.to_vec(), &state.drum_kit, |v| {
            Message::Form(FormMsg::DrumKit(v))
        }),
        number("Темп, ударов в минуту", &state.bpm, |v| Message::Form(FormMsg::Bpm(v))),
        combo("Размер такта", TIME_SIGNATURES.to_vec(), &state.time_signature, |v| {
            Message::Form(FormMsg::TimeSignature(v))
        }),
        combo_owned("Тональность", key_options, &state.key_signature, |v| {
            Message::Form(FormMsg::KeySignature(v))
        }),
        number("Длина, тактов", &state.bars, |v| Message::Form(FormMsg::Bars(v))),
        number("Резерв событий на такт", &state.events_per_bar, |v| {
            Message::Form(FormMsg::EventsPerBar(v))
        }),
        text(state.length_label()),
    ]
    .spacing(6)
    .width(Length::FillPortion(1));

    row![instruments, controls].spacing(14).into()
}

fn params_panel(state: &State) -> Element<'_, Message> {
    let sliders = row![
        labeled(
            "Температура",
            slider(0.1..=1.2, state.temperature, |v| Message::Form(FormMsg::Temperature(v)))
                .step(0.01)
                .width(Length::Fixed(160.0)),
        ),
        labeled(
            "Порог вероятности",
            slider(0.1..=1.0, state.top_p, |v| Message::Form(FormMsg::TopP(v)))
                .step(0.01)
                .width(Length::Fixed(160.0)),
        ),
        number("Число вариантов", &state.top_k, |v| Message::Form(FormMsg::TopK(v))),
        number("Количество результатов", &state.batch, |v| Message::Form(FormMsg::Batch(v))),
        number("Начальное значение", &state.seed, |v| Message::Form(FormMsg::Seed(v))),
    ]
    .spacing(12);

    let toggles = row![
        checkbox("Случайное начальное значение", state.random_seed)
            .on_toggle(|v| Message::Form(FormMsg::RandomSeed(v))),
        checkbox("Разрешить управляющие MIDI-события", state.allow_cc)
            .on_toggle(|v| Message::Form(FormMsg::AllowControlChanges(v))),
        combo_owned(
            "Музыкальная память",
            CONTEXT_WINDOWS.iter().map(|s| s.to_string()).collect(),
            &state.context_window,
            |v| Message::Form(FormMsg::ContextWindow(v)),
        ),
    ]
    .spacing(12);

    let generate = if state.generating {
        button(text("Остановить")).on_press(Message::CancelGeneration)
    } else {
        button(text("Сгенерировать")).on_press(Message::Generate)
    };
    let controls = row![
        generate,
        progress_bar(0.0..=1.0, state.progress).width(Length::Fill),
        text(&state.status),
    ]
    .spacing(10);

    section(
        "Параметры генерации",
        column![sliders, toggles, controls].spacing(8).into(),
    )
}

fn results_panel(state: &State) -> Element<'_, Message> {
    let result_names: Vec<String> = (0..state.results.len())
        .map(|i| format!("Результат {}", i + 1))
        .collect();
    let selected = state.selected_result.map(|i| format!("Результат {}", i + 1));
    let selector = pick_list(result_names, selected, |choice| {
        let index = choice
            .rsplit(' ')
            .next()
            .and_then(|n| n.parse::<usize>().ok())
            .map(|n| n - 1)
            .unwrap_or(0);
        Message::SelectResult(index)
    })
    .placeholder("Трек");

    let durations = if state.results.is_empty() {
        "Нет результатов.".to_string()
    } else {
        let mut lines = format!("Начальное значение: {}\n", state.seed_used);
        for (i, duration) in state.result_durations.iter().enumerate() {
            lines.push_str(&format!("Результат {}: {}\n", i + 1, time_label(*duration)));
        }
        lines
    };

    let has_result = state.selected_result.is_some();
    let export = row![
        maybe(button(text("Сохранить MIDI")), has_result, Message::SaveMidi),
        maybe(button(text("Сохранить WAV")), has_result, Message::SaveWav),
        button(text("Открыть папку")).on_press(Message::OpenOutputs),
        button(text("Очистить кэш")).on_press(Message::ClearCache),
    ]
    .spacing(8);

    section(
        "Результаты",
        column![
            row![text("Трек для прослушивания и сохранения"), selector].spacing(10),
            text(durations),
            export,
            player_panel(state),
        ]
        .spacing(8)
        .into(),
    )
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

    let play_label = if state.playing { "Пауза" } else { "Воспроизвести" };
    let play_message = if state.playing { Message::Pause } else { Message::Play };
    let has_timeline = state.timeline.is_some();

    let controls = row![
        maybe(button(text(play_label)), has_timeline, play_message),
        maybe(button(text("Стоп")), has_timeline, Message::StopPlayback),
        Space::with_width(Length::Fill),
        text(format!(
            "{} / {}",
            time_label(state.position),
            time_label(state.duration)
        )),
    ]
    .spacing(8);

    container(column![visualization, seek, controls].spacing(6))
        .padding(8)
        .into()
}

// --- helpers ---

fn labeled<'a>(label: &'a str, widget: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![text(label).size(13), widget.into()].spacing(2).into()
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
        pick_list(options, Some(selected.to_string()), on_select).width(Length::Fixed(200.0)),
    )
}

fn maybe<'a>(
    b: iced::widget::Button<'a, Message>,
    enabled: bool,
    message: Message,
) -> Element<'a, Message> {
    if enabled {
        b.on_press(message).into()
    } else {
        b.into()
    }
}

fn time_label(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}
