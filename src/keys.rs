//! Controller buttons and the keyboard/mouse keys they map to.
//!
//! Names are matched case-insensitively, so `a`, `A` and `A` are the same
//! button, and `space`, `Space` and `SPACE` are the same key.

use std::fmt;
use std::str::FromStr;

use serde::Deserialize;
use serde::de::{self, Deserializer};

/// A digital button on an Xbox-compatible pad.
///
/// `Lt` and `Rt` are listed here for configuration convenience but are read
/// from the analog trigger values rather than the button bitmask.
///
/// The d-pad diagonals are listed even though a real pad has no diagonal
/// switches: they are derived from the two cardinal bits, so a diagonal counts
/// as pressed only while both of its cardinals are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Button {
    A,
    B,
    X,
    Y,
    Lb,
    Rb,
    Ls,
    Rs,
    Back,
    Start,
    Up,
    Down,
    Left,
    Right,
    UpLeft,
    UpRight,
    DownLeft,
    DownRight,
    Lt,
    Rt,
}

impl Button {
    /// Every button, in a stable order useful for diagnostics.
    pub const ALL: [Self; 20] = [
        Self::A,
        Self::B,
        Self::X,
        Self::Y,
        Self::Lb,
        Self::Rb,
        Self::Ls,
        Self::Rs,
        Self::Back,
        Self::Start,
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
        Self::UpLeft,
        Self::UpRight,
        Self::DownLeft,
        Self::DownRight,
        Self::Lt,
        Self::Rt,
    ];

    /// Canonical upper-case name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::X => "X",
            Self::Y => "Y",
            Self::Lb => "LB",
            Self::Rb => "RB",
            Self::Ls => "LS",
            Self::Rs => "RS",
            Self::Back => "BACK",
            Self::Start => "START",
            Self::Up => "UP",
            Self::Down => "DOWN",
            Self::Left => "LEFT",
            Self::Right => "RIGHT",
            Self::UpLeft => "DPAD_UP_LEFT",
            Self::UpRight => "DPAD_UP_RIGHT",
            Self::DownLeft => "DPAD_DOWN_LEFT",
            Self::DownRight => "DPAD_DOWN_RIGHT",
            Self::Lt => "LT",
            Self::Rt => "RT",
        }
    }

    /// The d-pad diagonals, in the order they are listed in [`Self::ALL`].
    ///
    /// They are spelled `DPAD_`-prefixed so that the bare `UP_LEFT` stays free
    /// for the stick directions, where it is qualified by the stick instead.
    const DIAGONALS: [(&'static str, Self); 8] = [
        ("DPAD_UP_LEFT", Self::UpLeft),
        ("DPADUPLEFT", Self::UpLeft),
        ("DPAD_UP_RIGHT", Self::UpRight),
        ("DPADUPRIGHT", Self::UpRight),
        ("DPAD_DOWN_LEFT", Self::DownLeft),
        ("DPADDOWNLEFT", Self::DownLeft),
        ("DPAD_DOWN_RIGHT", Self::DownRight),
        ("DPADDOWNRIGHT", Self::DownRight),
    ];

    /// Analog triggers report a 0-255 value instead of a digital bit.
    #[must_use]
    pub const fn is_analog(self) -> bool {
        matches!(self, Self::Lt | Self::Rt)
    }

    /// Case-insensitive parse, accepting the common alternative names.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError::UnknownButton`] when the text names no button.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let upper = text.trim().to_ascii_uppercase();
        let button = match upper.as_str() {
            "A" => Self::A,
            "B" => Self::B,
            "X" => Self::X,
            "Y" => Self::Y,
            "LB" | "L1" | "LEFTBUMPER" | "LEFT_BUMPER" => Self::Lb,
            "RB" | "R1" | "RIGHTBUMPER" | "RIGHT_BUMPER" => Self::Rb,
            "LS" | "L3" | "LEFTSTICK" | "LEFT_STICK" => Self::Ls,
            "RS" | "R3" | "RIGHTSTICK" | "RIGHT_STICK" => Self::Rs,
            "BACK" | "SELECT" => Self::Back,
            "START" => Self::Start,
            "UP" | "DPAD_UP" | "DPADUP" => Self::Up,
            "DOWN" | "DPAD_DOWN" | "DPADDOWN" => Self::Down,
            "LEFT" | "DPAD_LEFT" | "DPADLEFT" => Self::Left,
            "RIGHT" | "DPAD_RIGHT" | "DPADRIGHT" => Self::Right,
            "LT" | "L2" | "LEFTTRIGGER" | "LEFT_TRIGGER" => Self::Lt,
            "RT" | "R2" | "RIGHTTRIGGER" | "RIGHT_TRIGGER" => Self::Rt,
            other => {
                return Self::DIAGONALS
                    .iter()
                    .find(|(name, _)| *name == other)
                    .map_or_else(
                        || Err(ParseError::UnknownButton(other.to_owned())),
                        |&(_, button)| Ok(button),
                    );
            }
        };
        Ok(button)
    }
}

