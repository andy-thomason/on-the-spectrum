//! The startup menu: pick something to play, from disk or from the internet.
//!
//! It is drawn **on the Spectrum's own screen**, in the ROM's font, by writing into the
//! display file — see [`spectrum::text`]. So there is no second font, no text renderer and
//! nothing for the Bevy layer to do but display the frame it was going to display anyway,
//! and the whole thing can be checked headlessly by reading the screen back.
//!
//! The catalogue is fetched live from the Internet Archive's ZX Spectrum collection rather
//! than being a list checked into this repository: no URLs to rot, nothing of anyone
//! else's baked into the source, and the choice of what to download stays with whoever is
//! sitting in front of it. Downloads land in `games/`, which is not tracked.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Mutex, mpsc};

use bevy::prelude::*;

use on_the_spectrum::spectrum::{snapshot, text};

use crate::Emulator;

/// Where downloads go, and where local snapshots are looked for.
const GAMES_DIR: &str = "games";
const ENTRIES: usize = 10;

/// Black ink on white paper, as the ROM itself uses; inverted for the selected line;
/// blue for anything that is a note rather than a choice.
const NORMAL: u8 = 0x38;
const SELECTED: u8 = 0x78;
const NOTE: u8 = 0x39;
/// A white border, so the menu looks like something the machine would have drawn.
const BORDER: u8 = 7;

pub struct MenuPlugin;

impl Plugin for MenuPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Menu::new())
            .add_systems(Startup, start_catalogue_fetch)
            .add_systems(Update, (input, poll, draw).chain());
    }
}

enum Source {
    /// A file already on disk.
    Local(PathBuf),
    /// An Internet Archive item, whose snapshot has still to be found and fetched.
    Archive(String),
}

struct Entry {
    title: String,
    source: Source,
}

/// What a background thread has finished doing.
enum Job {
    Catalogue(Result<Vec<Entry>, String>),
    Snapshot(Result<PathBuf, String>),
}

#[derive(Resource)]
pub struct Menu {
    pub open: bool,
    entries: Vec<Entry>,
    selected: usize,
    status: String,
    /// Which page of the catalogue is showing; `more` steps through them.
    page: u32,
    /// A background thread is working, and nothing else should start.
    busy: bool,
    jobs: Option<Mutex<Receiver<Job>>>,
    /// The screen needs redrawing.
    dirty: bool,
}

impl Menu {
    fn new() -> Self {
        Menu {
            open: true,
            entries: local_entries(),
            selected: 0,
            status: "fetching the catalogue...".to_string(),
            page: 1,
            busy: false,
            jobs: None,
            dirty: true,
        }
    }

    fn close(&mut self) {
        self.open = false;
    }

    /// Run `work` on a thread and take its result in [`poll`].
    fn spawn<F>(&mut self, work: F)
    where
        F: FnOnce() -> Job + Send + 'static,
    {
        let (sender, receiver) = channel();
        self.jobs = Some(Mutex::new(receiver));
        self.busy = true;
        self.dirty = true;
        std::thread::spawn(move || {
            let _ = sender.send(work());
        });
    }
}

fn start_catalogue_fetch(mut menu: ResMut<Menu>) {
    let page = menu.page;
    menu.spawn(move || Job::Catalogue(fetch_catalogue(page)));
}

