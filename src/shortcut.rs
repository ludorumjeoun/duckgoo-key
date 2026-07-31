use std::fmt;

use global_hotkey::hotkey::{Code, HotKey, Modifiers as GlobalHotKeyModifiers};
use iced::keyboard::{Key, Modifiers as IcedModifiers, key::Named};
use serde::{Deserialize, Serialize};

/// The modifier keys supported by DuckGooKey shortcuts.
///
/// The fields are private so a binding can enforce its safety invariant at
/// construction and deserialization boundaries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShortcutModifiers {
    command: bool,
    option: bool,
    control: bool,
    shift: bool,
}

impl ShortcutModifiers {
    pub const NONE: Self = Self::new(false, false, false, false);
    pub const COMMAND: Self = Self::new(true, false, false, false);
    pub const OPTION: Self = Self::new(false, true, false, false);
    pub const CONTROL: Self = Self::new(false, false, true, false);
    pub const SHIFT: Self = Self::new(false, false, false, true);

    pub const fn new(command: bool, option: bool, control: bool, shift: bool) -> Self {
        Self {
            command,
            option,
            control,
            shift,
        }
    }

    pub const fn command(self) -> bool {
        self.command
    }

    pub const fn option(self) -> bool {
        self.option
    }

    pub const fn control(self) -> bool {
        self.control
    }

    pub const fn shift(self) -> bool {
        self.shift
    }

    pub const fn union(self, other: Self) -> Self {
        Self {
            command: self.command || other.command,
            option: self.option || other.option,
            control: self.control || other.control,
            shift: self.shift || other.shift,
        }
    }

    pub fn from_iced(modifiers: IcedModifiers) -> Self {
        Self::new(
            modifiers.logo(),
            modifiers.alt(),
            modifiers.control(),
            modifiers.shift(),
        )
    }

    const fn has_non_shift_modifier(self) -> bool {
        self.command || self.option || self.control
    }

    fn to_global_hotkey(self) -> GlobalHotKeyModifiers {
        let mut modifiers = GlobalHotKeyModifiers::empty();
        if self.command {
            modifiers.insert(GlobalHotKeyModifiers::SUPER);
        }
        if self.option {
            modifiers.insert(GlobalHotKeyModifiers::ALT);
        }
        if self.control {
            modifiers.insert(GlobalHotKeyModifiers::CONTROL);
        }
        if self.shift {
            modifiers.insert(GlobalHotKeyModifiers::SHIFT);
        }
        modifiers
    }
}

/// A stable, persisted key identifier for a DuckGooKey shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutKey {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    Space,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,
    F25,
    F26,
    F27,
    F28,
    F29,
    F30,
    F31,
    F32,
    F33,
    F34,
    F35,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Return,
    Tab,
    Backspace,
    Delete,
}

