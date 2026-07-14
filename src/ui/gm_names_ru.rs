//! Russian display names for the instrument picker. Only a subset is translated
//! (matching the Python `INSTRUMENT_TRANSLATIONS`); the rest fall back to
//! "Инструмент MIDI №N".

use crate::core::midi::gm::PATCH_NAMES;

fn translation(name: &str) -> Option<&'static str> {
    Some(match name {
        "Acoustic Grand" => "Акустический рояль",
        "Bright Acoustic" => "Яркое фортепиано",
        "Electric Grand" => "Электрический рояль",
        "Honky-Tonk" => "Хонки-тонк",
        "Electric Piano 1" => "Электропиано 1",
        "Electric Piano 2" => "Электропиано 2",
        "Church Organ" => "Церковный орган",
        "Acoustic Guitar(nylon)" => "Акустическая гитара (нейлон)",
        "Acoustic Guitar(steel)" => "Акустическая гитара (сталь)",
        "Electric Guitar(clean)" => "Электрогитара (чистый звук)",
        "Electric Guitar(muted)" => "Электрогитара (приглушённая)",
        "Overdriven Guitar" => "Перегруженная гитара",
        "Distortion Guitar" => "Гитара с дисторшном",
        "Acoustic Bass" => "Акустический бас",
        "Electric Bass(finger)" => "Электробас (пальцы)",
        "Electric Bass(pick)" => "Электробас (медиатор)",
        "Synth Bass 1" => "Синтезаторный бас 1",
        "Synth Bass 2" => "Синтезаторный бас 2",
        "Violin" => "Скрипка",
        "Viola" => "Альт",
        "Cello" => "Виолончель",
        "Contrabass" => "Контрабас",
        "Tremolo Strings" => "Струнные тремоло",
        "Pizzicato Strings" => "Струнные пиццикато",
        "String Ensemble 1" => "Струнный ансамбль 1",
        "String Ensemble 2" => "Струнный ансамбль 2",
        "SynthStrings 1" => "Синтезаторные струнные 1",
        "SynthStrings 2" => "Синтезаторные струнные 2",
        "Trumpet" => "Труба",
        "Trombone" => "Тромбон",
        "Tuba" => "Туба",
        "French Horn" => "Валторна",
        "Soprano Sax" => "Сопрано-саксофон",
        "Alto Sax" => "Альт-саксофон",
        "Tenor Sax" => "Тенор-саксофон",
        "Baritone Sax" => "Баритон-саксофон",
        "Oboe" => "Гобой",
        "English Horn" => "Английский рожок",
        "Bassoon" => "Фагот",
        "Clarinet" => "Кларнет",
        "Flute" => "Флейта",
        "Lead 2 (sawtooth)" => "Соло-синтезатор (пила)",
        "Lead 5 (charang)" => "Соло-синтезатор (чаранго)",
        "Pad 1 (new age)" => "Синтезаторная подкладка (нью-эйдж)",
        "Pad 2 (warm)" => "Тёплая синтезаторная подкладка",
        "Orchestra Hit" => "Оркестровый акцент",
        _ => return None,
    })
}

/// Russian label for the instrument at program number `index`.
pub fn instrument_label(index: usize) -> String {
    let english = PATCH_NAMES[index];
    translation(english)
        .map(str::to_string)
        .unwrap_or_else(|| format!("Инструмент MIDI №{}", index + 1))
}
