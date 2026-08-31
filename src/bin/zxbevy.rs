//! `zxbevy` — the emulator with a window on it.
//!
//! ```sh
//! cargo run --release --features ui --bin zxbevy
//! cargo run --release --features ui --bin zxbevy -- game.z80
//! ```
//!
//! The Bevy layer adds no emulator capability. It paces `Machine::run_frame` against real
//! time, uploads what `screen::render` produces, and turns host key events into the eight
//! bytes of the key matrix — every one of which `zxheadless` already does without a
//! window. See [`doc/ui.md`](../../doc/ui.md).

use bevy::asset::RenderAssetUsages;
use bevy::image::ImagePlugin;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::{PrimaryWindow, WindowResolution};

use on_the_spectrum::spectrum::keyboard::{Chord, Key};
use on_the_spectrum::spectrum::{Machine, screen, snapshot};

const ROM: &str = "roms/48.rom";
/// 69888 T-states at 3.5 MHz — 19.968 ms, which is neither 20 ms nor the host's refresh
/// rate, and the reason emulation is paced by an accumulator rather than by frames drawn.
const FRAME_SECONDS: f64 = 69888.0 / 3_500_000.0;
/// Give up catching up beyond this, so that dragging the window does not cause a
/// multi-second fast-forward on release.
const MAX_CATCH_UP: f64 = FRAME_SECONDS * 4.0;
const INITIAL_SCALE: u32 = 3;

#[derive(Resource)]
struct Emulator {
    machine: Machine,
    /// Seconds of emulated time owed.
    accumulator: f64,
    /// 1.0 is real time; 0.0 is paused.
    speed: f64,
}

/// What each held host key produced as text.
///
/// A symbol has to be looked up by the character the host layout actually made, not by
/// where the key sits: `+` is Shift and `=` on this keyboard and SYM SHIFT and `K` on a
/// Spectrum, and the two have no key in common.
#[derive(Resource, Default)]
struct HostText(HashMap<KeyCode, char>);

#[derive(Resource)]
struct Screen {
    image: Handle<Image>,
    /// The frame is rendered here and then copied in, so the render never has to care
    /// about how Bevy is storing the texture.
    frame: Vec<u8>,
}

fn main() -> AppExit {
    let rom = match std::fs::read(ROM) {
        Ok(rom) => rom,
        Err(e) => {
            eprintln!("zxbevy: {ROM}: {e}");
            return AppExit::error();
        }
    };

    let mut machine = Machine::new(&rom);
    // One argument, and it is a snapshot to start from rather than a cold boot.
    if let Some(path) = std::env::args().nth(1) {
        match snapshot::load_path(&mut machine, std::path::Path::new(&path)) {
            Ok(()) => println!("zxbevy: loaded {path}"),
            Err(e) => {
                eprintln!("zxbevy: {path}: {e}");
                return AppExit::error();
            }
        }
    }

    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "on the spectrum".into(),
                        resolution: WindowResolution::new(
                            screen::WIDTH as u32 * INITIAL_SCALE,
                            screen::HEIGHT as u32 * INITIAL_SCALE,
                        ),
                        ..default()
                    }),
                    ..default()
                })
                // Nearest sampling, or a 1-pixel stroke of the Spectrum's font turns to
                // mud the moment the window is not at an exact multiple.
                .set(ImagePlugin::default_nearest()),
        )
        .insert_resource(ClearColor(Color::BLACK))
        .init_resource::<HostText>()
        .insert_resource(Emulator {
            machine,
            accumulator: 0.0,
            speed: 1.0,
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (keyboard, run_emulation, blit, screenshot).chain())
        .run()
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let image = images.add(Image::new_fill(
        Extent3d {
            width: screen::WIDTH as u32,
            height: screen::HEIGHT as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 0xFF],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    ));

    commands.spawn(Camera2d);
    commands.spawn(Sprite::from_image(image.clone()));
    commands.insert_resource(Screen {
        image,
        frame: vec![0; screen::FRAME_BYTES],
    });
}

/// Host keys in, eight matrix bytes out. Nothing else: the ROM does its own debouncing,
/// auto-repeat and decoding on the 50 Hz interrupt, which is why the single `P` key gives
/// the whole `PRINT` keyword at a fresh prompt.
///
/// Letters and digits are taken by **position**, so the layout is predictable whatever the
/// host locale is; punctuation is taken by the **character produced**, because that is the
/// only thing the two keyboards agree on.
fn keyboard(
    mut emu: ResMut<Emulator>,
    mut host: ResMut<HostText>,
    mut input: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    for event in input.read() {
        match event.state {
            ButtonState::Pressed => {
                if let Some(c) = event.text.as_ref().and_then(|t| t.chars().next()) {
                    host.0.insert(event.key_code, c);
                }
            }
            ButtonState::Released => {
                host.0.remove(&event.key_code);
            }
        }
    }

    let held: Vec<(KeyCode, Option<char>)> = keys
        .get_pressed()
        .map(|code| (*code, host.0.get(code).copied()))
        .collect();

    let keyboard = &mut emu.machine.bus.keyboard;
    keyboard.release_all();
    for chord in resolve(&held) {
        keyboard.press(chord);
    }
}

