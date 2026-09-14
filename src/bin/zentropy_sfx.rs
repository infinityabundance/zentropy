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
//! archive9                    # self-extracting: reconstructs to ./data9
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
            #[cfg(feature = "mem-guard")]
            guard_encode(data.len() as u64)?;
            zentropy::archive::encode(&data)
        } else {
            #[cfg(feature = "mem-guard")]
            guard_decode(&data)?;
            zentropy::archive::decode(&data).ok_or("malformed archive")?
        };
        fs::write(&args[3], &out).map_err(|e| format!("write {}: {e}", args[3]))?;
        return Ok(());
    }

    // Explicit form: `zentropy-sfx <archive> <out>`. Useful for testing and for
    // the `decomp9.exe + archive9.bhm` relaxation.
    if args.len() == 3 {
        let arch = fs::read(&args[1]).map_err(|e| format!("read {}: {e}", args[1]))?;
        #[cfg(feature = "mem-guard")]
        guard_decode(&arch)?;
        let out = zentropy::archive::decode(&arch).ok_or("malformed archive")?;
        fs::write(&args[2], &out).map_err(|e| format!("write {}: {e}", args[2]))?;
        return Ok(());
    }

    // Self-extracting form: `archive9` with no arguments reconstructs the corpus.
    if args.len() == 1 {
        let exe = env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
        let bytes = fs::read(&exe).map_err(|e| format!("read self: {e}"))?;
        let archive = extract_sfx(&bytes).ok_or("no appended archive (is this a packaged sfx?)")?;
        #[cfg(feature = "mem-guard")]
        guard_decode(archive)?;
        let out = zentropy::archive::decode(archive).ok_or("malformed appended archive")?;
        // Fixed output name per the Hutter convention. Deliberately *not*
        // configurable by environment: the rules forbid results that depend on
        // unrecorded environment state, and an override here would be exactly
        // that, for no benefit — the judge renames the file if it wants to.
        let name = "data9";
        fs::write(name, &out).map_err(|e| format!("write {name}: {e}"))?;
        return Ok(());
    }

    Err("usage: archive9                 (self-extract)\n       \
         zentropy-sfx c <in> <archive>  (compress)\n       \
         zentropy-sfx d <archive> <out> (decompress)\n       \
         zentropy-sfx <archive> <out>"
        .into())
}

/// Refuse a run that would exceed the memory budget. The submission stub uses
/// the same budget logic as the driver so a judged run cannot OOM the host.
/// The check is formatting-free to keep the scored binary small.
#[cfg(feature = "mem-guard")]
#[inline]
fn guard_encode(n: u64) -> Result<(), String> {
    if zentropy::memory::fits(
        zentropy::memory::projected_encode(n, 2),
        zentropy::memory::budget(None),
    ) {
        Ok(())
    } else {
        Err("projected memory exceeds budget (set ZENTROPY_MAX_RAM_BYTES to raise)".into())
    }
}

#[cfg(feature = "mem-guard")]
#[inline]
fn guard_decode(archive: &[u8]) -> Result<(), String> {
    // Project the configuration the archive *declares*. Using the accepted
    // method/tune here would clear a forged header that asks for a different
    // geometry — the exact hole T2 would otherwise have opened, since the tune
    // byte now selects the table size.
    if let Some((method, tune, n)) = zentropy::archive::peek_header(archive) {
        if !zentropy::memory::fits(
            zentropy::memory::projected_decode_for(archive.len() as u64, n, method, tune),
            zentropy::memory::budget(None),
        ) {
            return Err(
                "projected memory exceeds budget (set ZENTROPY_MAX_RAM_BYTES to raise)".into(),
            );
        }
    }
    Ok(())
}