/// Arrow keys to choose, ENTER to play, `R` for another page, ESC to boot the ROM instead.
fn input(mut menu: ResMut<Menu>, mut emu: ResMut<Emulator>, keys: Res<ButtonInput<KeyCode>>) {
    if !menu.open {
        return;
    }
    let count = menu.entries.len();

    if keys.just_pressed(KeyCode::Escape) {
        menu.close();
        // The ROM has not run yet, so it will clear the menu off the screen itself.
        emu.speed = 1.0;
        return;
    }
    if count > 0 && keys.just_pressed(KeyCode::ArrowDown) {
        menu.selected = (menu.selected + 1) % count;
        menu.dirty = true;
    }
    if count > 0 && keys.just_pressed(KeyCode::ArrowUp) {
        menu.selected = (menu.selected + count - 1) % count;
        menu.dirty = true;
    }
    if !menu.busy && keys.just_pressed(KeyCode::KeyR) {
        menu.page += 1;
        let page = menu.page;
        menu.status = format!("fetching page {page}...");
        menu.spawn(move || Job::Catalogue(fetch_catalogue(page)));
    }
    // The digits are shortcuts for the first nine lines: pick and go.
    let digits = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    let digit = digits.iter().position(|&k| keys.just_pressed(k));
    if let Some(i) = digit.filter(|&i| i < count) {
        menu.selected = i;
        menu.dirty = true;
    }

    if !menu.busy && count > 0 && (keys.just_pressed(KeyCode::Enter) || digit.is_some()) {
        match &menu.entries[menu.selected].source {
            Source::Local(path) => {
                let path = path.clone();
                load(&mut menu, &mut emu, &path);
            }
            Source::Archive(id) => {
                let id = id.clone();
                menu.status = "downloading...".to_string();
                menu.spawn(move || Job::Snapshot(fetch_snapshot(&id)));
            }
        }
    }
}

/// Take whatever a background thread finished, without ever blocking on it.
fn poll(mut menu: ResMut<Menu>, mut emu: ResMut<Emulator>) {
    let received = menu
        .jobs
        .as_ref()
        .and_then(|jobs| jobs.lock().ok().map(|receiver| receiver.try_recv()));

    let job = match received {
        None | Some(Err(mpsc::TryRecvError::Empty)) => return,
        Some(Ok(job)) => job,
        // The thread went away without answering, which only a panic can do.
        Some(Err(mpsc::TryRecvError::Disconnected)) => {
            menu.jobs = None;
            menu.busy = false;
            menu.dirty = true;
            menu.status = "the fetch gave up".to_string();
            return;
        }
    };
    menu.jobs = None;
    menu.busy = false;
    menu.dirty = true;

    match job {
        Job::Catalogue(Ok(fetched)) => {
            let mut entries = local_entries();
            entries.extend(fetched);
            menu.selected = menu.selected.min(entries.len().saturating_sub(1));
            menu.entries = entries;
            menu.status = "ENTER to play".to_string();
        }
        Job::Catalogue(Err(e)) => {
            menu.status = if menu.entries.is_empty() {
                format!("no catalogue: {}", truncate(&e))
            } else {
                truncate(&e)
            };
        }
        Job::Snapshot(Ok(path)) => load(&mut menu, &mut emu, &path),
        Job::Snapshot(Err(e)) => menu.status = truncate(&e),
    }
}

fn load(menu: &mut Menu, emu: &mut Emulator, path: &Path) {
    match snapshot::load_path(&mut emu.machine, path) {
        Ok(()) => {
            menu.close();
            emu.speed = 1.0;
            println!("zxbevy: running {}", path.display());
        }
        Err(e) => {
            menu.status = truncate(&e.to_string());
            menu.dirty = true;
        }
    }
}

/// Draw the menu into the display file. Only when something has changed: it is 6912 bytes
/// of poking, and nothing moves between keypresses.
fn draw(mut menu: ResMut<Menu>, mut emu: ResMut<Emulator>) {
    if !menu.open || !menu.dirty {
        return;
    }
    menu.dirty = false;
    emu.machine.bus.ula.border = BORDER;
    let memory = &mut emu.machine.bus.memory;

    text::clear(memory, NORMAL);
    text::print_centred(memory, 0, " on the spectrum ", SELECTED);
    text::print_centred(memory, 2, "pick something to play", NOTE);

    for (i, entry) in menu.entries.iter().take(ENTRIES + 8).enumerate() {
        let row = 4 + i;
        if row > 18 {
            break;
        }
        let mark = match entry.source {
            Source::Local(_) => '*',
            Source::Archive(_) => ' ',
        };
        let title: String = entry.title.chars().take(28).collect();
        let attribute = if i == menu.selected { SELECTED } else { NORMAL };
        text::print_at(memory, 1, row, &format!("{mark}{title}"), attribute);
        if i == menu.selected {
            text::highlight_row(memory, row, SELECTED);
        }
    }

    text::print_at(memory, 0, 20, &menu.status, NOTE);
    text::print_at(memory, 0, 22, "1-9 or ENTER play, arrows", NOTE);
    text::print_at(memory, 0, 23, "R more, ESC boot the ROM", NOTE);
}