impl fmt::Display for Button {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

impl FromStr for Button {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

impl<'de> Deserialize<'de> for Button {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(de::Error::custom)
    }
}

/// Which analog stick a trigger or output reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Stick {
    Left,
    Right,
}

impl Stick {
    /// Canonical prefix used in configuration names.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Left => "LS",
            Self::Right => "RS",
        }
    }

    /// Long form used by diagnostics.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Left => "left stick",
            Self::Right => "right stick",
        }
    }

    /// The prefix that names this stick's directions, e.g. `LS_`.
    const fn prefix(self) -> &'static str {
        match self {
            Self::Left => "LS_",
            Self::Right => "RS_",
        }
    }
}

impl fmt::Display for Stick {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// One of the eight directions a stick can be pushed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StickDirection {
    Up,
    Down,
    Left,
    Right,
    UpLeft,
    UpRight,
    DownLeft,
    DownRight,
}

impl StickDirection {
    /// Every direction, cardinals first.
    pub const ALL: [Self; 8] = [
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
        Self::UpLeft,
        Self::UpRight,
        Self::DownLeft,
        Self::DownRight,
    ];

    /// Suffixes accepted after a stick prefix, e.g. `UP_LEFT` in `LS_UP_LEFT`.
    const SUFFIXES: [(&'static str, Self); 12] = [
        ("UP", Self::Up),
        ("DOWN", Self::Down),
        ("LEFT", Self::Left),
        ("RIGHT", Self::Right),
        ("UP_LEFT", Self::UpLeft),
        ("UPLEFT", Self::UpLeft),
        ("UP_RIGHT", Self::UpRight),
        ("UPRIGHT", Self::UpRight),
        ("DOWN_LEFT", Self::DownLeft),
        ("DOWNLEFT", Self::DownLeft),
        ("DOWN_RIGHT", Self::DownRight),
        ("DOWNRIGHT", Self::DownRight),
    ];

    /// Canonical suffix, e.g. `UP_LEFT`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Up => "UP",
            Self::Down => "DOWN",
            Self::Left => "LEFT",
            Self::Right => "RIGHT",
            Self::UpLeft => "UP_LEFT",
            Self::UpRight => "UP_RIGHT",
            Self::DownLeft => "DOWN_LEFT",
            Self::DownRight => "DOWN_RIGHT",
        }
    }
}

impl fmt::Display for StickDirection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

/// Splits `LS_UP_LEFT` / `LS_AXIS` into the stick and the rest.
fn split_stick(name: &str) -> Option<(Stick, &str)> {
    [Stick::Left, Stick::Right]
        .into_iter()
        .find_map(|stick| name.strip_prefix(stick.prefix()).map(|rest| (stick, rest)))
}

/// A mouse button or scroll direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    WheelUp,
    WheelDown,
}

