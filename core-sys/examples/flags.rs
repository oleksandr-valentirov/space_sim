//! The cargo-side equivalent of `make flags`.
//!
//! Exists for the ROADMAP D1 fork: when hashes diverge, the first thing to do
//! is put the two command lines side by side. Without this they would have to
//! be dug out of `cargo build -vv`, four screens of log holding one wanted
//! line.
//!
//!     make flags
//!     cargo run -q --example flags
//!
//! Prints what `build.rs` actually read from `core/cflags.txt`. The full
//! compiler invocation, including what `cc` mixes in, is a separate command:
//!
//!     CC_ENABLE_DEBUG_OUTPUT=1 cargo build -vv

fn main() {
    println!("{}", env!("CORE_CFLAGS"));
}
