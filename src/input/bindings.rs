//! Key rebinding (D93): which key or trackpad button drives each
//! [`PlayerIntent`](crate::shared::PlayerIntent) action.
//!
//! - [`Bindings`] holds one [`Binding`] per [`Action`]; its default is D43's
//!   layout. It lives in [`Tuning`](crate::tuning::Tuning) as `bindings`, so it
//!   saves with the other settings in `userdata/settings.json`.
//! - [`Bindings::bind`] rebinds an action. A binding already used by another
//!   action **swaps** the two. Esc (pause), Cmd and every Cmd combination, and
//!   the F3/F4 dev keys are refused.
//! - The Controls page asks for a binding through [`BindingCapture`]; the
//!   input adapter reads the next key or click ([`captured_input`]) and applies
//!   it, so `src/input.rs` stays the only code that reads devices.
//! - Trackpad look is fixed (not an action).
//!
//! A binding and its "twin" count as one key: Shift means either Shift, and
//! Enter either Enter (the main one or the number pad's).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Every rebindable action, in the Controls page's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    /// Forward; holding it also sprints (D41: sprint by default).
    MoveForward,
    MoveBack,
    MoveLeft,
    MoveRight,
    Jump,
    /// Crouch; a press while sprinting slides.
    Crouch,
    /// Fire the gun, or place (and turbo-place) pieces, or select edit tiles.
    Fire,
    /// Aim down sights while held (D40).
    Aim,
    /// Reload, or reset an edit when editing or aiming at an edited piece.
    Reload,
    Rifle,
    Pump,
    Wall,
    Ramp,
    Floor,
    Cone,
    /// Enter or leave edit mode (D44).
    Edit,
    /// Start the next wave in a break; go again on the results (D79, D84).
    Start,
}

impl Action {
    pub const COUNT: usize = 17;

    pub const ALL: [Action; Action::COUNT] = [
        Action::MoveForward,
        Action::MoveBack,
        Action::MoveLeft,
        Action::MoveRight,
        Action::Jump,
        Action::Crouch,
        Action::Fire,
        Action::Aim,
        Action::Reload,
        Action::Rifle,
        Action::Pump,
        Action::Wall,
        Action::Ramp,
        Action::Floor,
        Action::Cone,
        Action::Edit,
        Action::Start,
    ];

    /// The key in `settings.json`.
    pub fn id(self) -> &'static str {
        match self {
            Action::MoveForward => "move_forward",
            Action::MoveBack => "move_back",
            Action::MoveLeft => "move_left",
            Action::MoveRight => "move_right",
            Action::Jump => "jump",
            Action::Crouch => "crouch",
            Action::Fire => "fire",
            Action::Aim => "aim",
            Action::Reload => "reload",
            Action::Rifle => "rifle",
            Action::Pump => "pump",
            Action::Wall => "wall",
            Action::Ramp => "ramp",
            Action::Floor => "floor",
            Action::Cone => "cone",
            Action::Edit => "edit",
            Action::Start => "start",
        }
    }

    /// The Controls page's label.
    pub fn label(self) -> &'static str {
        match self {
            Action::MoveForward => "Forward (sprint)",
            Action::MoveBack => "Back",
            Action::MoveLeft => "Left",
            Action::MoveRight => "Right",
            Action::Jump => "Jump",
            Action::Crouch => "Crouch / slide",
            Action::Fire => "Fire / place",
            Action::Aim => "Aim (hold)",
            Action::Reload => "Reload / reset edit",
            Action::Rifle => "Rifle",
            Action::Pump => "Pump",
            Action::Wall => "Wall",
            Action::Ramp => "Ramp",
            Action::Floor => "Floor",
            Action::Cone => "Cone",
            Action::Edit => "Edit",
            Action::Start => "Start wave / go again",
        }
    }

    /// D43's layout.
    pub fn default_binding(self) -> Binding {
        use Binding::{Key, Mouse};
        match self {
            Action::MoveForward => Key(KeyCode::KeyW),
            Action::MoveBack => Key(KeyCode::KeyS),
            Action::MoveLeft => Key(KeyCode::KeyA),
            Action::MoveRight => Key(KeyCode::KeyD),
            Action::Jump => Key(KeyCode::Space),
            Action::Crouch => Key(KeyCode::KeyC),
            Action::Fire => Mouse(MouseButton::Left),
            Action::Aim => Key(KeyCode::ShiftLeft),
            Action::Reload => Key(KeyCode::KeyR),
            Action::Rifle => Key(KeyCode::Digit1),
            Action::Pump => Key(KeyCode::Digit2),
            Action::Wall => Key(KeyCode::KeyQ),
            Action::Ramp => Key(KeyCode::KeyE),
            Action::Floor => Key(KeyCode::KeyF),
            Action::Cone => Key(KeyCode::KeyV),
            Action::Edit => Key(KeyCode::KeyG),
            Action::Start => Key(KeyCode::Enter),
        }
    }

    fn from_id(id: &str) -> Option<Action> {
        Action::ALL.into_iter().find(|a| a.id() == id)
    }
}

