// Standalone reproduction harness for the "premature submit during dictation" bug.
// Mirrors src/output/keyboard.rs::type_string exactly — same chunking, same Shift+Return
// strategy, same delays — so behavior here matches the streaming-keyboard output path
// the running app uses. Bypasses the LLM and the Tauri pipeline so the only variable
// is what enigo does with the bytes you pass in.
//
// Usage:
//   cd src-tauri
//   cargo run --example keystroke_test -- "<payload>" [delay_ms]
//
// `\n` in the payload becomes a real newline (triggers the Shift+Return branch).
// On macOS, grant Accessibility to target/debug/examples/keystroke_test on first run,
// or the OS silently drops the synthesized events.

use enigo::{Direction, Enigo, Key, Keyboard, Settings};
use std::env;
use std::thread::sleep;
use std::time::Duration;

const TYPE_CHUNK_SIZE: usize = 200;
const TYPE_CHUNK_DELAY_MS: u64 = 5;

fn type_string(enigo: &mut Enigo, text: &str) -> anyhow::Result<()> {
    let lines: Vec<&str> = text.split('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        if !line.is_empty() {
            for chunk in line.chars().collect::<Vec<_>>().chunks(TYPE_CHUNK_SIZE) {
                let s: String = chunk.iter().collect();
                enigo
                    .text(&s)
                    .map_err(|e| anyhow::anyhow!("enigo.text failed: {:?}", e))?;
                sleep(Duration::from_millis(TYPE_CHUNK_DELAY_MS));
            }
        }
        if i < lines.len() - 1 {
            enigo
                .key(Key::Shift, Direction::Press)
                .map_err(|e| anyhow::anyhow!("Shift press: {:?}", e))?;
            enigo
                .key(Key::Return, Direction::Click)
                .map_err(|e| anyhow::anyhow!("Return click: {:?}", e))?;
            enigo
                .key(Key::Shift, Direction::Release)
                .map_err(|e| anyhow::anyhow!("Shift release: {:?}", e))?;
        }
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let mut args = env::args().skip(1);
    let payload = args.next().unwrap_or_else(|| {
        eprintln!(
            "Usage: keystroke_test \"<payload>\" [delay_ms]\n  \
             Use \\n in the payload for a newline (triggers Shift+Return).\n  \
             Example: keystroke_test \"alpha\\nbravo\\ncharlie\" 3000"
        );
        std::process::exit(2);
    });
    let delay_ms: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(3000);

    let payload = payload.replace("\\n", "\n");

    eprintln!("--- keystroke_test ---");
    eprintln!("payload ({} bytes):", payload.len());
    for (i, line) in payload.split('\n').enumerate() {
        eprintln!("  line {}: {:?}", i, line);
    }
    eprintln!("Focus the target window now. Typing in {} ms...", delay_ms);
    sleep(Duration::from_millis(delay_ms));

    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| anyhow::anyhow!("Enigo init: {:?}", e))?;
    type_string(&mut enigo, &payload)?;

    eprintln!("Done. If nothing appeared in the target window on macOS, the example binary");
    eprintln!("needs Accessibility permission: System Settings > Privacy & Security >");
    eprintln!("Accessibility, then add target/debug/examples/keystroke_test and toggle it on.");
    Ok(())
}
