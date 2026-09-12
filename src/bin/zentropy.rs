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
//! zentropy eval      <in> --candidate <method> [--parent <method>] --binary-cost <n> [--receipt <f>]
//! zentropy sweep     <in> [--method <method>] [--receipt <f>]
//! zentropy pack-sfx  <stub> <archive> <out>
//! zentropy corrupt-court
//! zentropy negative-court
//! zentropy gate
//! zentropy meminfo    [file] [--max-ram <size>]
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
use zentropy::memory;
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
        "eval" => cmd_eval(&args[2..]),
        "sweep" => cmd_sweep(&args[2..]),
        "corrupt-court" => cmd_corrupt_court(),
        "negative-court" => cmd_negative_court(),
        "gate" => cmd_gate(),
        "meminfo" => cmd_meminfo(&args[2..]),
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

/// `--max-ram <size>` override, if present.
fn max_ram_override(args: &[String]) -> Option<u64> {
    args.iter()
        .position(|a| a == "--max-ram")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| memory::parse_size(s))
}

/// Refuse to start a memory-heavy operation that would exceed the budget.
fn guard_encode(n: u64, max_ram: Option<u64>) -> Result<(), String> {
    memory::check(memory::projected_encode(n, 2), memory::budget(max_ram))
}

fn guard_decode(archive_len: u64, n: u64, max_ram: Option<u64>) -> Result<(), String> {
    memory::check(
        memory::projected_decode(archive_len, n),
        memory::budget(max_ram),
    )
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
    guard_encode(data.len() as u64, max_ram_override(args))?;
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
    if let Some(n) = archive::peek_len(&arch) {
        guard_decode(arch.len() as u64, n, max_ram_override(args))?;
    }
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
    if let Some(n) = archive::peek_len(&arch) {
        guard_decode(arch.len() as u64, n, max_ram_override(args))?;
    }
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
    guard_encode(data.len() as u64, max_ram_override(args))?;

    let input_receipt = corpus::CorpusReceipt::of(path, &data);
    let t0 = Instant::now();
    let arch = archive::encode(&data);
    let c_time = t0.elapsed();

    if let Some(n) = archive::peek_len(&arch) {
        guard_decode(arch.len() as u64, n, max_ram_override(args))?;
    }
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
    let s = Score::shared_program(stub, arch.len() as u64);
    let sfx = Score::self_extracting(stub, image.len() as u64);
    println!("stub_bytes={} archive_bytes={}", stub, arch.len());
    println!("archive9_bytes={}", image.len());
    println!("S(self-extracting: comp9 + archive9)  = {}", sfx.total());
    println!("S(separate, comp9a=decomp9: 2P + bhm)  = {}", s.total());
    println!(
        "  (program charged {}x = {}, archive = {})",
        2,
        2 * stub,
        arch.len()
    );
    Ok(())
}

/// Phase-3 structural-hoisting experiment. Measures the complete `ΔS` of the
/// transform: archive delta plus the transform's binary bytes. Adopt only if
/// the total is negative.
fn cmd_hoist(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or("hoist: need <in>")?;
    let data = read(path)?;
    guard_encode(data.len() as u64, max_ram_override(args))?;

    let t0 = Instant::now();
    let base = archive::encode_with(&data, Method::RawCm);
    let base_s = t0.elapsed();
    let t1 = Instant::now();
    let hoisted = archive::encode_with(&data, Method::StructHoist);
    let hoist_s = t1.elapsed();

    let base_ok = archive::decode(&base).as_deref() == Some(&data[..]);
    let hoist_ok = archive::decode(&hoisted).as_deref() == Some(&data[..]);

    // `S` is authority: the executable cost of the mechanism must be *measured*,
    // never estimated. Pass the delta produced by `tools/measure_binary_cost.sh`.
    let measured_bin_cost: Option<u64> = args
        .iter()
        .position(|a| a == "--binary-cost")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok());

    let binary_cost = measured_bin_cost.unwrap_or(0);
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
    let delta_s = hoisted.len() as i64 + binary_cost as i64 - base.len() as i64;
    println!("archive saving = {} bytes", archive_saving);
    match measured_bin_cost {
        Some(c) => println!("binary cost   = {} bytes (measured)", c),
        None => println!(
            "binary cost   = UNMEASURED -- run tools/measure_binary_cost.sh and pass --binary-cost N"
        ),
    }
    println!("DeltaS(hoist) = {} bytes", delta_s);
    let decision = if measured_bin_cost.is_none() {
        "INDETERMINATE (binary cost unmeasured)"
    } else if delta_s < 0 {
        "ADOPTED"
    } else {
        "REJECTED (complete cost not negative)"
    };
    println!("decision: {decision}");
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
        r.decision = decision.into();
        r.notes = format!(
            "archive_saving={archive_saving} binary_cost_measured={} delta_S={delta_s}",
            measured_bin_cost
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unmeasured".into())
        );
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