impl MouseButton {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Left => "MOUSE_LEFT",
            Self::Right => "MOUSE_RIGHT",
            Self::Middle => "MOUSE_MIDDLE",
            Self::WheelUp => "MOUSE_WHEEL_UP",
            Self::WheelDown => "MOUSE_WHEEL_DOWN",
        }
    }

    /// Wheel directions are a single event, not a held button.
    #[must_use]
    pub const fn is_wheel(self) -> bool {
        matches!(self, Self::WheelUp | Self::WheelDown)
    }

    fn parse(upper: &str) -> Option<Self> {
        match upper {
            "MOUSE_LEFT" | "MOUSELEFT" | "LMB" => Some(Self::Left),
            "MOUSE_RIGHT" | "MOUSERIGHT" | "RMB" => Some(Self::Right),
            "MOUSE_MIDDLE" | "MOUSEMIDDLE" | "MMB" => Some(Self::Middle),
            "MOUSE_WHEEL_UP" | "MOUSEWHEELUP" | "WHEEL_UP" | "WHEELUP" => Some(Self::WheelUp),
            "MOUSE_WHEEL_DOWN" | "MOUSEWHEELDOWN" | "WHEEL_DOWN" | "WHEELDOWN" => {
                Some(Self::WheelDown)
            }
            _ => None,
        }
    }
}

/// A keyboard key, stored as a Windows virtual-key code plus its extended flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyboardKey {
    /// Windows virtual-key code.
    pub vk: u16,
    /// Keys such as the arrow cluster need `KEYEVENTF_EXTENDEDKEY`.
    pub extended: bool,
}

impl KeyboardKey {
    #[must_use]
    pub const fn new(vk: u16, extended: bool) -> Self {
        Self { vk, extended }
    }
}

/// Everything `remaprs` can emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    Keyboard(KeyboardKey),
    Mouse(MouseButton),
}

impl Key {
    /// Wheel directions emit one scroll event and have no key-up.
    #[must_use]
    pub const fn is_wheel(self) -> bool {
        matches!(self, Self::Mouse(mouse) if mouse.is_wheel())
    }

    /// Case-insensitive parse of a key name.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError::UnknownKey`] when the text names no key.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        let upper = text.trim().to_ascii_uppercase();
        if let Some(mouse) = MouseButton::parse(&upper) {
            return Ok(Self::Mouse(mouse));
        }
        if let Some(keyboard) = keyboard_from_name(&upper) {
            return Ok(Self::Keyboard(keyboard));
        }
        Err(ParseError::UnknownKey(upper))
    }
}

impl fmt::Display for Key {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mouse(mouse) => formatter.write_str(mouse.name()),
            Self::Keyboard(keyboard) => formatter.write_str(&keyboard_name(*keyboard)),
        }
    }
}

impl FromStr for Key {
    type Err = ParseError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Self::parse(text)
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(de::Error::custom)
    }
}

/// What drives a binding: a controller button, a stick direction or axis, or a
/// physical keyboard key.
///
/// Controller buttons and keyboard keys share names (`A` is both), so the two
/// are told apart in the configuration by the binding's `source` field. Stick
/// names (`LS_UP`, `RS_AXIS`) are distinct from button names, so they need no
/// extra field: `LS` is still the stick click, `LS_UP` its direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Trigger {
    /// A digital button on the pad.
    Button(Button),
    /// A key on the physical keyboard, read through `GetAsyncKeyState`.
    Keyboard(KeyboardKey),
    /// The stick pushed in one of eight directions, past the deadzone.
    StickDirection {
        /// Stick that is read.
        stick: Stick,
        /// Direction that counts as pressed.
        direction: StickDirection,
    },
    /// The whole stick, driving a continuous output such as pointer motion.
    StickAxis(Stick),
}

impl Trigger {
    /// Whether this trigger is read from the keyboard.
    #[must_use]
    pub const fn is_keyboard(self) -> bool {
        matches!(self, Self::Keyboard(_))
    }