/// Snapshots already sitting in `games/`, so the menu works with no internet at all.
fn local_entries() -> Vec<Entry> {
    let Ok(dir) = std::fs::read_dir(GAMES_DIR) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = dir
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            matches!(
                p.extension().map(|e| e.to_string_lossy().to_lowercase()),
                Some(ref e) if e == "z80" || e == "sna"
            )
        })
        .map(|path| Entry {
            title: path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
            source: Source::Local(path),
        })
        .collect();
    entries.sort_by(|a, b| a.title.cmp(&b.title));
    entries
}

// ---------------------------------------------------------------------------- the network

/// One page of the Internet Archive's ZX Spectrum collection, most downloaded first.
fn fetch_catalogue(page: u32) -> Result<Vec<Entry>, String> {
    let url = format!(
        "https://archive.org/advancedsearch.php?q=collection%3Asoftwarelibrary_zx_spectrum\
         +AND+mediatype%3Asoftware&fl%5B%5D=identifier&fl%5B%5D=title&rows={ENTRIES}\
         &page={page}&sort%5B%5D=downloads+desc&output=json"
    );
    parse_catalogue(&get(&url)?)
}

/// Find the item's snapshot, download it into `games/`, and give back the path.
fn fetch_snapshot(id: &str) -> Result<PathBuf, String> {
    let metadata = get(&format!("https://archive.org/metadata/{id}"))?;
    let name = pick_snapshot(&metadata).ok_or_else(|| format!("{id} has no .z80 or .sna in it"))?;

    let target = Path::new(GAMES_DIR).join(&name);
    if target.exists() {
        return Ok(target);
    }
    std::fs::create_dir_all(GAMES_DIR).map_err(|e| e.to_string())?;

    let url = format!("https://archive.org/download/{id}/{}", encode(&name));
    let status = Command::new("curl")
        .args(["-sSLf", "--max-time", "120", "-o"])
        .arg(&target)
        .arg(&url)
        .status()
        .map_err(|e| format!("curl: {e}"))?;
    if !status.success() {
        let _ = std::fs::remove_file(&target);
        return Err(format!("download failed: {name}"));
    }
    Ok(target)
}

/// Fetch a URL as text. `curl` rather than an HTTP crate: the emulator has no other need
/// for one, and the UI is already the only part of this program with dependencies.
fn get(url: &str) -> Result<String, String> {
    let output = Command::new("curl")
        .args(["-sSLf", "--max-time", "30", url])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "request failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| e.to_string())
}

fn parse_catalogue(json: &str) -> Result<Vec<Entry>, String> {
    let value: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let docs = value["response"]["docs"]
        .as_array()
        .ok_or("no results in the reply")?;
    Ok(docs
        .iter()
        .filter_map(|doc| {
            let id = doc["identifier"].as_str()?.to_string();
            let title = doc["title"].as_str().unwrap_or(&id).to_string();
            Some(Entry {
                title,
                source: Source::Archive(id),
            })
        })
        .collect())
}

/// The first `.z80` in an item, or failing that a `.sna`. Nothing else is loadable yet.
fn pick_snapshot(metadata: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(metadata).ok()?;
    let files = value["files"].as_array()?;
    let named = |want: &str| {
        files.iter().find_map(|f| {
            let name = f["name"].as_str()?;
            Path::new(name)
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .filter(|e| e == want)
                .map(|_| name.to_string())
        })
    };
    named("z80").or_else(|| named("sna"))
}

