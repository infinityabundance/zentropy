//! `zentropy` — the research/production driver.
//!
//! Subcommands are intentionally small and scriptable so that every experiment
//! can be driven and receipted identically:
//!
//! ```text
//! zentropy hash      <file>
//! zentropy tokenize  <file> [--kinds]
//! zentropy compress  <in> <archive>
//! zentropy decompress <archive> <out>
//! zentropy verify    <original> <archive>
//! zentropy bench     <in> [--out <archive>] [--receipt <jsonl>]
//! zentropy ablate    <in>
//! zentropy hoist     <in>
//! zentropy pack-sfx  <stub> <archive> <out>
//! zentropy corrupt-court
//! zentropy negative-court
//! zentropy gate
//! zentropy selftest
//! ```

use std::env;
use std::fs;
use std::io::Write;
use std::process::ExitCode;
use std::time::Instant;

use zentropy::archive::{self, Method};
use zentropy::corpus::{self, hex, sha256};
use zentropy::evidence::RunReceipt;
use zentropy::ir::{self, Kind, Stats};
use zentropy::score::{Record, Score, Targets};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        usage();
        return ExitCode::from(2);
    }
    let result = match args[1].as_str() {
        "hash" => cmd_hash(&args[2..]),
        "tokenize" => cmd_tokenize(&args[2..]),
        "compress" => cmd_compress(&args[2..]),
        "decompress" => cmd_decompress(&args[2..]),
        "verify" => cmd_verify(&args[2..]),
        "bench" => cmd_bench(&args[2..]),
        "ablate" => cmd_ablate(&args[2..]),
        "pack-sfx" => cmd_pack_sfx(&args[2..]),
        "hoist" => cmd_hoist(&args[2..]),
        "corrupt-court" => cmd_corrupt_court(),
        "negative-court" => cmd_negative_court(),
        "gate" => cmd_gate(),
        "selftest" => cmd_selftest(),
        "help" | "-h" | "--help" => {
            usage();
            Ok(())
        }
        other => Err(format!("unknown subcommand: {other}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("zentropy: {e}");
            ExitCode::FAILURE
        }
    }
}

fn usage() {
    eprintln!(
        "zentropy {version}\n\
         \n\
         USAGE:\n  \
         zentropy hash       <file>\n  \
         zentropy tokenize   <file> [--kinds]\n  \
         zentropy compress   <in> <archive>\n  \
         zentropy decompress <archive> <out>\n  \
         zentropy verify     <original> <archive>\n  \
         zentropy bench      <in> [--out <archive>]\n  \
         zentropy gate\n  \
         zentropy selftest\n",
        version = zentropy::VERSION
    );
}

fn read(path: &str) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("cannot read {path}: {e}"))
}

fn write(path: &str, data: &[u8]) -> Result<(), String> {
    fs::write(path, data).map_err(|e| format!("cannot write {path}: {e}"))
}

fn cmd_hash(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or("hash: need a file")?;
    let data = read(path)?;
    let r = corpus::CorpusReceipt::of(path, &data);
    println!("{}", r.render());
    Ok(())
}

fn cmd_tokenize(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or("tokenize: need a file")?;
    let show = args.iter().any(|a| a == "--kinds");
    let data = read(path)?;
    let t0 = Instant::now();
    let toks = ir::tokenize(&data);
    let dt = t0.elapsed();
    let rendered = ir::render(&data, &toks);
    let exact = rendered == data;
    let st = Stats::of(&data, &toks);
    println!("file={path} bytes={} tokens={}", data.len(), toks.len());
    println!(
        "exact_roundtrip={exact} parse_ms={:.3}",
        dt.as_secs_f64() * 1e3
    );
    if !exact {
        return Err("IR tokenisation is not exact".into());
    }
    println!("{}", st.render_table());
    if show {
        for t in toks.iter().take(120) {
            let r = t.range();
            let s = String::from_utf8_lossy(&data[r]);
            let s: String = s.chars().take(60).collect();
            println!(
                "{:>10} {:>5} {:>8}  {}",
                t.kind.name(),
                t.start,
                t.len,
                s.escape_debug()
            );
        }
    }
    // Sanity: every Kind must be representable.
    assert_eq!(Kind::ALL.len(), 17);
    Ok(())
}

fn cmd_compress(args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        return Err("compress: need <in> <archive>".into());
    }
    let data = read(&args[0])?;
    let t0 = Instant::now();
    let arch = archive::encode(&data);
    let dt = t0.elapsed();
    write(&args[1], &arch)?;
    eprintln!(
        "compressed {} -> {} bytes in {:.3}s",
        data.len(),
        arch.len(),
        dt.as_secs_f64()
    );
    Ok(())
}