    /// The stick this trigger reads, if any.
    #[must_use]
    pub const fn stick(self) -> Option<Stick> {
        match self {
            Self::StickDirection { stick, .. } | Self::StickAxis(stick) => Some(stick),
            Self::Button(_) | Self::Keyboard(_) => None,
        }
    }

    /// Parses a controller-side trigger: a button first, then a stick name.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError::UnknownButton`] when the text names neither.
    pub fn parse_controller(text: &str) -> Result<Self, ParseError> {
        if let Ok(button) = Button::parse(text) {
            return Ok(Self::Button(button));
        }

        let upper = text.trim().to_ascii_uppercase();
        let Some((stick, rest)) = split_stick(&upper) else {
            return Err(ParseError::UnknownButton(upper));
        };

        if rest == "AXIS" {
            return Ok(Self::StickAxis(stick));
        }
        StickDirection::SUFFIXES
            .iter()
            .find(|(suffix, _)| *suffix == rest)
            .map(|&(_, direction)| Self::StickDirection { stick, direction })
            .ok_or(ParseError::UnknownButton(upper))
    }
}

impl fmt::Display for Trigger {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Button(button) => write!(formatter, "controller {button}"),
            Self::Keyboard(key) => write!(formatter, "keyboard {}", keyboard_name(*key)),
            Self::StickDirection { stick, direction } => {
                write!(formatter, "{} {direction}", stick.description())
            }
            Self::StickAxis(stick) => write!(formatter, "{} (analog)", stick.description()),
        }
    }
}

