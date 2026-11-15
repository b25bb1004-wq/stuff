//! Assemble the lithium kernel, boot it on silicon, and print what the UART shows.
use lithium::{boot, image, run};
use silicon::machine::Config;

fn main() {
    // Step 1: assemble the kernel source into bytes at RAM_BASE.
    match image() {
        Ok(bytes) => println!("assembled kernel: {} bytes", bytes.len()),
        Err(e) => {
            println!("assembly failed: {e}");
            return;
        }
    }

    // Step 2: boot a machine with the kernel loaded and the pc at its entry.
    match boot(Config::default()) {
        Ok(_) => println!("boot: kernel loaded into a default machine"),
        Err(e) => println!("boot failed: {e}"),
    }

    // Step 3: run for a fixed step budget; the kernel ends the run through the test finisher.
    let (exit, uart) = run(2_000_000);
    println!("exit: {exit:?}");
    let text = String::from_utf8_lossy(&uart);
    let lines: Vec<&str> = text.lines().collect();
    println!("uart: {} line(s)", lines.len());
    for line in lines {
        println!("  | {line}");
    }
}