/// A key or a trackpad (mouse) button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Binding {
    Key(KeyCode),
    Mouse(MouseButton),
}

/// Every key a binding can use: (key, id in `settings.json`, name on screen).
/// A key missing here (Esc, Cmd, F3/F4, Caps Lock, media keys...) can't be
/// bound. Right-hand twins (right Shift, the pad's Enter) bind as the left one.
const KEYS: &[(KeyCode, &str, &str)] = &[
    (KeyCode::KeyA, "KeyA", "A"),
    (KeyCode::KeyB, "KeyB", "B"),
    (KeyCode::KeyC, "KeyC", "C"),
    (KeyCode::KeyD, "KeyD", "D"),
    (KeyCode::KeyE, "KeyE", "E"),
    (KeyCode::KeyF, "KeyF", "F"),
    (KeyCode::KeyG, "KeyG", "G"),
    (KeyCode::KeyH, "KeyH", "H"),
    (KeyCode::KeyI, "KeyI", "I"),
    (KeyCode::KeyJ, "KeyJ", "J"),
    (KeyCode::KeyK, "KeyK", "K"),
    (KeyCode::KeyL, "KeyL", "L"),
    (KeyCode::KeyM, "KeyM", "M"),
    (KeyCode::KeyN, "KeyN", "N"),
    (KeyCode::KeyO, "KeyO", "O"),
    (KeyCode::KeyP, "KeyP", "P"),
    (KeyCode::KeyQ, "KeyQ", "Q"),
    (KeyCode::KeyR, "KeyR", "R"),
    (KeyCode::KeyS, "KeyS", "S"),
    (KeyCode::KeyT, "KeyT", "T"),
    (KeyCode::KeyU, "KeyU", "U"),
    (KeyCode::KeyV, "KeyV", "V"),
    (KeyCode::KeyW, "KeyW", "W"),
    (KeyCode::KeyX, "KeyX", "X"),
    (KeyCode::KeyY, "KeyY", "Y"),
    (KeyCode::KeyZ, "KeyZ", "Z"),
    (KeyCode::Digit0, "Digit0", "0"),
    (KeyCode::Digit1, "Digit1", "1"),
    (KeyCode::Digit2, "Digit2", "2"),
    (KeyCode::Digit3, "Digit3", "3"),
    (KeyCode::Digit4, "Digit4", "4"),
    (KeyCode::Digit5, "Digit5", "5"),
    (KeyCode::Digit6, "Digit6", "6"),
    (KeyCode::Digit7, "Digit7", "7"),
    (KeyCode::Digit8, "Digit8", "8"),
    (KeyCode::Digit9, "Digit9", "9"),
    (KeyCode::Space, "Space", "Space"),
    (KeyCode::Enter, "Enter", "Enter"),
    (KeyCode::Tab, "Tab", "Tab"),
    (KeyCode::Backspace, "Backspace", "Delete"),
    (KeyCode::Delete, "Delete", "Del"),
    (KeyCode::ShiftLeft, "Shift", "Shift"),
    (KeyCode::ControlLeft, "Control", "Ctrl"),
    (KeyCode::AltLeft, "Alt", "Option"),
    (KeyCode::ArrowUp, "ArrowUp", "Up"),
    (KeyCode::ArrowDown, "ArrowDown", "Down"),
    (KeyCode::ArrowLeft, "ArrowLeft", "Left"),
    (KeyCode::ArrowRight, "ArrowRight", "Right"),
    (KeyCode::Backquote, "Backquote", "`"),
    (KeyCode::Minus, "Minus", "-"),
    (KeyCode::Equal, "Equal", "="),
    (KeyCode::BracketLeft, "BracketLeft", "["),
    (KeyCode::BracketRight, "BracketRight", "]"),
    (KeyCode::Backslash, "Backslash", "\\"),
    (KeyCode::Semicolon, "Semicolon", ";"),
    (KeyCode::Quote, "Quote", "'"),
    (KeyCode::Comma, "Comma", ","),
    (KeyCode::Period, "Period", "."),
    (KeyCode::Slash, "Slash", "/"),
    (KeyCode::Home, "Home", "Home"),
    (KeyCode::End, "End", "End"),
    (KeyCode::PageUp, "PageUp", "Page Up"),
    (KeyCode::PageDown, "PageDown", "Page Down"),
    (KeyCode::F1, "F1", "F1"),
    (KeyCode::F2, "F2", "F2"),
    (KeyCode::F5, "F5", "F5"),
    (KeyCode::F6, "F6", "F6"),
    (KeyCode::F7, "F7", "F7"),
    (KeyCode::F8, "F8", "F8"),
    (KeyCode::F9, "F9", "F9"),
    (KeyCode::F10, "F10", "F10"),
    (KeyCode::F11, "F11", "F11"),
    (KeyCode::F12, "F12", "F12"),
    (KeyCode::Numpad0, "Numpad0", "Pad 0"),
    (KeyCode::Numpad1, "Numpad1", "Pad 1"),
    (KeyCode::Numpad2, "Numpad2", "Pad 2"),
    (KeyCode::Numpad3, "Numpad3", "Pad 3"),
    (KeyCode::Numpad4, "Numpad4", "Pad 4"),
    (KeyCode::Numpad5, "Numpad5", "Pad 5"),
    (KeyCode::Numpad6, "Numpad6", "Pad 6"),
    (KeyCode::Numpad7, "Numpad7", "Pad 7"),
    (KeyCode::Numpad8, "Numpad8", "Pad 8"),
    (KeyCode::Numpad9, "Numpad9", "Pad 9"),
    (KeyCode::NumpadAdd, "NumpadAdd", "Pad +"),
    (KeyCode::NumpadSubtract, "NumpadSubtract", "Pad -"),
    (KeyCode::NumpadMultiply, "NumpadMultiply", "Pad *"),
    (KeyCode::NumpadDivide, "NumpadDivide", "Pad /"),
    (KeyCode::NumpadDecimal, "NumpadDecimal", "Pad ."),
];

