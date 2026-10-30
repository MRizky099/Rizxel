//! egui side of the keymap: turns key presses into [`ActionId`]s.

use egui::{Event, Key, Modifiers};
use rizxel_app::{ActionId, KeyChord, Keymap};

/// The egui key of a keymap key name.
pub fn egui_key(name: &str) -> Option<Key> {
    Key::from_name(name)
}

/// The chord of a key press, `None` for keys the keymap doesn't know.
pub fn chord_of(key: Key, modifiers: Modifiers) -> Option<KeyChord> {
    KeyChord::new(
        modifiers.command,
        modifiers.shift,
        modifiers.alt,
        key.name(),
    )
}

fn matches(chord: &KeyChord, key: Key, modifiers: Modifiers) -> bool {
    chord_of(key, modifiers).as_ref() == Some(chord)
}

/// Actions handled by the viewport (they need the hovered voxel), left in
/// the input for it by [`take_actions`].
pub fn viewport_action(action: ActionId) -> bool {
    matches!(action, ActionId::SliceMode | ActionId::Delete)
}

/// Removes this frame's key presses bound to an action and returns the
/// actions in press order. Key repeats count only for repeating actions;
/// viewport actions and unbound keys stay in the input.
pub fn take_actions(input: &mut egui::InputState, keymap: &Keymap) -> Vec<ActionId> {
    let mut actions = Vec::new();
    input.events.retain(|e| {
        let Event::Key {
            key,
            pressed: true,
            repeat,
            modifiers,
            ..
        } = e
        else {
            return true;
        };
        let Some(action) = chord_of(*key, *modifiers).and_then(|c| keymap.action_for(&c)) else {
            return true;
        };
        if viewport_action(action) || action.is_fixed() {
            return true;
        }
        if !*repeat || action.repeats() {
            actions.push(action);
        }
        false
    });
    actions
}

/// Consumes a press of one of `action`'s chords (viewport actions).
pub fn consume(input: &mut egui::InputState, keymap: &Keymap, action: ActionId) -> bool {
    let chords = keymap.chords(action);
    let mut hit = false;
    input.events.retain(|e| match e {
        Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } if !hit && chords.iter().any(|c| matches(c, *key, *modifiers)) => {
            hit = true;
            false
        }
        _ => true,
    });
    hit
}

/// The next key pressed this frame as a chord (keymap editor capture).
pub fn captured_chord(input: &egui::InputState) -> Option<(Key, Modifiers)> {
    input.events.iter().find_map(|e| match e {
        Event::Key {
            key,
            pressed: true,
            repeat: false,
            modifiers,
            ..
        } if !is_modifier(*key) => Some((*key, *modifiers)),
        _ => None,
    })
}

/// A modifier on its own: egui sends these as key presses too, before the
/// key they modify, so a capture waits for the key that follows.
fn is_modifier(key: Key) -> bool {
    matches!(
        key,
        Key::ShiftLeft
            | Key::ShiftRight
            | Key::ControlLeft
            | Key::ControlRight
            | Key::AltLeft
            | Key::AltRight
            | Key::SuperLeft
            | Key::SuperRight
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_keymap_key_is_an_egui_key() {
        for name in rizxel_app::KEY_NAMES {
            let key = egui_key(name).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(key.name(), *name, "{name}");
        }
    }

    fn press(key: Key, modifiers: Modifiers, repeat: bool) -> Event {
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat,
            modifiers,
        }
    }

    #[test]
    fn capture_waits_for_the_key_after_the_modifiers() {
        let mut input = egui::InputState::default();
        input.events = vec![
            press(Key::ControlLeft, Modifiers::COMMAND, false),
            press(Key::ShiftLeft, Modifiers::COMMAND | Modifiers::SHIFT, false),
        ];
        assert_eq!(captured_chord(&input), None);
        input
            .events
            .push(press(Key::U, Modifiers::COMMAND | Modifiers::SHIFT, false));
        assert_eq!(
            captured_chord(&input),
            Some((Key::U, Modifiers::COMMAND | Modifiers::SHIFT))
        );
    }

    #[test]
    fn presses_map_to_actions() {
        let keymap = Keymap::default();
        let mut input = egui::InputState::default();
        input.events = vec![
            press(Key::S, Modifiers::COMMAND | Modifiers::SHIFT, false),
            press(Key::B, Modifiers::NONE, false),
            press(Key::G, Modifiers::NONE, true),
            press(Key::Z, Modifiers::COMMAND, true),
            press(Key::Space, Modifiers::NONE, false),
            press(Key::Q, Modifiers::NONE, false),
        ];
        let actions = take_actions(&mut input, &keymap);
        assert_eq!(
            actions,
            vec![
                ActionId::SaveAs,
                ActionId::Tool(rizxel_engine::Tool::Brush),
                ActionId::Undo,
            ]
        );
        // Space (viewport) and the unbound Q stay for the viewport and egui.
        assert_eq!(input.events.len(), 2);
        assert!(consume(&mut input, &keymap, ActionId::SliceMode));
        assert_eq!(input.events.len(), 1);
    }
}
