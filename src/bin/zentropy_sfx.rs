//! `zentropy-sfx` — the submission-plane (de)compressor.
//!
//! This binary is both `comp9a` and `decomp9`. The Hutter rules allow a single
//! program to serve both roles, in which case the `2×` decompressor factor in
//! the separate-file relaxation reduces to `1×`:
//!
//! ```text
//! S = len(comp9a) + len(decomp9) + len(archive9.bhm)   with comp9a = decomp9
//! ```
//!
//! Modes:
//! ```text
//! archive9                 # self-extracting: reconstruct to $ZENTROPY_OUT
//! zentropy-sfx c <in> <out>   # compress
//! zentropy-sfx d <in> <out>   # decompress
//! zentropy-sfx <archive> <out>
//! ```
//!
//! It links only [`zentropy::archive`] and its dependencies; the research
//! machinery in the rest of the crate is never reached from this path. Keeping
//! the stub separate is what lets us measure the real `compressor_bytes +
//! archive_bytes` split rather than guessing at it.

use std::env;
use std::fs;
use std::process::ExitCode;

use zentropy::archive::extract_sfx;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("zentropy-sfx: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    // Explicit roles: `c` compress, `d` decompress. `comp9a == decomp9`.
    if args.len() == 4 && (args[1] == "c" || args[1] == "d") {
        let data = fs::read(&args[2]).map_err(|e| format!("read {}: {e}", args[2]))?;
        let out = if args[1] == "c" {
            zentropy::archive::encode(&data)
        } else {
            zentropy::archive::decode(&data).ok_or("malformed archive")?
        };
        fs::write(&args[3], &out).map_err(|e| format!("write {}: {e}", args[3]))?;
        return Ok(());
    }

    // Explicit form: `zentropy-sfx <archive> <out>`. Useful for testing and for
    // the `decomp9.exe + archive9.bhm` relaxation.
    if args.len() == 3 {
        let arch = fs::read(&args[1]).map_err(|e| format!("read {}: {e}", args[1]))?;
        let out = zentropy::archive::decode(&arch).ok_or("malformed archive")?;
        fs::write(&args[2], &out).map_err(|e| format!("write {}: {e}", args[2]))?;
        return Ok(());
    }

    // Self-extracting form: `archive9` with no arguments reconstructs the corpus.
    if args.len() == 1 {
        let exe = env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        let bytes = fs::read(&exe).map_err(|e| format!("read self: {e}"))?;
        let archive = extract_sfx(&bytes).ok_or("no appended archive (is this a packaged sfx?)")?;
        let out = zentropy::archive::decode(archive).ok_or("malformed appended archive")?;
        // Default output name per the Hutter convention; overridable by env.
        let name = env::var("ZENTROPY_OUT").unwrap_or_else(|_| "data9".to_string());
        fs::write(&name, &out).map_err(|e| format!("write {name}: {e}"))?;
        return Ok(());
    }

    Err("usage: archive9                 (self-extract)\n       \
         zentropy-sfx c <in> <archive>  (compress)\n       \
         zentropy-sfx d <archive> <out> (decompress)\n       \
         zentropy-sfx <archive> <out>"
        .into())
}