/// Every trackpad (mouse) button a binding can use.
const BUTTONS: &[(MouseButton, &str, &str)] = &[
    (MouseButton::Left, "MouseLeft", "Click"),
    (MouseButton::Right, "MouseRight", "Right click"),
    (MouseButton::Middle, "MouseMiddle", "Middle click"),
    (MouseButton::Back, "MouseBack", "Mouse back"),
    (MouseButton::Forward, "MouseForward", "Mouse fwd"),
];

/// Keys that are always the pause key, macOS's command key, or dev keys.
const CMD_KEYS: [KeyCode; 3] = [KeyCode::SuperLeft, KeyCode::SuperRight, KeyCode::Meta];

/// The key a twin binds as: right Shift/Ctrl/Option as the left one, the
/// number pad's Enter as Enter.
pub(crate) fn canonical_key(key: KeyCode) -> KeyCode {
    match key {
        KeyCode::ShiftRight => KeyCode::ShiftLeft,
        KeyCode::ControlRight => KeyCode::ControlLeft,
        KeyCode::AltRight => KeyCode::AltLeft,
        KeyCode::NumpadEnter => KeyCode::Enter,
        other => other,
    }
}

/// A key's name on screen ("?" for a key that can't be bound; Esc is "Esc").
pub(crate) fn key_display_name(key: KeyCode) -> &'static str {
    let key = canonical_key(key);
    if key == KeyCode::Escape {
        return "Esc";
    }
    KEYS.iter()
        .find(|(k, _, _)| *k == key)
        .map_or("?", |(_, _, name)| name)
}

/// The other key that counts as `key` (see [`canonical_key`]).
fn twin(key: KeyCode) -> Option<KeyCode> {
    match key {
        KeyCode::ShiftLeft => Some(KeyCode::ShiftRight),
        KeyCode::ControlLeft => Some(KeyCode::ControlRight),
        KeyCode::AltLeft => Some(KeyCode::AltRight),
        KeyCode::Enter => Some(KeyCode::NumpadEnter),
        _ => None,
    }
}

impl Binding {
    /// The binding as it's stored: a right-hand twin as the left key.
    pub fn canonical(self) -> Self {
        match self {
            Binding::Key(key) => Binding::Key(canonical_key(key)),
            mouse => mouse,
        }
    }