/// Optimization Phase A: complete-cost evaluation of a candidate method against
/// a parent method. `S` is authority; the candidate's executable cost must be
/// supplied as a *measured* value.
fn cmd_eval(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or("eval: need <in>")?;
    let get = |k: &str| -> Option<String> {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let candidate = get("--candidate")
        .and_then(|s| Method::from_name(&s))
        .ok_or("eval: --candidate <method> required")?;
    let parent = match get("--parent") {
        Some(s) => Method::from_name(&s).ok_or("eval: unknown --parent method")?,
        None => archive::ACCEPTED_METHOD,
    };
    let bin_cost: u64 = get("--binary-cost")
        .and_then(|v| v.parse().ok())
        .ok_or("eval: --binary-cost <measured bytes> required")?;
    let receipt_path = get("--receipt");
    // The Phase-A parent is the currently accepted tune; override to compare
    // against a different optimizer variant.
    let parent_tune: u8 = get("--parent-tune")
        .and_then(|v| v.parse().ok())
        .unwrap_or(archive::ACCEPTED_TUNE);
    let tune: u8 = get("--tune")
        .and_then(|v| v.parse().ok())
        .unwrap_or(archive::ACCEPTED_TUNE);

    let data = read(path)?;
    guard_encode(data.len() as u64, max_ram_override(args))?;

    let t0 = Instant::now();
    let parent_arch = archive::encode_tuned(&data, parent, parent_tune);
    let parent_s = t0.elapsed();
    let t1 = Instant::now();
    let cand_arch = archive::encode_tuned(&data, candidate, tune);
    let cand_s = t1.elapsed();

    let parent_ok = archive::decode(&parent_arch).as_deref() == Some(&data[..]);
    // Decode the candidate once: the exactness court and the receipt's
    // decoded digest both need the result, and on enwik9 a second decode costs
    // ~20 minutes.
    let cand_dec = archive::decode(&cand_arch);
    let cand_ok = cand_dec.as_deref() == Some(&data[..]);

    let n = data.len() as f64;
    let archive_delta = cand_arch.len() as i64 - parent_arch.len() as i64;
    let delta_s = archive_delta + bin_cost as i64;
    let decision = if !cand_ok {
        "REJECTED (roundtrip)"
    } else if delta_s < 0 {
        "ADOPTED"
    } else {
        "REJECTED"
    };

    println!("input_bytes: {}", data.len());
    println!(
        "parent    {:<26} {:>12} bytes  {:.4} bpc  {:.3}s  exact={}",
        parent.name(),
        parent_arch.len(),
        (parent_arch.len() as f64 * 8.0) / n,
        parent_s.as_secs_f64(),
        parent_ok
    );
    println!(
        "candidate {:<26} {:>12} bytes  {:.4} bpc  {:.3}s  exact={}",
        candidate.name(),
        cand_arch.len(),
        (cand_arch.len() as f64 * 8.0) / n,
        cand_s.as_secs_f64(),
        cand_ok
    );
    println!("tune          = {tune}");
    println!("archive_delta = {} bytes", archive_delta);
    println!("binary_cost   = {} bytes (measured)", bin_cost);
    println!("DeltaS        = {} bytes", delta_s);
    println!("decision: {decision}");

    if let Some(rp) = receipt_path {
        let mut r = RunReceipt::default();
        r.id = format!("opt-a/{}/{}", candidate.name(), data.len());
        r.hypothesis = format!(
            "{} improves complete S over {}",
            candidate.name(),
            parent.name()
        );
        r.parent = parent.name().into();
        r.revision = revision();
        r.compiler = format!(
            "rustc {} {}-{}",
            rustc_version(),
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        r.corpus = path.clone();
        r.input_sha256 = hex(&sha256(&data));
        r.archive_sha256 = hex(&sha256(&cand_arch));
        r.decoded_sha256 = hex(&sha256(cand_dec.as_deref().unwrap_or_default()));
        r.exact = cand_ok;
        r.compressor_bytes = bin_cost;
        r.archive_bytes = cand_arch.len() as u64;
        r.wall_seconds = cand_s.as_secs_f64();
        r.peak_rss_bytes = peak_rss();
        r.environment = format!(
            "{} {} cores={}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            std::thread::available_parallelism()
                .map(|x| x.get())
                .unwrap_or(1)
        );
        r.attribution = format!("method={} parent={}", candidate.name(), parent.name());
        r.decision = decision.into();
        r.notes = format!(
            "archive_delta={archive_delta} binary_cost_measured={bin_cost} delta_S={delta_s}"
        );
        r.extra.push(("phase".into(), "optimization-a".into()));
        r.extra.push((
            "bits_per_byte".into(),
            format!("{:.6}", (cand_arch.len() as f64 * 8.0) / n),
        ));
        r.extra
            .push(("parent_archive_bytes".into(), parent_arch.len().to_string()));
        r.extra.push(("parent_exact".into(), parent_ok.to_string()));
        append_jsonl(&rp, &r)?;
    }
    if !(parent_ok && cand_ok) {
        return Err("eval: a method failed exactness".into());
    }
    Ok(())
}

/// A20: sweep the optimizer tuning variants. Because `tune` is a runtime
/// parameter carried in the archive header, every variant has identical
/// executable cost; only the archive bytes differ.
fn cmd_sweep(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or("sweep: need <in>")?;
    let get = |k: &str| -> Option<String> {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let method = match get("--method") {
        Some(s) => Method::from_name(&s).ok_or("sweep: unknown method")?,
        None => archive::ACCEPTED_METHOD,
    };
    let receipt_path = get("--receipt");
    let data = read(path)?;
    guard_encode(data.len() as u64, max_ram_override(args))?;
    let n = data.len() as f64;

    let mut best: Option<(u8, usize)> = None;
    let mut base_size = 0usize;
    println!("method={} tune sweep", method.name());
    for tune in 0..16u8 {
        let t0 = Instant::now();
        let arch = archive::encode_tuned(&data, method, tune);
        let dt = t0.elapsed();
        let ok = archive::decode(&arch).as_deref() == Some(&data[..]);
        if tune == 0 {
            base_size = arch.len();
        }
        println!(
            "  tune {tune} (lr idx {tune}): {:>12} bytes  {:.4} bpc  {:.3}s  exact={}",
            arch.len(),
            (arch.len() as f64 * 8.0) / n,
            dt.as_secs_f64(),
            ok
        );
        if !ok {
            return Err(format!("sweep: tune {tune} failed exactness"));
        }
        if best.map(|(_, s)| arch.len() < s).unwrap_or(true) {
            best = Some((tune, arch.len()));
        }
    }
    let (bt, bs) = best.unwrap();
    let delta_s = bs as i64 - base_size as i64;
    println!(
        "best tune = {bt} ({}), DeltaS vs tune 0 = {} bytes",
        bs, delta_s
    );
    println!(
        "decision: {}",
        if delta_s < 0 {
            "ADOPTED (learning-rate variant)"
        } else {
            "REJECTED (tune 0 already optimal)"
        }
    );

    if let Some(rp) = receipt_path {
        let mut r = RunReceipt::default();
        r.id = format!("opt-a/a20-sweep/{}/{}", method.name(), data.len());
        r.hypothesis =
            "a mixer learning-rate/update-law variant lowers complete S (zero binary cost)".into();
        r.parent = format!("{}@tune0", method.name());
        r.revision = revision();
        r.compiler = format!(
            "rustc {} {}-{}",
            rustc_version(),
            std::env::consts::OS,
            std::env::consts::ARCH
        );
        r.corpus = path.clone();
        r.input_sha256 = hex(&sha256(&data));
        r.exact = true;
        r.compressor_bytes = 0;
        r.archive_bytes = bs as u64;
        r.peak_rss_bytes = peak_rss();
        r.environment = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
        r.attribution = format!("A20 tune sweep, best tune={bt}");
        r.decision = if delta_s < 0 { "ADOPTED" } else { "REJECTED" }.into();
        r.notes = format!("best_tune={bt} base_bytes={base_size} delta_S={delta_s}");
        r.extra.push(("phase".into(), "optimization-a".into()));
        r.extra.push(("tune0_bytes".into(), base_size.to_string()));
        append_jsonl(&rp, &r)?;
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

fn cmd_meminfo(args: &[String]) -> Result<(), String> {
    let b = memory::budget(max_ram_override(args));
    println!("{}", memory::summary());
    println!(
        "budget_bytes={} ({:.2} GiB)",
        b,
        b as f64 / (1024.0 * 1024.0 * 1024.0)
    );
    if let Some(path) = args.iter().find(|a| !a.starts_with("--")) {
        let data = read(path)?;
        let n = data.len() as u64;
        let pe = memory::projected_encode(n, 2);
        let pd = memory::projected_decode(n / 3 + (1 << 20), n);
        let g = 1024.0 * 1024.0 * 1024.0;
        println!("file={path} bytes={n}");
        println!(
            "projected_encode={} ({:.2} GiB) {}",
            pe,
            pe as f64 / g,
            if pe <= b { "OK" } else { "BLOCKED" }
        );
        println!(
            "projected_decode={} ({:.2} GiB) {}",
            pd,
            pd as f64 / g,
            if pd <= b { "OK" } else { "BLOCKED" }
        );
        println!("model_bytes={}", memory::model_bytes(n as usize));
    }
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