/// Failure to recognise a button or key name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// No controller button with that name.
    UnknownButton(String),
    /// No output key with that name.
    UnknownKey(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownButton(name) => {
                write!(formatter, "unknown controller button `{name}`")
            }
            Self::UnknownKey(name) => write!(formatter, "unknown output key `{name}`"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Keys that are not covered by the letter, digit or `F1`-`F24` rules.
///
/// The third element marks keys that require `KEYEVENTF_EXTENDEDKEY`.
const NAMED_KEYS: &[(&str, u16, bool)] = &[
    ("SPACE", 0x20, false),
    ("ENTER", 0x0D, false),
    ("RETURN", 0x0D, false),
    ("TAB", 0x09, false),
    ("ESCAPE", 0x1B, false),
    ("ESC", 0x1B, false),
    ("BACKSPACE", 0x08, false),
    ("DELETE", 0x2E, true),
    ("DEL", 0x2E, true),
    ("INSERT", 0x2D, true),
    ("INS", 0x2D, true),
    ("HOME", 0x24, true),
    ("END", 0x23, true),
    ("PAGEUP", 0x21, true),
    ("PAGEDOWN", 0x22, true),
    ("UP", 0x26, true),
    ("DOWN", 0x28, true),
    ("LEFT", 0x25, true),
    ("RIGHT", 0x27, true),
    ("SHIFT", 0x10, false),
    ("LSHIFT", 0xA0, false),
    ("RSHIFT", 0xA1, false),
    ("CONTROL", 0x11, false),
    ("CTRL", 0x11, false),
    ("LCONTROL", 0xA2, false),
    ("RCONTROL", 0xA3, false),
    ("LCTRL", 0xA2, false),
    ("RCTRL", 0xA3, false),
    ("ALT", 0x12, false),
    ("LALT", 0xA4, false),
    ("RALT", 0xA5, false),
    ("LWIN", 0x5B, true),
    ("RWIN", 0x5C, true),
    ("APPS", 0x5D, true),
    ("MENU", 0x5D, true),
    ("CAPSLOCK", 0x14, false),
    ("NUMLOCK", 0x90, true),
    ("SCROLLLOCK", 0x91, false),
    ("PRINTSCREEN", 0x2C, true),
    ("PAUSE", 0x13, false),
    ("MINUS", 0xBD, false),
    ("EQUALS", 0xBB, false),
    ("COMMA", 0xBC, false),
    ("PERIOD", 0xBE, false),
    ("SLASH", 0xBF, false),
    ("BACKSLASH", 0xDC, false),
    ("SEMICOLON", 0xBA, false),
    ("APOSTROPHE", 0xDE, false),
    ("GRAVE", 0xC0, false),
    ("LBRACKET", 0xDB, false),
    ("RBRACKET", 0xDD, false),
    ("NUMPAD0", 0x60, false),
    ("NUMPAD1", 0x61, false),
    ("NUMPAD2", 0x62, false),
    ("NUMPAD3", 0x63, false),
    ("NUMPAD4", 0x64, false),
    ("NUMPAD5", 0x65, false),
    ("NUMPAD6", 0x66, false),
    ("NUMPAD7", 0x67, false),
    ("NUMPAD8", 0x68, false),
    ("NUMPAD9", 0x69, false),
    ("NUMPAD_MULTIPLY", 0x6A, false),
    ("NUMPAD_ADD", 0x6B, false),
    ("NUMPAD_SUBTRACT", 0x6D, false),
    ("NUMPAD_DECIMAL", 0x6E, false),
    ("NUMPAD_DIVIDE", 0x6F, true),
];

fn keyboard_from_name(name: &str) -> Option<KeyboardKey> {
    let bytes = name.as_bytes();
    if bytes.len() == 1 {
        let character = bytes[0];
        if character.is_ascii_alphabetic() || character.is_ascii_digit() {
            // ASCII letters and digits share their value with the VK_* codes.
            return Some(KeyboardKey::new(u16::from(character), false));
        }
    }

    if let Some(digits) = name.strip_prefix('F')
        && let Ok(number) = digits.parse::<u16>()
        && (1..=24).contains(&number)
    {
        return Some(KeyboardKey::new(0x70 + number - 1, false));
    }

    NAMED_KEYS
        .iter()
        .find(|(candidate, _, _)| *candidate == name)
        .map(|&(_, vk, extended)| KeyboardKey::new(vk, extended))
}

fn keyboard_name(key: KeyboardKey) -> String {
    if (0x41..=0x5A).contains(&key.vk) || (0x30..=0x39).contains(&key.vk) {
        // The range is ASCII, so the conversion cannot fail.
        return char::from_u32(u32::from(key.vk)).map_or_else(|| String::from("?"), String::from);
    }
    if (0x70..=0x87).contains(&key.vk) {
        return format!("F{}", key.vk - 0x70 + 1);
    }
    NAMED_KEYS
        .iter()
        .find(|(_, vk, _)| *vk == key.vk)
        .map_or_else(
            || format!("VK_{:02X}", key.vk),
            |(name, _, _)| (*name).to_owned(),
        )
}

#[cfg(test)]
mod tests {
    use super::{
        Button, Key, KeyboardKey, MouseButton, ParseError, Stick, StickDirection, Trigger,
    };

    #[test]
    fn parses_buttons_case_insensitively() {
        assert_eq!(Button::parse("a"), Ok(Button::A));
        assert_eq!(Button::parse("Lt"), Ok(Button::Lt));
        assert_eq!(
            Button::parse("left shoulder"),
            Err(ParseError::UnknownButton("LEFT SHOULDER".to_owned()))
        );
        assert_eq!(Button::parse("lb"), Ok(Button::Lb));
    }

    #[test]
    fn parses_letters_digits_and_function_keys() {
        assert_eq!(
            Key::parse("a"),
            Ok(Key::Keyboard(KeyboardKey::new(0x41, false)))
        );
        assert_eq!(
            Key::parse("7"),
            Ok(Key::Keyboard(KeyboardKey::new(0x37, false)))
        );
        assert_eq!(
            Key::parse("f12"),
            Ok(Key::Keyboard(KeyboardKey::new(0x7B, false)))
        );
        assert_eq!(
            Key::parse("f24"),
            Ok(Key::Keyboard(KeyboardKey::new(0x87, false)))
        );
        assert!(Key::parse("f25").is_err());
    }

    #[test]
    fn parses_named_and_mouse_keys() {
        assert_eq!(
            Key::parse("space"),
            Ok(Key::Keyboard(KeyboardKey::new(0x20, false)))
        );
        assert_eq!(
            Key::parse("PgDown"),
            Err(ParseError::UnknownKey("PGDOWN".to_owned()))
        );
        assert_eq!(
            Key::parse("PAGEDOWN"),
            Ok(Key::Keyboard(KeyboardKey::new(0x22, true)))
        );
        assert_eq!(Key::parse("mouse_left"), Ok(Key::Mouse(MouseButton::Left)));
        assert_eq!(Key::parse("wheel_up"), Ok(Key::Mouse(MouseButton::WheelUp)));
    }

    #[test]
    fn display_round_trips() {
        for name in [
            "A",
            "SPACE",
            "F12",
            "PAGEDOWN",
            "MOUSE_LEFT",
            "MOUSE_WHEEL_UP",
        ] {
            let key = Key::parse(name).expect("valid key name");
            assert_eq!(key.to_string(), name);
        }
        for button in Button::ALL {
            assert_eq!(Button::parse(button.name()), Ok(button));
        }
    }

    #[test]
    fn analog_buttons_are_marked() {
        assert!(Button::Lt.is_analog());
        assert!(Button::Rt.is_analog());
        assert!(!Button::Lb.is_analog());
    }

    #[test]
    fn wheel_keys_report_themselves() {
        assert!(Key::parse("MOUSE_WHEEL_DOWN").expect("valid").is_wheel());
        assert!(!Key::parse("MOUSE_LEFT").expect("valid").is_wheel());
    }

    #[test]
    fn parses_dpad_diagonals_only_with_the_dpad_prefix() {
        for (name, expected) in [
            ("DPAD_UP_LEFT", Button::UpLeft),
            ("dpad_up_right", Button::UpRight),
            ("DPAD_DOWN_LEFT", Button::DownLeft),
            ("DPADDOWNRIGHT", Button::DownRight),
        ] {
            assert_eq!(Button::parse(name), Ok(expected), "{name}");
        }

        // The bare spelling belongs to the stick directions, so a bare d-pad
        // diagonal must not silently resolve to a button.
        assert!(matches!(
            Button::parse("UP_LEFT"),
            Err(ParseError::UnknownButton(_))
        ));
        assert_eq!(
            Button::parse("LS_UP"),
            Err(ParseError::UnknownButton("LS_UP".to_owned()))
        );
    }

    #[test]
    fn every_button_name_round_trips_including_diagonals() {
        assert_eq!(Button::ALL.len(), 20);
        for button in Button::ALL {
            assert_eq!(Button::parse(button.name()), Ok(button), "{button}");
        }
    }

    #[test]
    fn parses_stick_directions_and_axes() {
        for direction in StickDirection::ALL {
            for stick in [Stick::Left, Stick::Right] {
                let name = format!("{}_{}", stick.name(), direction.name());
                assert_eq!(
                    Trigger::parse_controller(&name),
                    Ok(Trigger::StickDirection { stick, direction }),
                    "{name}"
                );
                let shown = Trigger::parse_controller(&name).expect("valid").to_string();
                assert!(
                    shown.starts_with(stick.description()),
                    "{name} displayed as {shown}"
                );
            }
        }

        assert_eq!(
            Trigger::parse_controller("rs_axis"),
            Ok(Trigger::StickAxis(Stick::Right))
        );
        // The stick click keeps its old meaning.
        assert_eq!(
            Trigger::parse_controller("LS"),
            Ok(Trigger::Button(Button::Ls))
        );
        assert!(matches!(
            Trigger::parse_controller("LS_SIDEWAYS"),
            Err(ParseError::UnknownButton(_))
        ));
    }
}