    /// Its name on screen ("W", "Shift", "Click").
    pub fn name(self) -> &'static str {
        match self.canonical() {
            Binding::Key(key) => key_display_name(key),
            Binding::Mouse(button) => BUTTONS
                .iter()
                .find(|(b, _, _)| *b == button)
                .map_or("?", |(_, _, name)| name),
        }
    }

    /// Its id in `settings.json`.
    pub fn id(self) -> Option<&'static str> {
        match self.canonical() {
            Binding::Key(key) => KEYS.iter().find(|(k, _, _)| *k == key).map(|e| e.1),
            Binding::Mouse(button) => BUTTONS.iter().find(|(b, _, _)| *b == button).map(|e| e.1),
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        KEYS.iter()
            .find(|(_, i, _)| *i == id)
            .map(|(k, _, _)| Binding::Key(*k))
            .or_else(|| {
                BUTTONS
                    .iter()
                    .find(|(_, i, _)| *i == id)
                    .map(|(b, _, _)| Binding::Mouse(*b))
            })
    }

    /// Held (either twin).
    pub fn pressed(self, keys: &ButtonInput<KeyCode>, mouse: &ButtonInput<MouseButton>) -> bool {
        match self {
            Binding::Key(key) => keys.pressed(key) || twin(key).is_some_and(|t| keys.pressed(t)),
            Binding::Mouse(button) => mouse.pressed(button),
        }
    }

    /// Pressed this frame (either twin).
    pub fn just_pressed(
        self,
        keys: &ButtonInput<KeyCode>,
        mouse: &ButtonInput<MouseButton>,
    ) -> bool {
        match self {
            Binding::Key(key) => {
                keys.just_pressed(key) || twin(key).is_some_and(|t| keys.just_pressed(t))
            }
            Binding::Mouse(button) => mouse.just_pressed(button),
        }
    }

    /// Whether this can be bound at all, and why not.
    pub fn check(self) -> Result<(), BindError> {
        match self.canonical() {
            Binding::Key(key) if CMD_KEYS.contains(&key) => Err(BindError::Cmd),
            Binding::Key(KeyCode::Escape) => Err(BindError::Reserved("Esc pauses")),
            Binding::Key(KeyCode::F3 | KeyCode::F4) => {
                Err(BindError::Reserved("F3 and F4 are the stats and tuning keys"))
            }
            b if b.id().is_some() => Ok(()),
            _ => Err(BindError::Unsupported),
        }
    }
}

/// Why a binding was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindError {
    /// Cmd, or a key pressed with Cmd (macOS shortcuts: Cmd-Q, Cmd-Tab...).
    Cmd,
    /// A key with a fixed job (Esc pauses; F3/F4 are dev keys).
    Reserved(&'static str),
    /// A key the game can't name (Caps Lock, Fn, media keys...).
    Unsupported,
}

impl BindError {
    /// What the Controls page says.
    pub fn message(self) -> &'static str {
        match self {
            BindError::Cmd => "Cmd shortcuts can't be bound",
            BindError::Reserved(why) => why,
            BindError::Unsupported => "That key can't be bound",
        }
    }
}

/// One binding per action. The default is D43's layout. Saved in
/// `settings.json` as `{ "jump": "Space", "fire": "MouseLeft", ... }`; a
/// missing or unknown entry falls back to its default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    from = "BTreeMap<String, String>",
    into = "BTreeMap<String, String>"
)]
pub struct Bindings([Binding; Action::COUNT]);

impl Default for Bindings {
    fn default() -> Self {
        Self(Action::ALL.map(Action::default_binding))
    }
}

impl Bindings {
    pub fn get(&self, action: Action) -> Binding {
        self.0[action as usize]
    }

    /// The bound input's name ("Enter", "Click"), for hints.
    pub fn name(&self, action: Action) -> &'static str {
        self.get(action).name()
    }

    /// The action bound to `binding` (or its twin), if any.
    pub fn action_for(&self, binding: Binding) -> Option<Action> {
        let binding = binding.canonical();
        Action::ALL.into_iter().find(|a| self.get(*a) == binding)
    }

    /// Binds `action` to `binding`. If another action had it, the two swap:
    /// that action takes `action`'s old binding (returned). Refused bindings
    /// change nothing.
    pub fn bind(&mut self, action: Action, binding: Binding) -> Result<Option<Action>, BindError> {
        binding.check()?;
        let binding = binding.canonical();
        let old = self.get(action);
        if old == binding {
            return Ok(None);
        }
        let other = self.action_for(binding);
        self.0[action as usize] = binding;
        if let Some(other) = other {
            self.0[other as usize] = old;
        }
        Ok(other)
    }

    /// Back to D43's layout.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Held this frame.
    pub fn pressed(
        &self,
        action: Action,
        keys: &ButtonInput<KeyCode>,
        mouse: &ButtonInput<MouseButton>,
    ) -> bool {
        self.get(action).pressed(keys, mouse)
    }

    /// Pressed this frame.
    pub fn just_pressed(
        &self,
        action: Action,
        keys: &ButtonInput<KeyCode>,
        mouse: &ButtonInput<MouseButton>,
    ) -> bool {
        self.get(action).just_pressed(keys, mouse)
    }
}