impl ShortcutKey {
    pub const fn to_global_code(self) -> Code {
        match self {
            Self::A => Code::KeyA,
            Self::B => Code::KeyB,
            Self::C => Code::KeyC,
            Self::D => Code::KeyD,
            Self::E => Code::KeyE,
            Self::F => Code::KeyF,
            Self::G => Code::KeyG,
            Self::H => Code::KeyH,
            Self::I => Code::KeyI,
            Self::J => Code::KeyJ,
            Self::K => Code::KeyK,
            Self::L => Code::KeyL,
            Self::M => Code::KeyM,
            Self::N => Code::KeyN,
            Self::O => Code::KeyO,
            Self::P => Code::KeyP,
            Self::Q => Code::KeyQ,
            Self::R => Code::KeyR,
            Self::S => Code::KeyS,
            Self::T => Code::KeyT,
            Self::U => Code::KeyU,
            Self::V => Code::KeyV,
            Self::W => Code::KeyW,
            Self::X => Code::KeyX,
            Self::Y => Code::KeyY,
            Self::Z => Code::KeyZ,
            Self::Digit0 => Code::Digit0,
            Self::Digit1 => Code::Digit1,
            Self::Digit2 => Code::Digit2,
            Self::Digit3 => Code::Digit3,
            Self::Digit4 => Code::Digit4,
            Self::Digit5 => Code::Digit5,
            Self::Digit6 => Code::Digit6,
            Self::Digit7 => Code::Digit7,
            Self::Digit8 => Code::Digit8,
            Self::Digit9 => Code::Digit9,
            Self::Space => Code::Space,
            Self::F1 => Code::F1,
            Self::F2 => Code::F2,
            Self::F3 => Code::F3,
            Self::F4 => Code::F4,
            Self::F5 => Code::F5,
            Self::F6 => Code::F6,
            Self::F7 => Code::F7,
            Self::F8 => Code::F8,
            Self::F9 => Code::F9,
            Self::F10 => Code::F10,
            Self::F11 => Code::F11,
            Self::F12 => Code::F12,
            Self::F13 => Code::F13,
            Self::F14 => Code::F14,
            Self::F15 => Code::F15,
            Self::F16 => Code::F16,
            Self::F17 => Code::F17,
            Self::F18 => Code::F18,
            Self::F19 => Code::F19,
            Self::F20 => Code::F20,
            Self::F21 => Code::F21,
            Self::F22 => Code::F22,
            Self::F23 => Code::F23,
            Self::F24 => Code::F24,
            Self::F25 => Code::F25,
            Self::F26 => Code::F26,
            Self::F27 => Code::F27,
            Self::F28 => Code::F28,
            Self::F29 => Code::F29,
            Self::F30 => Code::F30,
            Self::F31 => Code::F31,
            Self::F32 => Code::F32,
            Self::F33 => Code::F33,
            Self::F34 => Code::F34,
            Self::F35 => Code::F35,
            Self::ArrowUp => Code::ArrowUp,
            Self::ArrowDown => Code::ArrowDown,
            Self::ArrowLeft => Code::ArrowLeft,
            Self::ArrowRight => Code::ArrowRight,
            Self::Return => Code::Enter,
            Self::Tab => Code::Tab,
            Self::Backspace => Code::Backspace,
            Self::Delete => Code::Delete,
        }
    }