/// The chords a set of held host keys makes, given what each of them typed.
///
/// Pure, so the awkward cases can be tested without a window in the way.
fn resolve(held: &[(KeyCode, Option<char>)]) -> Vec<Chord> {
    let mut chords: Vec<(KeyCode, Chord, bool)> = Vec::new();
    let mut symbol_held = false;

    for &(code, text) in held {
        let as_symbol = text
            .filter(|c| !c.is_ascii_alphanumeric() && *c != ' ')
            .and_then(Chord::for_char);
        if let Some(chord) = as_symbol {
            chords.push((code, chord, true));
            symbol_held = true;
        } else if let Some(chord) = chord_of(code) {
            chords.push((code, chord, false));
        } else if let Some(chord) = text.and_then(Chord::for_char) {
            // The numeric keypad, and anything else with no position of its own.
            chords.push((code, chord, false));
        }
    }

    chords
        .into_iter()
        .filter(|&(code, _, from_symbol)| {
            // The host shift that made `+` was spent making it. Pressing CAPS SHIFT as
            // well would put the machine into extended mode instead.
            !(symbol_held && !from_symbol && is_host_shift(code))
        })
        .map(|(_, chord, _)| chord)
        .collect()
}

fn is_host_shift(code: KeyCode) -> bool {
    matches!(
        code,
        KeyCode::ShiftLeft
            | KeyCode::ShiftRight
            | KeyCode::ControlLeft
            | KeyCode::ControlRight
            | KeyCode::AltLeft
    )
}

fn run_emulation(mut emu: ResMut<Emulator>, time: Res<Time>) {
    if emu.speed == 0.0 {
        return;
    }
    emu.accumulator += time.delta_secs_f64() * emu.speed;
    emu.accumulator = emu.accumulator.min(MAX_CATCH_UP);
    while emu.accumulator >= FRAME_SECONDS {
        emu.machine.run_frame();
        emu.accumulator -= FRAME_SECONDS;
    }
}

/// Render the frame, upload it, and size the sprite to a whole number of screen pixels.
fn blit(
    emu: Res<Emulator>,
    mut screen_res: ResMut<Screen>,
    mut images: ResMut<Assets<Image>>,
    mut sprites: Query<&mut Sprite>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let Screen { image, frame } = &mut *screen_res;
    emu.machine.render_into(frame);
    if let Some(mut texture) = images.get_mut(&*image) {
        match &mut texture.data {
            Some(data) => data.copy_from_slice(frame),
            none => *none = Some(frame.clone()),
        }
    }

    // Integer scaling only. A fractional factor puts a 1-pixel stroke across two output
    // pixels at uneven weights, and the Spectrum's font is nothing but 1-pixel strokes.
    let Ok(window) = windows.single() else { return };
    let scale = (window.resolution.width() as u32 / screen::WIDTH as u32)
        .min(window.resolution.height() as u32 / screen::HEIGHT as u32)
        .max(1);
    let size = Vec2::new(
        (screen::WIDTH as u32 * scale) as f32,
        (screen::HEIGHT as u32 * scale) as f32,
    );
    for mut sprite in &mut sprites {
        if sprite.custom_size != Some(size) {
            sprite.custom_size = Some(size);
        }
    }
}

/// `ZXBEVY_SCREENSHOT=path` boots for a couple of seconds, saves what the GPU actually
/// drew, and quits. It is how the window gets checked from a terminal — there is no other
/// way to know that what the renderer produced is what the screen is showing.
fn screenshot(mut commands: Commands, mut frames: Local<u32>, mut exit: MessageWriter<AppExit>) {
    let Ok(path) = std::env::var("ZXBEVY_SCREENSHOT") else {
        return;
    };
    *frames += 1;
    // Long enough for the ROM to have booted to the copyright message.
    match *frames {
        150 => {
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path));
        }
        180 => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}