/// Percent-encode a path segment. Archive filenames are full of spaces and brackets.
fn encode(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for byte in name.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Keep a message inside the 32 columns there are.
fn truncate(message: &str) -> String {
    message.chars().take(32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_catalogue_reply_becomes_entries() {
        let json = r#"{"response":{"numFound":2,"docs":[
            {"identifier":"zx_Jetpac_1983","title":"Jetpac [16K]"},
            {"identifier":"zx_Atic_Atac_1983"}
        ]}}"#;
        let entries = parse_catalogue(json).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "Jetpac [16K]");
        // With no title, the identifier is the best name available.
        assert_eq!(entries[1].title, "zx_Atic_Atac_1983");
        assert!(matches!(&entries[0].source, Source::Archive(id) if id == "zx_Jetpac_1983"));
    }

    #[test]
    fn nonsense_is_an_error_not_a_panic() {
        assert!(parse_catalogue("not json").is_err());
        assert!(parse_catalogue("{}").is_err());
    }

    #[test]
    fn the_snapshot_is_picked_out_of_the_file_list() {
        let json = r#"{"files":[
            {"name":"00_coverscreenshot.png","format":"PNG"},
            {"name":"Manic Miner (1983).z80","format":"Unknown"},
            {"name":"other.tap"}
        ]}"#;
        assert_eq!(
            pick_snapshot(json).as_deref(),
            Some("Manic Miner (1983).z80")
        );

        // A .sna will do if there is no .z80.
        let json = r#"{"files":[{"name":"game.tap"},{"name":"game.SNA"}]}"#;
        assert_eq!(pick_snapshot(json).as_deref(), Some("game.SNA"));

        // Tapes alone are no use yet.
        assert_eq!(pick_snapshot(r#"{"files":[{"name":"g.tzx"}]}"#), None);
        assert_eq!(pick_snapshot("{}"), None);
    }

    /// The one thing the fixtures cannot prove: that a file written by somebody else's
    /// emulator loads. Off by default because it needs the network.
    ///
    ///     cargo test --features ui --bin zxbevy -- --ignored --nocapture
    #[test]
    #[ignore = "needs the network"]
    fn a_real_archive_snapshot_loads_and_runs() {
        use on_the_spectrum::spectrum::Machine;

        let entries = fetch_catalogue(1).expect("catalogue");
        assert!(!entries.is_empty(), "the catalogue came back empty");

        let mut loaded = 0;
        for entry in entries.iter().take(4) {
            let Source::Archive(id) = &entry.source else {
                continue;
            };
            let path = match fetch_snapshot(id) {
                Ok(path) => path,
                Err(e) => {
                    println!("  {}: {e}", entry.title);
                    continue;
                }
            };
            let rom = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/roms/48.rom")).unwrap();
            let mut machine = Machine::new(&rom);
            match snapshot::load_path(&mut machine, &path) {
                Ok(()) => {
                    machine.run_frames(50);
                    let non_blank = machine
                        .screen_text()
                        .iter()
                        .filter(|l| !l.trim().is_empty())
                        .count();
                    println!(
                        "  {} -> PC ${:04X} after 50 frames, {non_blank} rows of text",
                        entry.title, machine.cpu.regs.pc
                    );
                    loaded += 1;
                }
                Err(e) => println!("  {}: {e}", entry.title),
            }
        }
        assert!(loaded > 0, "not one real snapshot loaded");
    }

    #[test]
    fn archive_filenames_survive_being_put_in_a_url() {
        assert_eq!(
            encode("Manic Miner (1983).z80"),
            "Manic%20Miner%20%281983%29.z80"
        );
        assert_eq!(encode("plain.z80"), "plain.z80");
    }
}