    pub const fn macos_label(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::D => "D",
            Self::E => "E",
            Self::F => "F",
            Self::G => "G",
            Self::H => "H",
            Self::I => "I",
            Self::J => "J",
            Self::K => "K",
            Self::L => "L",
            Self::M => "M",
            Self::N => "N",
            Self::O => "O",
            Self::P => "P",
            Self::Q => "Q",
            Self::R => "R",
            Self::S => "S",
            Self::T => "T",
            Self::U => "U",
            Self::V => "V",
            Self::W => "W",
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
            Self::Digit0 => "0",
            Self::Digit1 => "1",
            Self::Digit2 => "2",
            Self::Digit3 => "3",
            Self::Digit4 => "4",
            Self::Digit5 => "5",
            Self::Digit6 => "6",
            Self::Digit7 => "7",
            Self::Digit8 => "8",
            Self::Digit9 => "9",
            Self::Space => "Space",
            Self::F1 => "F1",
            Self::F2 => "F2",
            Self::F3 => "F3",
            Self::F4 => "F4",
            Self::F5 => "F5",
            Self::F6 => "F6",
            Self::F7 => "F7",
            Self::F8 => "F8",
            Self::F9 => "F9",
            Self::F10 => "F10",
            Self::F11 => "F11",
            Self::F12 => "F12",
            Self::F13 => "F13",
            Self::F14 => "F14",
            Self::F15 => "F15",
            Self::F16 => "F16",
            Self::F17 => "F17",
            Self::F18 => "F18",
            Self::F19 => "F19",
            Self::F20 => "F20",
            Self::F21 => "F21",
            Self::F22 => "F22",
            Self::F23 => "F23",
            Self::F24 => "F24",
            Self::F25 => "F25",
            Self::F26 => "F26",
            Self::F27 => "F27",
            Self::F28 => "F28",
            Self::F29 => "F29",
            Self::F30 => "F30",
            Self::F31 => "F31",
            Self::F32 => "F32",
            Self::F33 => "F33",
            Self::F34 => "F34",
            Self::F35 => "F35",
            Self::ArrowUp => "↑",
            Self::ArrowDown => "↓",
            Self::ArrowLeft => "←",
            Self::ArrowRight => "→",
            Self::Return => "↩",
            Self::Tab => "⇥",
            Self::Backspace => "⌫",
            Self::Delete => "⌦",
        }
    }

    const fn is_function_key(self) -> bool {
        matches!(
            self,
            Self::F1
                | Self::F2
                | Self::F3
                | Self::F4
                | Self::F5
                | Self::F6
                | Self::F7
                | Self::F8
                | Self::F9
                | Self::F10
                | Self::F11
                | Self::F12
                | Self::F13
                | Self::F14
                | Self::F15
                | Self::F16
                | Self::F17
                | Self::F18
                | Self::F19
                | Self::F20
                | Self::F21
                | Self::F22
                | Self::F23
                | Self::F24
                | Self::F25
                | Self::F26
                | Self::F27
                | Self::F28
                | Self::F29
                | Self::F30
                | Self::F31
                | Self::F32
                | Self::F33
                | Self::F34
                | Self::F35
        )
    }

    fn from_iced(key: &Key) -> Result<Self, ShortcutBindingError> {
        match key.as_ref() {
            Key::Named(named) if is_modifier_key(named) => Err(ShortcutBindingError::ModifierOnly),
            Key::Named(named) => {
                Self::from_iced_named(named).ok_or(ShortcutBindingError::UnsupportedKey)
            }
            Key::Character(character) => {
                Self::from_character(character).ok_or(ShortcutBindingError::UnsupportedKey)
            }
            Key::Unidentified => Err(ShortcutBindingError::UnsupportedKey),
        }
    }

    fn from_character(character: &str) -> Option<Self> {
        let mut characters = character.chars();
        let character = characters.next()?;
        if characters.next().is_some() {
            return None;
        }

        Some(match character.to_ascii_lowercase() {
            'a' => Self::A,
            'b' => Self::B,
            'c' => Self::C,
            'd' => Self::D,
            'e' => Self::E,
            'f' => Self::F,
            'g' => Self::G,
            'h' => Self::H,
            'i' => Self::I,
            'j' => Self::J,
            'k' => Self::K,
            'l' => Self::L,
            'm' => Self::M,
            'n' => Self::N,
            'o' => Self::O,
            'p' => Self::P,
            'q' => Self::Q,
            'r' => Self::R,
            's' => Self::S,
            't' => Self::T,
            'u' => Self::U,
            'v' => Self::V,
            'w' => Self::W,
            'x' => Self::X,
            'y' => Self::Y,
            'z' => Self::Z,
            '0' => Self::Digit0,
            '1' => Self::Digit1,
            '2' => Self::Digit2,
            '3' => Self::Digit3,
            '4' => Self::Digit4,
            '5' => Self::Digit5,
            '6' => Self::Digit6,
            '7' => Self::Digit7,
            '8' => Self::Digit8,
            '9' => Self::Digit9,
            ' ' => Self::Space,
            _ => return None,
        })
    }

    fn from_iced_named(named: Named) -> Option<Self> {
        Some(match named {
            Named::Space => Self::Space,
            Named::F1 => Self::F1,
            Named::F2 => Self::F2,
            Named::F3 => Self::F3,
            Named::F4 => Self::F4,
            Named::F5 => Self::F5,
            Named::F6 => Self::F6,
            Named::F7 => Self::F7,
            Named::F8 => Self::F8,
            Named::F9 => Self::F9,
            Named::F10 => Self::F10,
            Named::F11 => Self::F11,
            Named::F12 => Self::F12,
            Named::F13 => Self::F13,
            Named::F14 => Self::F14,
            Named::F15 => Self::F15,
            Named::F16 => Self::F16,
            Named::F17 => Self::F17,
            Named::F18 => Self::F18,
            Named::F19 => Self::F19,
            Named::F20 => Self::F20,
            Named::F21 => Self::F21,
            Named::F22 => Self::F22,
            Named::F23 => Self::F23,
            Named::F24 => Self::F24,
            Named::F25 => Self::F25,
            Named::F26 => Self::F26,
            Named::F27 => Self::F27,
            Named::F28 => Self::F28,
            Named::F29 => Self::F29,
            Named::F30 => Self::F30,
            Named::F31 => Self::F31,
            Named::F32 => Self::F32,
            Named::F33 => Self::F33,
            Named::F34 => Self::F34,
            Named::F35 => Self::F35,
            Named::ArrowUp => Self::ArrowUp,
            Named::ArrowDown => Self::ArrowDown,
            Named::ArrowLeft => Self::ArrowLeft,
            Named::ArrowRight => Self::ArrowRight,
            Named::Enter => Self::Return,
            Named::Tab => Self::Tab,
            Named::Backspace => Self::Backspace,
            Named::Delete => Self::Delete,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutBindingError {
    ModifierOnly,
    UnsupportedKey,
    UnsafeWithoutModifier(ShortcutKey),
}

impl fmt::Display for ShortcutBindingError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModifierOnly => formatter.write_str("a shortcut needs a non-modifier key"),
            Self::UnsupportedKey => formatter.write_str("this key is not supported as a shortcut"),
            Self::UnsafeWithoutModifier(key) => write!(
                formatter,
                "{} needs Command, Option, or Control to avoid intercepting normal typing",
                key.macos_label()
            ),
        }
    }
}