/// The host-key mapping from [`doc/spectrum-keyboard.md`](../../doc/spectrum-keyboard.md).
///
/// Physical key *positions*, not text input, so the layout is the same whatever the host
/// locale is set to.
fn chord_of(code: KeyCode) -> Option<Chord> {
    use KeyCode::*;
    let name = match code {
        KeyA => "A",
        KeyB => "B",
        KeyC => "C",
        KeyD => "D",
        KeyE => "E",
        KeyF => "F",
        KeyG => "G",
        KeyH => "H",
        KeyI => "I",
        KeyJ => "J",
        KeyK => "K",
        KeyL => "L",
        KeyM => "M",
        KeyN => "N",
        KeyO => "O",
        KeyP => "P",
        KeyQ => "Q",
        KeyR => "R",
        KeyS => "S",
        KeyT => "T",
        KeyU => "U",
        KeyV => "V",
        KeyW => "W",
        KeyX => "X",
        KeyY => "Y",
        KeyZ => "Z",
        Digit0 => "0",
        Digit1 => "1",
        Digit2 => "2",
        Digit3 => "3",
        Digit4 => "4",
        Digit5 => "5",
        Digit6 => "6",
        Digit7 => "7",
        Digit8 => "8",
        Digit9 => "9",
        Enter | NumpadEnter => "ENTER",
        Space => "SPACE",
        ShiftLeft => "CAPS SHIFT",
        ShiftRight | ControlLeft | ControlRight | AltLeft => "SYM SHIFT",
        // The synthesised chords. A real Spectrum has no dedicated key for any of these.
        Backspace => return Some(Chord::caps(Key::named("0")?)),
        ArrowLeft => return Some(Chord::caps(Key::named("5")?)),
        ArrowDown => return Some(Chord::caps(Key::named("6")?)),
        ArrowUp => return Some(Chord::caps(Key::named("7")?)),
        ArrowRight => return Some(Chord::caps(Key::named("8")?)),
        Escape => return Some(Chord::caps(Key::SPACE)),
        CapsLock => return Some(Chord::caps(Key::named("2")?)),
        _ => return None,
    };
    Some(Chord::plain(Key::named(name)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use on_the_spectrum::spectrum::Machine;

    fn chords(held: &[(KeyCode, Option<char>)]) -> Vec<Chord> {
        resolve(held)
    }

    #[test]
    fn a_symbol_uses_its_own_shift_and_not_the_host_s() {
        // `+` is Shift and `=` here, and SYM SHIFT and `K` there.
        let plus = chords(&[(KeyCode::ShiftLeft, None), (KeyCode::Equal, Some('+'))]);
        assert_eq!(plus, vec![Chord::symbol(Key::named("K").unwrap())]);

        // ...and the keypad's `+` needs no shift at all to say the same thing.
        let keypad = chords(&[(KeyCode::NumpadAdd, Some('+'))]);
        assert_eq!(keypad, vec![Chord::symbol(Key::named("K").unwrap())]);
    }

    #[test]
    fn a_shifted_letter_is_still_caps_shift() {
        let a = chords(&[(KeyCode::ShiftLeft, None), (KeyCode::KeyA, Some('A'))]);
        assert_eq!(
            a,
            vec![
                Chord::plain(Key::CAPS_SHIFT),
                Chord::plain(Key::named("A").unwrap())
            ]
        );
    }

    #[test]
    fn the_symbol_shift_key_still_works_by_position() {
        // Ctrl and K produces no text, so it falls through to the positions.
        let plus = chords(&[(KeyCode::ControlLeft, None), (KeyCode::KeyK, None)]);
        assert_eq!(
            plus,
            vec![
                Chord::plain(Key::SYM_SHIFT),
                Chord::plain(Key::named("K").unwrap())
            ]
        );
    }

    /// One host keypress: the keys held down, and the character it should reach the
    /// Spectrum's edit line as.
    type Case = (&'static [(KeyCode, Option<char>)], u8);

    /// The whole path, on a real machine: hold what the host would hold, and see the
    /// character land in the edit line.
    #[test]
    fn the_punctuation_a_host_keyboard_can_reach_all_arrives() {
        let rom = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/roms/48.rom")).unwrap();
        let mut machine = Machine::new(&rom);
        assert!(machine.run_until_pc(0x12A9, 12_000_000), "never booted");

        // (host keys held, what the Spectrum should end up with)
        let cases: &[Case] = &[
            (
                &[(KeyCode::ShiftLeft, None), (KeyCode::Equal, Some('+'))],
                b'+',
            ),
            (&[(KeyCode::Minus, Some('-'))], b'-'),
            (
                &[(KeyCode::ShiftLeft, None), (KeyCode::Digit8, Some('*'))],
                b'*',
            ),
            (&[(KeyCode::Slash, Some('/'))], b'/'),
            (
                &[(KeyCode::ShiftLeft, None), (KeyCode::Digit9, Some('('))],
                b'(',
            ),
            (&[(KeyCode::Quote, Some('\''))], b'\''),
            (
                &[(KeyCode::ShiftLeft, None), (KeyCode::Quote, Some('"'))],
                b'"',
            ),
            (&[(KeyCode::Comma, Some(','))], b','),
            (&[(KeyCode::Period, Some('.'))], b'.'),
            (&[(KeyCode::Semicolon, Some(';'))], b';'),
            (
                &[(KeyCode::ShiftLeft, None), (KeyCode::Semicolon, Some(':'))],
                b':',
            ),
            (&[(KeyCode::Equal, Some('='))], b'='),
        ];

        for &(held, expected) in cases {
            let before = machine.edit_line().len();
            for chord in resolve(held) {
                machine.bus.keyboard.press(chord);
            }
            machine.run_frames(3);
            machine.bus.keyboard.release_all();
            machine.run_frames(3);

            let line = machine.edit_line();
            assert_eq!(
                line.len(),
                before + 1,
                "{expected:?} typed nothing; the line reads {:?}",
                String::from_utf8_lossy(&line)
            );
            assert_eq!(
                *line.last().unwrap(),
                expected,
                "expected {:?}, got {:?}",
                expected as char,
                *line.last().unwrap() as char
            );
        }
    }
}
