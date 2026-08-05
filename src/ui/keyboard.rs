// =======================================================
// src/ui/keyboard.rs — C64 keyboard matrix mapping and state
// =======================================================

use winit::keyboard::KeyCode;

/* Host key codes map to physical C64 matrix coordinates rather than characters. Shifted legends therefore reuse the same switch position, while host-only controls return no matrix contact. */
pub fn key_to_matrix(key: KeyCode) -> Option<(u8, u8)> {
	match key {
		KeyCode::Backspace => Some((0, 0)),
		KeyCode::Digit3    => Some((0, 1)),
		KeyCode::Digit5    => Some((0, 2)),
		KeyCode::Digit7    => Some((0, 3)),
		KeyCode::Digit9    => Some((0, 4)),
		KeyCode::Minus     => Some((0, 5)),
		KeyCode::F9        => Some((0, 6)),
		KeyCode::Digit1    => Some((0, 7)),

		KeyCode::Enter     => Some((1, 0)),
		KeyCode::KeyW      => Some((1, 1)),
		KeyCode::KeyR      => Some((1, 2)),
		KeyCode::KeyY      => Some((1, 3)),
		KeyCode::KeyI      => Some((1, 4)),
		KeyCode::KeyP      => Some((1, 5)),
		KeyCode::BracketRight => Some((1, 6)),
		KeyCode::Backquote => Some((1, 7)),

		KeyCode::ArrowRight => Some((2, 0)),
		KeyCode::KeyA      => Some((2, 1)),
		KeyCode::KeyD      => Some((2, 2)),
		KeyCode::KeyG      => Some((2, 3)),
		KeyCode::KeyJ      => Some((2, 4)),
		KeyCode::KeyL      => Some((2, 5)),
		KeyCode::Quote     => Some((2, 6)),

		KeyCode::Tab       => None,

		KeyCode::F7        => Some((3, 0)),
		KeyCode::F8        => Some((3, 0)),
		KeyCode::Digit4    => Some((3, 1)),
		KeyCode::Digit6    => Some((3, 2)),
		KeyCode::Digit8    => Some((3, 3)),
		KeyCode::Digit0    => Some((3, 4)),
		KeyCode::Equal     => Some((3, 5)),
		KeyCode::F10       => Some((3, 6)),
		KeyCode::Digit2    => Some((3, 7)),

		KeyCode::F1        => Some((4, 0)),
		KeyCode::F2        => Some((4, 0)),
		KeyCode::KeyZ      => Some((4, 1)),
		KeyCode::KeyC      => Some((4, 2)),
		KeyCode::KeyB      => Some((4, 3)),
		KeyCode::KeyM      => Some((4, 4)),
		KeyCode::Period    => Some((4, 5)),
		KeyCode::ShiftRight => Some((4, 6)),
		KeyCode::Space     => Some((4, 7)),

		KeyCode::F3        => Some((5, 0)),
		KeyCode::F4        => Some((5, 0)),
		KeyCode::KeyS      => Some((5, 1)),
		KeyCode::KeyF      => Some((5, 2)),
		KeyCode::KeyH      => Some((5, 3)),
		KeyCode::KeyK      => Some((5, 4)),
		KeyCode::Semicolon => Some((5, 5)),
		KeyCode::AltRight  => Some((5, 6)),
		KeyCode::ControlLeft => Some((5, 7)),

		KeyCode::F5        => Some((6, 0)),
		KeyCode::F6        => Some((6, 0)),
		KeyCode::KeyE      => Some((6, 1)),
		KeyCode::KeyT      => Some((6, 2)),
		KeyCode::KeyU      => Some((6, 3)),
		KeyCode::KeyO      => Some((6, 4)),
		KeyCode::BracketLeft => Some((6, 5)),
		KeyCode::Backslash => Some((6, 6)),
		KeyCode::KeyQ      => Some((6, 7)),

		KeyCode::ArrowDown => Some((7, 0)),
		KeyCode::ShiftLeft => Some((7, 1)),
		KeyCode::KeyX      => Some((7, 2)),
		KeyCode::KeyV      => Some((7, 3)),
		KeyCode::KeyN      => Some((7, 4)),
		KeyCode::Comma     => Some((7, 5)),
		KeyCode::Slash     => Some((7, 6)),
		KeyCode::Escape    => Some((7, 7)),

		KeyCode::Home      => Some((3, 6)),
		KeyCode::Delete    => Some((0, 0)),
		_ => None,
	}
}