impl std::error::Error for ShortcutBindingError {}

/// A validated global shortcut binding.
///
/// Function keys may be used without a modifier. Every other supported key
/// requires Command, Option, or Control; Shift alone does not make a typing or
/// navigation key safe to register globally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    try_from = "SerializedShortcutBinding",
    into = "SerializedShortcutBinding"
)]
pub struct ShortcutBinding {
    modifiers: ShortcutModifiers,
    key: ShortcutKey,
}

impl ShortcutBinding {
    pub const DEFAULT: Self = Self {
        modifiers: ShortcutModifiers::OPTION,
        key: ShortcutKey::Space,
    };

    pub fn new(
        modifiers: ShortcutModifiers,
        key: ShortcutKey,
    ) -> Result<Self, ShortcutBindingError> {
        if !modifiers.has_non_shift_modifier() && !key.is_function_key() {
            return Err(ShortcutBindingError::UnsafeWithoutModifier(key));
        }

        Ok(Self { modifiers, key })
    }

    pub const fn modifiers(self) -> ShortcutModifiers {
        self.modifiers
    }

    pub const fn key(self) -> ShortcutKey {
        self.key
    }

    pub fn try_from_iced(
        key: &Key,
        modifiers: IcedModifiers,
    ) -> Result<Self, ShortcutBindingError> {
        Self::new(
            ShortcutModifiers::from_iced(modifiers),
            ShortcutKey::from_iced(key)?,
        )
    }

    pub fn to_hotkey(self) -> HotKey {
        let modifiers = self.modifiers.to_global_hotkey();
        let modifiers = (!modifiers.is_empty()).then_some(modifiers);
        HotKey::new(modifiers, self.key.to_global_code())
    }

    pub fn macos_label(self) -> String {
        let mut label = String::with_capacity(12);

        // This is the conventional order used in macOS menu shortcuts.
        if self.modifiers.control {
            label.push('⌃');
        }
        if self.modifiers.option {
            label.push('⌥');
        }
        if self.modifiers.shift {
            label.push('⇧');
        }
        if self.modifiers.command {
            label.push('⌘');
        }
        label.push_str(self.key.macos_label());

        label
    }
}

impl Default for ShortcutBinding {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl fmt::Display for ShortcutBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.macos_label())
    }
}

