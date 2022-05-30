use cfg_aliases::cfg_aliases;

fn main() {
    // Setup cfg aliases
    cfg_aliases! {
        // Platforms
        wasm: { all(target_arch = "wasm32", target_os = "unknown") },
        wasi: { all(target_arch = "wasm32", target_os = "wasi") },
    }
}