fn cmd_decompress(args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        return Err("decompress: need <archive> <out>".into());
    }
    let arch = read(&args[0])?;
    let t0 = Instant::now();
    let out = archive::decode(&arch).ok_or("decompress: malformed archive")?;
    let dt = t0.elapsed();
    write(&args[1], &out)?;
    eprintln!(
        "decompressed {} -> {} bytes in {:.3}s",
        arch.len(),
        out.len(),
        dt.as_secs_f64()
    );
    Ok(())
}

fn cmd_verify(args: &[String]) -> Result<(), String> {
    if args.len() < 2 {
        return Err("verify: need <original> <archive>".into());
    }
    let original = read(&args[0])?;
    let arch = read(&args[1])?;
    let t0 = Instant::now();
    let out = archive::decode(&arch).ok_or("verify: malformed archive")?;
    let dt = t0.elapsed();
    let exact = out == original;
    let oh = sha256(&original);
    let dh = sha256(&out);
    println!("original bytes={} sha256={}", original.len(), hex(&oh));
    println!("decoded  bytes={} sha256={}", out.len(), hex(&dh));
    println!("exact={exact} decode_s={:.3}", dt.as_secs_f64());
    if !exact {
        return Err("reconstruction is not exact".into());
    }
    Ok(())
}

fn cmd_bench(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or("bench: need <in>")?;
    let out_path = args
        .iter()
        .position(|a| a == "--out")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let receipt_path = args
        .iter()
        .position(|a| a == "--receipt")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let data = read(path)?;

    let input_receipt = corpus::CorpusReceipt::of(path, &data);
    let t0 = Instant::now();
    let arch = archive::encode(&data);
    let c_time = t0.elapsed();

    let t1 = Instant::now();
    let out = archive::decode(&arch).ok_or("bench: malformed archive")?;
    let d_time = t1.elapsed();

    let exact = out == data;
    let decoded_hash = hex(&sha256(&out));
    if let Some(p) = out_path {
        write(&p, &arch)?;
    }

    // The driver binary is not yet the submission stub; report archive-only for
    // now and label it explicitly so the number cannot be mistaken for S. The
    // ratio and bits/byte are measured against *this input*, since the Hutter
    // projection is only meaningful once the full corpus has been run.
    let s = Score::new(0, arch.len() as u64);
    let t = Targets::pinned();
    let input_len = data.len() as f64;
    println!("=== Zentropy run report ===");
    println!("revision: {}", revision());
    println!("input: {}", input_receipt.render());
    println!("method: RawCm (Phase-2 floor)");
    println!("archive_bytes: {}", arch.len());
    println!("compressor_bytes: 0 (driver not yet packaged as submission)");
    println!("S(archive-only): {}", s.total());
    println!("ratio(vs input): {:.4}", input_len / arch.len() as f64);
    println!(
        "bits/byte(input): {:.4}",
        (arch.len() as f64 * 8.0) / input_len
    );
    println!("compression: wall={:.3}s", c_time.as_secs_f64());
    println!("decompression: wall={:.3}s", d_time.as_secs_f64());
    println!("peak_rss_bytes: {}", peak_rss());
    println!(
        "reconstruction: exact={} decoded_sha256={}",
        exact, decoded_hash
    );
    if data.len() as u64 == zentropy::corpus::ENWIK9_LEN {
        println!(
            "delta vs official record: S - L = {}",
            s.total() as i64 - t.t0.previous_total as i64
        );
        println!("gate (0.99*L): {}", t.gate());
    } else {
        println!(
            "note: full-corpus score applies only at {} bytes (this run is {} bytes)",
            zentropy::corpus::ENWIK9_LEN,
            data.len()
        );
        println!("gate (0.99*L, for reference): {}", t.gate());
    }
    let decision = if exact {
        "MEASURED"
    } else {
        "REJECTED(in exact)"
    };
    println!("decision: {}", decision);

    // Bind an immutable evidence receipt. A negative or failed run is preserved
    // exactly as a successful one: never delete negative knowledge.
    if let Some(rp) = receipt_path {
        let mut r = RunReceipt::default();
        r.id = format!(
            "rawcm/{}/{}",
            &input_receipt.hex_digest()[..12],
            input_receipt.len
        );
        r.hypothesis = "Phase-2 coding floor reconstructs the corpus exactly".into();
        r.parent = "".into();
        r.revision = revision();
        r.compiler = format!(
            "rustc {} {}-{}",
            rustc_version(),
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        r.corpus = input_receipt.name.clone();
        r.input_sha256 = input_receipt.hex_digest();
        r.archive_sha256 = hex(&sha256(&arch));
        r.decoded_sha256 = decoded_hash.clone();
        r.exact = exact;
        r.compressor_bytes = 0;
        r.archive_bytes = arch.len() as u64;
        r.wall_seconds = (c_time + d_time).as_secs_f64();
        r.cpu_seconds = 0.0;
        r.peak_rss_bytes = peak_rss();
        r.temp_disk_bytes = 0;
        r.environment = format!(
            "{} {} cores={}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        );
        r.attribution = "raw context-mixing floor, orders 1,2,3,4,6,8 + match model".into();
        r.decision = decision.into();
        r.notes = "archive-only; compressor bytes not yet counted (driver, not SFX)".into();
        r.extra.push((
            "bits_per_byte".into(),
            format!("{:.6}", (arch.len() as f64 * 8.0) / input_len),
        ));
        r.extra.push((
            "ratio".into(),
            format!("{:.6}", input_len / arch.len() as f64),
        ));
        append_jsonl(&rp, &r)?;
    }
    if !exact {
        return Err("bench: reconstruction is not exact".into());
    }
    Ok(())
}

/// Measure the marginal value of the word/bigram experts by leave-one-out.
/// Law 7: if the complete marginal cost is not negative, the expert is deleted.
fn cmd_ablate(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or("ablate: need <in>")?;
    let data = read(path)?;

    let t0 = Instant::now();
    let full = archive::encode_with(&data, Method::RawCm);
    let full_s = t0.elapsed();
    let t1 = Instant::now();
    let no_word = archive::encode_with(&data, Method::RawCmNoWord);
    let no_word_s = t1.elapsed();

    let full_ok = archive::decode(&full).as_deref() == Some(&data[..]);
    let no_ok = archive::decode(&no_word).as_deref() == Some(&data[..]);

    let n = data.len() as f64;
    println!("input_bytes: {}", data.len());
    println!(
        "full (orders+word+bigram+match): {} bytes, {:.4} bpc, {:.3}s, exact={}",
        full.len(),
        (full.len() as f64 * 8.0) / n,
        full_s.as_secs_f64(),
        full_ok
    );
    println!(
        "no-word (orders+match):          {} bytes, {:.4} bpc, {:.3}s, exact={}",
        no_word.len(),
        (no_word.len() as f64 * 8.0) / n,
        no_word_s.as_secs_f64(),
        no_ok
    );
    // Saving is positive when the full model is smaller than the ablation.
    let saving = no_word.len() as i64 - full.len() as i64;
    println!(
        "DeltaS(word experts) = no_word - full = {} bytes ({:.4} bpc)",
        saving,
        (saving as f64 * 8.0) / n
    );
    println!(
        "decision: {}",
        if saving > 0 {
            "ADOPTED (word experts pay for themselves)"
        } else {
            "REJECTED (complete marginal cost not negative)"
        }
    );
    if !(full_ok && no_ok) {
        return Err("ablate: a variant failed exactness".into());
    }
    Ok(())
}

/// Build a self-extracting `archive9` = stub + marker + length + archive.
/// Report the real byte split so `S` is measured, never assumed.
fn cmd_pack_sfx(args: &[String]) -> Result<(), String> {
    if args.len() < 3 {
        return Err("pack-sfx: need <stub> <archive> <out>".into());
    }
    let mut image = read(&args[0])?;
    let stub = image.len() as u64;
    let arch = read(&args[1])?;
    archive::append_sfx(&mut image, &arch);
    write(&args[2], &image)?;
    // A self-extracting archive must be directly runnable.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&args[2], fs::Permissions::from_mode(0o755));
    }
    let s = Score::new(stub, arch.len() as u64);
    println!("stub_bytes={} archive_bytes={}", stub, arch.len());
    println!("archive9_bytes={}", image.len());
    println!("S(comp9=stub + archive9)={}", stub + image.len() as u64);
    println!("S(separate, comp9a=decomp9)={}", s.total());
    Ok(())
}

/// Phase-3 structural-hoisting experiment. Measures the complete `ΔS` of the
/// transform: archive delta plus the transform's binary bytes. Adopt only if
/// the total is negative.
fn cmd_hoist(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or("hoist: need <in>")?;
    let data = read(path)?;

    let t0 = Instant::now();
    let base = archive::encode_with(&data, Method::RawCm);
    let base_s = t0.elapsed();
    let t1 = Instant::now();
    let hoisted = archive::encode_with(&data, Method::StructHoist);
    let hoist_s = t1.elapsed();

    let base_ok = archive::decode(&base).as_deref() == Some(&data[..]);
    let hoist_ok = archive::decode(&hoisted).as_deref() == Some(&data[..]);
    let bin_cost = zentropy::transform::binary_cost_estimate();

    let binary_cost = bin_cost;
    let n = data.len() as f64;
    let receipt_path = args
        .iter()
        .position(|a| a == "--receipt")
        .and_then(|i| args.get(i + 1))
        .cloned();
    println!("input_bytes: {}", data.len());
    println!(
        "raw  : {} bytes, {:.4} bpc, {:.3}s, exact={}",
        base.len(),
        (base.len() as f64 * 8.0) / n,
        base_s.as_secs_f64(),
        base_ok
    );
    println!(
        "hoist: {} bytes, {:.4} bpc, {:.3}s, exact={}",
        hoisted.len(),
        (hoisted.len() as f64 * 8.0) / n,
        hoist_s.as_secs_f64(),
        hoist_ok
    );
    let archive_saving = base.len() as i64 - hoisted.len() as i64;
    let delta_s = hoisted.len() as i64 + bin_cost as i64 - base.len() as i64;
    println!("archive saving = {} bytes", archive_saving);
    println!("binary cost   = {} bytes (upper estimate)", bin_cost);
    println!("DeltaS(hoist) = {} bytes", delta_s);
    println!(
        "decision: {}",
        if delta_s < 0 {
            "ADOPTED"
        } else {
            "REJECTED (complete cost not negative)"
        }
    );
    if let Some(rp) = receipt_path {
        let mut r = RunReceipt::default();
        r.id = format!(
            "struct-hoist/{}/{}",
            &sha256(&data)[..6]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            data.len()
        );
        r.hypothesis =
            "hoisting frequent structural strings into single-byte codes lowers complete S".into();
        r.parent = "rawcm".into();
        r.revision = revision();
        r.compiler = format!(
            "rustc {} {}-{}",
            rustc_version(),
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        r.corpus = path.clone();
        r.input_sha256 = hex(&sha256(&data));
        r.archive_sha256 = hex(&sha256(&hoisted));
        r.decoded_sha256 = hex(&sha256(&archive::decode(&hoisted).unwrap_or_default()));
        r.exact = hoist_ok;
        r.compressor_bytes = binary_cost;
        r.archive_bytes = hoisted.len() as u64;
        r.wall_seconds = hoist_s.as_secs_f64();
        r.peak_rss_bytes = peak_rss();
        r.environment = format!(
            "{} {} cores={}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::thread::available_parallelism()
                .map(|x| x.get())
                .unwrap_or(1)
        );
        r.attribution = "Phase-3 structural hoisting + full floor".into();
        r.decision = if delta_s < 0 { "ADOPTED" } else { "REJECTED" }.into();
        r.notes =
            format!("archive_saving={archive_saving} binary_cost={binary_cost} delta_S={delta_s}");
        r.extra.push((
            "bits_per_byte".into(),
            format!("{:.6}", (hoisted.len() as f64 * 8.0) / n),
        ));
        r.extra
            .push(("baseline_archive_bytes".into(), base.len().to_string()));
        append_jsonl(&rp, &r)?;
    }
    if !(base_ok && hoist_ok) {
        return Err("hoist: a variant failed exactness".into());
    }
    Ok(())
}

/// Decoder-corruption court (§42): mutate a valid archive many ways and require
/// that decoding never panics, overflows, or allocates without bound. A corrupt
/// archive need not recover gracefully; it must not undermine confidence in the
/// valid path. Because `panic = "abort"` in release, a panic would kill the
/// process and fail this court.
fn cmd_corrupt_court() -> Result<(), String> {
    let data: Vec<u8> =
        b"<page><title>Zentropy</title><text>corruption court</text></page>\n".repeat(400);
    let arch = archive::encode(&data);
    let mut tested = 0u64;

    // Truncations (including the header).
    for k in 0..arch.len().min(64) {
        let _ = archive::decode(&arch[..k]);
        tested += 1;
    }
    let _ = archive::decode(&arch[..arch.len() / 2]);
    tested += 1;

    // Single-byte mutations across the whole archive, including the header.
    for i in 0..arch.len() {
        let mut m = arch.clone();
        m[i] ^= 0xA5;
        let _ = archive::decode(&m);
        tested += 1;
    }

    // Bad magic and absurd declared length.
    let mut m = arch.clone();
    m[0] ^= 0xff;
    if archive::decode(&m).is_some() {
        return Err("bad magic was accepted".into());
    }
    let mut m = arch.clone();
    m[5..13].copy_from_slice(&u64::MAX.to_le_bytes());
    if archive::decode(&m).is_some() {
        return Err("absurd length was accepted".into());
    }
    tested += 2;

    println!("corrupt-court OK: {tested} mutations, no panic, no unbounded allocation");
    Ok(())
}

/// Negative control (§26): a deterministic incompressible stream must not
/// compress. If it did, the model would be leaking knowledge of the generator.
fn cmd_negative_court() -> Result<(), String> {
    let n = 1_000_000usize;
    let mut s = 0x2545_F491_4F6C_DD1Du64;
    let mut data = Vec::with_capacity(n);
    for _ in 0..n {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        data.push((s >> 24) as u8);
    }
    let arch = archive::encode(&data);
    let bpb = (arch.len() as f64 * 8.0) / n as f64;
    println!(
        "negative-court: incompressible {n} bytes -> {} bytes ({bpb:.4} bpc)",
        arch.len()
    );
    if arch.len() + 64 < data.len() {
        return Err("incompressible stream compressed; negative control failed".into());
    }
    if archive::decode(&arch).as_deref() != Some(&data[..]) {
        return Err("negative control did not reconstruct exactly".into());
    }
    println!("negative-court OK");
    Ok(())
}

fn cmd_gate() -> Result<(), String> {
    let t = Targets::pinned();
    println!(
        "T0 (official accepted record): {} = {}",
        t.t0.label, t.t0.previous_total
    );
    println!("prize gate = floor(0.99*T0) = {}", t.gate());
    println!("T1 (strongest credible pending frontier): {}", t.t1);
    println!("T2 (internal moonshot, not a claim): {}", t.t2);
    let rec = Record {
        previous_total: t.t1,
        label: "hypothetical pending frontier",
    };
    println!("if T1 becomes the record, gate = {}", rec.gate_total());
    Ok(())
}

fn cmd_selftest() -> Result<(), String> {
    // A deterministic synthetic corpus that exercises the pipeline end to end
    // without requiring network access.
    let mut data = Vec::new();
    for i in 0..20_000u32 {
        data.extend_from_slice(
            format!(
                "<page><title>Article {i}</title><text>The quick brown fox number {i} jumps \
                 over the lazy dog. {{cite|id={i}}} [[Target {i}|display]] &amp; more text.\n\
                 </text></page>\n"
            )
            .as_bytes(),
        );
    }
    let toks = ir::tokenize(&data);
    if ir::render(&data, &toks) != data {
        return Err("selftest: IR round-trip failed".into());
    }
    let arch = archive::encode(&data);
    let back = archive::decode(&arch).ok_or("selftest: decode failed")?;
    if back != data {
        return Err("selftest: archive round-trip failed".into());
    }
    println!(
        "selftest OK: bytes={} tokens={} archive={} ratio={:.3}",
        data.len(),
        toks.len(),
        arch.len(),
        data.len() as f64 / arch.len() as f64
    );
    Ok(())
}

/// Best-effort source revision, from an environment variable or a literal.
fn revision() -> String {
    env::var("ZENTROPY_REVISION").unwrap_or_else(|_| "<untracked>".to_string())
}

/// Compiler identity captured at build time by `build.rs`.
fn rustc_version() -> &'static str {
    option_env!("RUSTC_VERSION").unwrap_or("unknown")
}

/// Append one JSON object plus newline to `path`, creating it if needed.
fn append_jsonl(path: &str, r: &zentropy::evidence::RunReceipt) -> Result<(), String> {
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("cannot append receipt {path}: {e}"))?;
    writeln!(f, "{}", r.to_json()).map_err(|e| format!("cannot write receipt {path}: {e}"))?;
    Ok(())
}

/// Peak resident set size in bytes (Linux). Zero if unavailable.
fn peak_rss() -> u64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(s) = fs::read_to_string("/proc/self/status") {
            for line in s.lines() {
                if let Some(rest) = line.strip_prefix("VmHWM:") {
                    let kb: u64 = rest
                        .split_whitespace()
                        .next()
                        .and_then(|x| x.parse().ok())
                        .unwrap_or(0);
                    return kb * 1024;
                }
            }
        }
    }
    0
}

// Keep the `Write` import meaningful across platforms/toolchains.
#[allow(dead_code)]
fn _assert_write_trait() {
    fn _f<W: Write>(_: W) {}
}