impl From<ShortcutBinding> for HotKey {
    fn from(binding: ShortcutBinding) -> Self {
        binding.to_hotkey()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SerializedShortcutBinding {
    modifiers: ShortcutModifiers,
    key: ShortcutKey,
}

impl TryFrom<SerializedShortcutBinding> for ShortcutBinding {
    type Error = ShortcutBindingError;

    fn try_from(value: SerializedShortcutBinding) -> Result<Self, Self::Error> {
        Self::new(value.modifiers, value.key)
    }
}

impl From<ShortcutBinding> for SerializedShortcutBinding {
    fn from(binding: ShortcutBinding) -> Self {
        Self {
            modifiers: binding.modifiers,
            key: binding.key,
        }
    }
}

fn is_modifier_key(named: Named) -> bool {
    matches!(
        named,
        Named::Alt
            | Named::AltGraph
            | Named::Control
            | Named::Fn
            | Named::FnLock
            | Named::Shift
            | Named::Symbol
            | Named::SymbolLock
            | Named::Meta
            | Named::Hyper
            | Named::Super
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_option_space() {
        let binding = ShortcutBinding::default();
        let hotkey = binding.to_hotkey();

        assert_eq!(binding.macos_label(), "⌥Space");
        assert_eq!(hotkey.mods, GlobalHotKeyModifiers::ALT);
        assert_eq!(hotkey.key, Code::Space);
    }

    #[test]
    fn label_uses_macos_modifier_order() {
        let modifiers = ShortcutModifiers::COMMAND
            .union(ShortcutModifiers::OPTION)
            .union(ShortcutModifiers::CONTROL)
            .union(ShortcutModifiers::SHIFT);
        let binding = ShortcutBinding::new(modifiers, ShortcutKey::K).unwrap();

        assert_eq!(binding.macos_label(), "⌃⌥⇧⌘K");
    }

    #[test]
    fn captures_letters_and_digits_case_insensitively() {
        let command_k =
            ShortcutBinding::try_from_iced(&Key::Character("K".into()), IcedModifiers::LOGO)
                .unwrap();
        let option_seven =
            ShortcutBinding::try_from_iced(&Key::Character("7".into()), IcedModifiers::ALT)
                .unwrap();

        assert_eq!(command_k.key(), ShortcutKey::K);
        assert_eq!(command_k.macos_label(), "⌘K");
        assert_eq!(option_seven.key(), ShortcutKey::Digit7);
    }

    #[test]
    fn captures_named_keys_and_converts_them() {
        let binding =
            ShortcutBinding::try_from_iced(&Key::Named(Named::ArrowLeft), IcedModifiers::CTRL)
                .unwrap();

        assert_eq!(binding.key(), ShortcutKey::ArrowLeft);
        assert_eq!(binding.to_hotkey().key, Code::ArrowLeft);
        assert_eq!(binding.macos_label(), "⌃←");
    }

    #[test]
    fn rejects_modifier_only_and_unsupported_keys() {
        assert_eq!(
            ShortcutBinding::try_from_iced(&Key::Named(Named::Shift), IcedModifiers::SHIFT),
            Err(ShortcutBindingError::ModifierOnly)
        );
        assert_eq!(
            ShortcutBinding::try_from_iced(&Key::Named(Named::Escape), IcedModifiers::ALT),
            Err(ShortcutBindingError::UnsupportedKey)
        );
        assert_eq!(
            ShortcutBinding::try_from_iced(&Key::Character("한".into()), IcedModifiers::ALT),
            Err(ShortcutBindingError::UnsupportedKey)
        );
    }

    #[test]
    fn rejects_unsafe_global_typing_and_navigation_keys() {
        assert_eq!(
            ShortcutBinding::new(ShortcutModifiers::NONE, ShortcutKey::A),
            Err(ShortcutBindingError::UnsafeWithoutModifier(ShortcutKey::A))
        );
        assert_eq!(
            ShortcutBinding::new(ShortcutModifiers::SHIFT, ShortcutKey::ArrowDown),
            Err(ShortcutBindingError::UnsafeWithoutModifier(
                ShortcutKey::ArrowDown
            ))
        );
    }

    #[test]
    fn allows_unmodified_function_keys() {
        let binding = ShortcutBinding::new(ShortcutModifiers::NONE, ShortcutKey::F12).unwrap();

        assert_eq!(binding.to_hotkey().mods, GlobalHotKeyModifiers::empty());
        assert_eq!(binding.to_hotkey().key, Code::F12);
    }

    #[test]
    fn serde_round_trip_uses_stable_names_and_revalidates() {
        let json = serde_json::to_string(&ShortcutBinding::DEFAULT).unwrap();
        let decoded: ShortcutBinding = serde_json::from_str(&json).unwrap();

        assert_eq!(
            json,
            r#"{"modifiers":{"command":false,"option":true,"control":false,"shift":false},"key":"space"}"#
        );
        assert_eq!(decoded, ShortcutBinding::DEFAULT);

        let unsafe_json = r#"{
            "modifiers": {},
            "key": "a"
        }"#;
        assert!(serde_json::from_str::<ShortcutBinding>(unsafe_json).is_err());
    }

    #[test]
    fn all_supported_special_keys_map_to_global_hotkey_codes() {
        let cases = [
            (ShortcutKey::Return, Code::Enter, "↩"),
            (ShortcutKey::Tab, Code::Tab, "⇥"),
            (ShortcutKey::Backspace, Code::Backspace, "⌫"),
            (ShortcutKey::Delete, Code::Delete, "⌦"),
            (ShortcutKey::F35, Code::F35, "F35"),
        ];

        for (key, expected_code, expected_label) in cases {
            assert_eq!(key.to_global_code(), expected_code);
            assert_eq!(key.macos_label(), expected_label);
        }
    }
}
