//! `zxbevy` — the emulator with a window on it.
//!
//! ```sh
//! cargo run --release --features ui --bin zxbevy
//! ```
//!
//! The Bevy layer adds no emulator capability. It paces `Machine::run_frame` against real
//! time, uploads what `screen::render` produces, and turns host key events into the eight
//! bytes of the key matrix — every one of which `zxheadless` already does without a
//! window. See [`doc/ui.md`](../../doc/ui.md).

use bevy::asset::RenderAssetUsages;
use bevy::image::ImagePlugin;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use bevy::window::{PrimaryWindow, WindowResolution};

use on_the_spectrum::spectrum::keyboard::{Chord, Key};
use on_the_spectrum::spectrum::{Machine, screen};

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
        .insert_resource(Emulator {
            machine: Machine::new(&rom),
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
fn keyboard(mut emu: ResMut<Emulator>, keys: Res<ButtonInput<KeyCode>>) {
    emu.machine.bus.keyboard.release_all();
    for code in keys.get_pressed() {
        if let Some(chord) = chord_of(*code) {
            emu.machine.bus.keyboard.press(chord);
        }
    }
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
        Comma => return Some(Chord::symbol(Key::named("N")?)),
        Period => return Some(Chord::symbol(Key::named("M")?)),
        Semicolon => return Some(Chord::symbol(Key::named("O")?)),
        Quote => return Some(Chord::symbol(Key::named("P")?)),
        Minus => return Some(Chord::symbol(Key::named("J")?)),
        Equal => return Some(Chord::symbol(Key::named("L")?)),
        Slash => return Some(Chord::symbol(Key::named("V")?)),
        CapsLock => return Some(Chord::caps(Key::named("2")?)),
        _ => return None,
    };
    Some(Chord::plain(Key::named(name)?))
}