impl From<BTreeMap<String, String>> for Bindings {
    fn from(file: BTreeMap<String, String>) -> Self {
        let mut bindings = Self::default();
        for (action, binding) in &file {
            if let (Some(action), Some(binding)) =
                (Action::from_id(action), Binding::from_id(binding))
                && binding.check().is_ok()
            {
                bindings.0[action as usize] = binding.canonical();
            }
        }
        bindings
    }
}

impl From<Bindings> for BTreeMap<String, String> {
    fn from(bindings: Bindings) -> Self {
        Action::ALL
            .into_iter()
            .filter_map(|a| {
                let id = bindings.get(a).id()?;
                Some((a.id().to_string(), id.to_string()))
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Capturing a new binding (the Controls page)
// ---------------------------------------------------------------------------

/// The Controls page's request for a new binding, and how the last one went.
/// The page sets `waiting`; the input adapter reads the next key or click and
/// clears it.
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct BindingCapture {
    pub waiting: Option<Action>,
    pub last: Option<CaptureOutcome>,
}

/// How a capture ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureOutcome {
    /// `action` is now on `binding`; `swapped` took `action`'s old binding.
    Bound {
        action: Action,
        binding: Binding,
        swapped: Option<Action>,
    },
    /// Esc: nothing changed.
    Cancelled(Action),
    /// The input can't be bound; nothing changed.
    Refused(Action, BindError),
}

/// What the player pressed while a capture waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Captured {
    /// Esc cancels.
    Cancel,
    /// A key pressed with Cmd held (or Cmd itself).
    CmdCombo(Binding),
    /// A key or click.
    Input(Binding),
}

/// The first key or button pressed this frame, if any (Esc first).
pub fn captured_input(
    keys: &ButtonInput<KeyCode>,
    mouse: &ButtonInput<MouseButton>,
) -> Option<Captured> {
    if keys.just_pressed(KeyCode::Escape) {
        return Some(Captured::Cancel);
    }
    let cmd = keys.any_pressed(CMD_KEYS);
    let pressed = keys
        .get_just_pressed()
        .copied()
        .map(Binding::Key)
        .chain(mouse.get_just_pressed().copied().map(Binding::Mouse))
        // A key's press edge before a click's in the same frame, but prefer
        // any non-Cmd key over Cmd itself (Cmd-K reports both).
        .min_by_key(|b| matches!(b, Binding::Key(k) if CMD_KEYS.contains(k)))?;
    Some(if cmd {
        Captured::CmdCombo(pressed)
    } else {
        Captured::Input(pressed)
    })
}

/// Applies a capture to `bindings`: how it went.
pub fn apply_capture(bindings: &mut Bindings, action: Action, captured: Captured) -> CaptureOutcome {
    match captured {
        Captured::Cancel => CaptureOutcome::Cancelled(action),
        Captured::CmdCombo(_) => CaptureOutcome::Refused(action, BindError::Cmd),
        Captured::Input(binding) => match bindings.bind(action, binding) {
            Ok(swapped) => CaptureOutcome::Bound {
                action,
                binding: binding.canonical(),
                swapped,
            },
            Err(e) => CaptureOutcome::Refused(action, e),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_binding_has_a_name_and_round_trips() {
        let b = Bindings::default();
        for a in Action::ALL {
            let binding = b.get(a);
            assert!(binding.check().is_ok(), "{a:?}");
            assert_ne!(binding.name(), "?");
            assert_eq!(Binding::from_id(binding.id().unwrap()), Some(binding));
        }
        assert_eq!(Action::ALL.len(), Action::COUNT);
        for (i, a) in Action::ALL.iter().enumerate() {
            assert_eq!(*a as usize, i);
        }
    }

    #[test]
    fn twins_bind_as_one_key() {
        assert_eq!(Binding::Key(KeyCode::ShiftRight).name(), "Shift");
        assert_eq!(Binding::Key(KeyCode::NumpadEnter).name(), "Enter");
        let b = Bindings::default();
        assert_eq!(
            b.action_for(Binding::Key(KeyCode::ShiftRight)),
            Some(Action::Aim)
        );
    }
}
