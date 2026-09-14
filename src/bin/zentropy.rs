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
//! zentropy eval      <in> --candidate <m> [--parent <m>] --binary-cost <n> [--tune <t>] [--parent-archive-bytes <n>] [--receipt <f>]
//! zentropy sweep     <in> [--method <method>] [--receipt <f>]
//! zentropy prune     <in> [--method <method>] [--remove i,j]
//! zentropy pack-sfx  <stub> <archive> <out>
//! zentropy corrupt-court
//! zentropy negative-court
//! zentropy gate
//! zentropy meminfo    [file] [--max-ram <size>]
//! zentropy selftest
//! ```
//!
//! Phase 9 adds the search subcommands, all research-plane and gated out of the
//! submission stub by the `submission` feature:
//!
//! ```text
//! zentropy sweep-tune <in> [--method M] [--schedule coord|full|fuzz|guided] [--trials N] [--tunes 0,1,…] [--jobs N] [--receipt <f>] [--memory <log>]…
//! zentropy frontier   <receipt.jsonl> [--method M]
//! zentropy observe    <receipt.jsonl> [--method M]
//! zentropy pblocks    <in> [--blocks N] [--jobs M] [--tune T] [--no-full]
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
    // OOM protection: the research driver arms the runtime memory floor, so a
    // multi-hour run aborts cleanly if the machine tightens underneath it rather
    // than pushing the user's session into swap. The judged stub never arms it,
    // and does not carry its implementation (`mem-floor` is outside `accepted`).
    memory::enable_runtime_guard();
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
        "prune" => cmd_prune(&args[2..]),
        "reorder-info" => cmd_reorder_info(&args[2..]),
        "reorder-out" => cmd_reorder_out(&args[2..]),
        "train-residual" => cmd_train_residual(&args[2..]),
        #[cfg(not(feature = "submission"))]
        "sweep-tune" => cmd_sweep_tune(&args[2..]),
        #[cfg(not(feature = "submission"))]
        "frontier" => cmd_frontier(&args[2..]),
        #[cfg(not(feature = "submission"))]
        "observe" => cmd_observe(&args[2..]),
        #[cfg(not(feature = "submission"))]
        "pblocks" => cmd_pblocks(&args[2..]),
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
         zentropy hash        <file>\n  \
         zentropy tokenize    <file> [--kinds]\n  \
         zentropy compress    <in> <archive>\n  \
         zentropy decompress  <archive> <out>\n  \
         zentropy verify      <original> <archive>\n  \
         zentropy bench       <in> [--out <archive>] [--receipt <f>]\n  \
         zentropy eval        <in> --candidate <m> [--parent <m>] --binary-cost <n> [--tune <t>] [--parent-archive-bytes <n>] [--receipt <f>]\n  \
         zentropy sweep       <in> [--method <m>] [--receipt <f>]\n  \
         zentropy prune       <in> [--method <m>] [--remove i,j]\n  \
         zentropy pack-sfx    <stub> <archive> <out>\n  \
         zentropy meminfo     [file] [--max-ram <size>]\n  \
         zentropy corrupt-court | negative-court | selftest | gate\n  \
         zentropy sweep-tune  <in> [--method <m>] [--schedule coord|full|fuzz|guided] [--trials <n>] [--tunes <list>] [--jobs <n>] [--receipt <f>] [--memory <log>]\n  \
         zentropy frontier    <receipt.jsonl> [--method <m>]\n  \
         zentropy observe     <receipt.jsonl> [--method <m>]\n  \
         zentropy pblocks     <in> [--blocks <n>] [--jobs <n>] [--tune <t>] [--no-full]\n",
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
///
/// The research driver shares the workstation with an editor, so it uses the
/// reserve-aware budget: it never claims [`memory::RESERVE_BYTES`]. The judged
/// stub uses the unreserved budget instead. It also arms the runtime floor, so a
/// long run aborts rather than swapping the machine to death.
fn guard_encode(n: u64, max_ram: Option<u64>) -> Result<(), String> {
    memory::check(
        memory::projected_encode(n, 2),
        memory::research_budget(max_ram),
    )
}

fn guard_decode(archive_len: u64, n: u64, max_ram: Option<u64>) -> Result<(), String> {
    memory::check(
        memory::projected_decode(archive_len, n),
        memory::research_budget(max_ram),
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
    println!(
        "method: {} (accepted configuration)",
        archive::ACCEPTED_METHOD.name()
    );
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

/// Phase 6.9: expert-roster pruning. For every expert in a method's model,
/// measure the archive size with that expert removed. A positive marginal value
/// means the expert earns its place; a negative value means removing it shrinks
/// the archive (law 7: delete negative-value experts). The measurement is
/// encode-only and research-scoped; any pruned roster is re-verified through a
/// real `Method` before adoption.
fn cmd_prune(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or("prune: need <in>")?;
    let get = |k: &str| -> Option<String> {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let method = get("--method")
        .and_then(|s| Method::from_name(&s))
        .unwrap_or(archive::ACCEPTED_METHOD);
    let tune: u8 = get("--tune")
        .and_then(|v| v.parse().ok())
        .unwrap_or(archive::ACCEPTED_TUNE);

    let data = read(path)?;
    guard_encode(data.len() as u64, max_ram_override(args))?;
    let cfg = method.config_for(data.len());
    let specs = cfg.specs.clone();

    // Optional direct combo test: `--remove 8,12,16` encodes only that roster.
    if let Some(list) = get("--remove") {
        let drop: Vec<usize> = list
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        let mut subset = specs.clone();
        for &i in drop.iter().rev() {
            if i < subset.len() {
                subset.remove(i);
            }
        }
        let t = Instant::now();
        let arch = archive::encode_specs(&data, method, tune, &subset);
        let full = archive::encode_specs(&data, method, tune, &specs);
        println!(
            "remove {drop:?}: full={} subset={} delta(S)={:+} ({:.2}s)",
            full.len(),
            arch.len(),
            arch.len() as i64 - full.len() as i64,
            t.elapsed().as_secs_f64()
        );
        return Ok(());
    }

    let t0 = Instant::now();
    let full = archive::encode_specs(&data, method, tune, &specs);
    println!(
        "full  {} experts  archive={} bytes  {:.3}s",
        specs.len(),
        full.len(),
        t0.elapsed().as_secs_f64()
    );
    println!(
        "idx  kind                          with={}  without  marginal(S)  verdict",
        full.len()
    );

    let mut rows: Vec<(i64, usize, String)> = Vec::new();
    for i in 0..specs.len() {
        let mut subset = specs.clone();
        let removed = subset.remove(i);
        let t = Instant::now();
        let arch = archive::encode_specs(&data, method, tune, &subset);
        // Positive marginal = the expert makes the archive smaller.
        let marginal = arch.len() as i64 - full.len() as i64;
        let label = format!("{:?}", removed.kind);
        rows.push((marginal, i, label));
        println!(
            "{:>3}  {:<28}        {:>10}  {:+9}  {:.2}s  {}",
            i,
            format!("{:?}", removed.kind),
            arch.len(),
            marginal,
            t.elapsed().as_secs_f64(),
            if marginal > 0 { "KEEP" } else { "PRUNE" }
        );
    }
    rows.sort();
    println!("\nmost negative (prune candidates):");
    for (m, i, label) in rows.iter().take(5) {
        println!("  idx {i:>3}  {label:<28}  marginal(S)={m:+}");
    }
    Ok(())
}

/// Phase 7 probe: report the page structure and the free-restoration
/// preconditions of a corpus, plus the cost of an explicit permutation of the
/// pages (7.7) so the saving from the id-sort is quantified, not assumed.
#[cfg(feature = "reorder")]
fn cmd_reorder_info(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or("reorder-info: need <in>")?;
    let data = read(path)?;
    let (pages, ascending) = zentropy::reorder::info(&data);
    let explicit = zentropy::reorder::explicit_permutation_bytes(pages);
    println!("input_bytes={}", data.len());
    println!("pages={pages}");
    println!("page_id_strictly_ascending={ascending}");
    println!("free_restoration_available={ascending}");
    println!("explicit_permutation_bytes={explicit}");
    println!("permutation_bytes_paid_by_id_sort=0");
    Ok(())
}

#[cfg(not(feature = "reorder"))]
fn cmd_reorder_info(_args: &[String]) -> Result<(), String> {
    Err("reorder-info: built without the `reorder` feature".into())
}

/// Phase 7 research helper: write a corpus reordered under one of the orderings,
/// so the transform can be inspected or fed to an external experiment.
#[cfg(feature = "reorder")]
fn cmd_reorder_out(args: &[String]) -> Result<(), String> {
    if args.len() < 3 {
        return Err("reorder-out: need <in> <order> <out>".into());
    }
    let data = read(&args[0])?;
    let order = match args[1].as_str() {
        "identity" => zentropy::reorder::Order::Identity,
        "title" => zentropy::reorder::Order::Title,
        "size" => zentropy::reorder::Order::Size,
        "struct" => zentropy::reorder::Order::Struct,
        "minhash" => zentropy::reorder::Order::MinHash,
        "greedy" => zentropy::reorder::Order::Greedy,
        "template" => zentropy::reorder::Order::Template,
        "category" => zentropy::reorder::Order::Category,
        "category-set" => zentropy::reorder::Order::CategorySet,
        "full" => zentropy::reorder::Order::Full,
        "full-residual" => zentropy::reorder::Order::FullResidual,
        "template-key" => zentropy::reorder::Order::TemplateKey,
        "shuffle" => zentropy::reorder::Order::Shuffle,
        other => return Err(format!("reorder-out: unknown order {other}")),
    };
    let out = zentropy::reorder::encode(&data, order)
        .ok_or("reorder-out: corpus does not satisfy the precondition")?;
    let restored = zentropy::reorder::restore(&out);
    if restored != data {
        return Err("reorder-out: restore did not reproduce the input".into());
    }
    write(&args[2], &out)?;
    eprintln!(
        "reorder-out: {} -> {} bytes (order={})",
        data.len(),
        out.len(),
        order.name()
    );
    Ok(())
}

#[cfg(not(feature = "reorder"))]
fn cmd_reorder_out(_args: &[String]) -> Result<(), String> {
    Err("reorder-out: built without the `reorder` feature".into())
}

/// Phase 8 research: train the learned residual corrector on a corpus prefix and
/// write the quantized weights. The trainer runs on the *transformed* stream the
/// predictor actually codes, so training sees the inference distribution.
#[cfg(feature = "learned")]
fn cmd_train_residual(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or("train-residual: need <in>")?;
    let get = |k: &str| -> Option<String> {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let hidden: usize = get("--hidden").and_then(|v| v.parse().ok()).unwrap_or(16);
    let lr: f32 = get("--lr").and_then(|v| v.parse().ok()).unwrap_or(0.02);
    let max_bytes: Option<usize> = get("--max-bytes").and_then(|v| v.parse().ok());
    let out = get("--out").unwrap_or_else(|| "src/learned/weights.bin".into());

    let raw = read(path)?;
    let raw = match max_bytes {
        Some(m) if m < raw.len() => &raw[..m],
        _ => &raw[..],
    };
    // Train on the same transformed stream the accepted configuration codes.
    let stream = archive::transformed_stream(raw, archive::ACCEPTED_METHOD, archive::ACCEPTED_TUNE);
    let n = stream.len();
    guard_encode(n as u64, max_ram_override(args))?;
    let cfg = archive::ACCEPTED_METHOD
        .config_for(n)
        .with_residual_train(hidden)
        .with_residual_lr(lr);
    let mut cm = zentropy::context::Cm::new(&cfg, n);
    let t0 = Instant::now();
    for &byte in stream.iter() {
        let mut mask = 0x80u32;
        while mask != 0 {
            let bit = if (byte as u32) & mask != 0 { 1 } else { 0 };
            let _ = cm.predict();
            cm.update(bit);
            mask >>= 1;
        }
    }
    let net = cm.take_residual_net().ok_or("train-residual: no trainer")?;
    let bytes = net.to_bytes();
    write(&out, &bytes)?;
    let (steps, loss, ema) = cm.residual_stats();
    println!(
        "hidden={hidden} lr={lr} steps={steps} mean_loss_bits_per_bit={loss:.6} recent_loss_bits_per_bit={ema:.6} model_bytes={} wall={:.1}s out={out}",
        bytes.len(),
        t0.elapsed().as_secs_f64()
    );
    Ok(())
}

#[cfg(not(feature = "learned"))]
fn cmd_train_residual(_args: &[String]) -> Result<(), String> {
    Err("train-residual: built without the `learned` feature".into())
}

/// Phase 9: measure one `tune` point for a method — encode, hash, receipt.
///
/// Research-plane: `exact` is `false` because a screening encode is not a
/// round-trip claim. The winner is gated by `eval`, which does reconstruct the
/// corpus byte-for-byte before anything is adopted.
#[cfg(not(feature = "submission"))]
#[allow(clippy::too_many_arguments)]
fn measure_tune(
    data: &[u8],
    method: Method,
    t: u8,
    corpus_path: &str,
    sha: &str,
    receipt: Option<&str>,
) -> Result<zentropy::search::Trial, String> {
    let (tr, arch) = encode_trial(data, method, t);
    write_tune_receipt(method, t, &tr, &arch, corpus_path, sha, receipt)?;
    Ok(tr)
}

/// Encode one tune and build its trial. Pure: no I/O, no shared state, so it is
/// safe to run many of these concurrently (`--jobs`).
#[cfg(not(feature = "submission"))]
fn encode_trial(data: &[u8], method: Method, t: u8) -> (zentropy::search::Trial, Vec<u8>) {
    let t0 = Instant::now();
    let arch = archive::encode_tuned(data, method, t);
    let wall = t0.elapsed().as_secs_f64();
    let tr = zentropy::search::Trial {
        tune: t,
        archive_bytes: arch.len() as u64,
        wall_ms: (wall * 1000.0) as u64,
        exact: false,
    };
    (tr, arch)
}

/// Receipt one measured trial. Kept separate from [`encode_trial`] so that a
/// parallel campaign writes receipts **serially and in tune order**, which keeps
/// the log deterministic and stops interleaved appends from corrupting lines.
#[cfg(not(feature = "submission"))]
#[allow(clippy::too_many_arguments)]
fn write_tune_receipt(
    method: Method,
    t: u8,
    tr: &zentropy::search::Trial,
    arch: &[u8],
    corpus_path: &str,
    sha: &str,
    receipt: Option<&str>,
) -> Result<(), String> {
    let Some(rp) = receipt else { return Ok(()) };
    let mut r = RunReceipt::default();
    r.id = format!("search/{}/{t}", method.name());
    r.hypothesis = format!(
        "tune={t} improves archive over the accepted tune for {}",
        method.name()
    );
    r.parent = method.name().into();
    r.revision = revision();
    r.compiler = format!("rustc {}", rustc_version());
    r.corpus = corpus_path.to_string();
    r.input_sha256 = sha.to_string();
    r.archive_sha256 = hex(&sha256(arch));
    r.exact = false;
    r.compressor_bytes = 0;
    r.archive_bytes = tr.archive_bytes;
    r.wall_seconds = tr.wall_ms as f64 / 1000.0;
    r.peak_rss_bytes = peak_rss();
    r.environment = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    r.attribution = "search/tune".into();
    r.decision = "MEASURED".into();
    r.notes = "runtime knob (header byte); screening encode, winner gated by `eval`".into();
    r.extra.push(("method".into(), method.name().into()));
    r.extra.push(("tune".into(), t.to_string()));
    let k = zentropy::search::Knobs::from_tune(t);
    r.extra.push(("lr_idx".into(), k.lr_idx.to_string()));
    r.extra.push(("apm_sel".into(), k.apm_sel.to_string()));
    let (r1, r2, r3) = zentropy::search::apm_rates(t);
    r.extra.push((
        "mixer_lr".into(),
        zentropy::context::MIXER_LRS[k.lr_idx as usize].to_string(),
    ));
    r.extra
        .push(("apm_rates".into(), format!("{r1},{r2},{r3}")));
    append_jsonl(rp, &r)
}

/// Phase 9: search the runtime hyperparameter space (`tune`) for a method. The
/// knob is carried in the archive header, so the decoder is unchanged and search
/// has no decode authority. Trials are recorded as receipts; the Gemel memory
/// (the receipt log) is consulted first so a known configuration is never paid
/// for twice.
#[cfg(not(feature = "submission"))]
fn cmd_sweep_tune(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or("sweep-tune: need <in>")?;
    let get = |k: &str| -> Option<String> {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let method = get("--method")
        .and_then(|s| Method::from_name(&s))
        .unwrap_or(archive::ACCEPTED_METHOD);
    let receipt = get("--receipt");
    let memory_paths: Vec<String> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| a.as_str() == "--memory")
        .filter_map(|(i, _)| args.get(i + 1).cloned())
        .collect();
    let schedule = get("--schedule").unwrap_or_else(|| "full".into());
    let fuzz_n: usize = get("--trials").and_then(|v| v.parse().ok()).unwrap_or(64);
    // Research-plane parallelism (rayon): how many trials to encode concurrently.
    // Each concurrent trial owns a full model, so this multiplies peak RAM and
    // the startup guard below is charged accordingly.
    let jobs: usize = get("--jobs")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
        .max(1);
    // An explicit point list shards a campaign across processes: each shard owns a
    // disjoint set of tunes and its own receipt, and the Gemel memory keeps the
    // shards consistent when their logs are pooled. `--tunes` overrides
    // `--schedule`.
    let explicit: Option<Vec<u8>> = match get("--tunes") {
        Some(s) => {
            let mut v: Vec<u8> = Vec::new();
            for part in s.split(',') {
                let p = part.trim();
                if p.is_empty() {
                    continue;
                }
                let t: u8 = p
                    .parse()
                    .map_err(|_| format!("sweep-tune: --tunes entry '{p}' is not a tune byte"))?;
                v.push(t);
            }
            v.sort_unstable();
            v.dedup();
            Some(v)
        }
        None => None,
    };

    let data = read(path)?;
    // `jobs` concurrent trials each hold a model, so the projection scales with
    // them instead of describing a single pass.
    memory::check(
        memory::projected_encode(data.len() as u64, 2).saturating_mul(jobs as u64),
        memory::research_budget(max_ram_override(args)),
    )?;
    let sha = hex(&sha256(&data));

    // Gemel memory: every receipt log named here (the append target first, then
    // any `--memory` archives) contributes the configurations already measured
    // for this method *on this corpus*. Filtering by the corpus digest is what
    // makes the memory sound: a tune tried on enwik7 says nothing about enwik9,
    // and skipping it on that basis would silently drop a real candidate.
    let mut memory_lines: Vec<String> = Vec::new();
    let read_log = |p: &str| -> Vec<String> {
        std::fs::read_to_string(p)
            .map(|s| s.lines().map(|l| l.to_string()).collect())
            .unwrap_or_default()
    };
    if let Some(r) = &receipt {
        memory_lines.extend(read_log(r));
    }
    for m in &memory_paths {
        memory_lines.extend(read_log(m));
    }
    let mut tried: std::collections::HashSet<u8> =
        zentropy::search::already_tried_for(&memory_lines, method.name(), Some(&sha))
            .into_iter()
            .collect();

    // Every trial known for this corpus (from memory), plus everything this
    // campaign measures. The observer and the schedules see one trial set.
    let mut all = zentropy::search::trials_from_lines_for(&memory_lines, method.name(), Some(&sha));
    let known = all.len();

    // Measure one point unless the Gemel memory already knows it; this is the
    // single path every schedule goes through, so no trial can be paid for twice
    // and every trial is receipted identically.
    let run = |t: u8,
               tried: &mut std::collections::HashSet<u8>,
               all: &mut Vec<zentropy::search::Trial>|
     -> Result<(), String> {
        if tried.contains(&t) {
            return Ok(());
        }
        let tr = measure_tune(&data, method, t, path, &sha, receipt.as_deref())?;
        tried.insert(t);
        all.push(tr);
        Ok(())
    };
    let min_archive = |all: &[zentropy::search::Trial], t: u8| -> Option<u64> {
        all.iter()
            .filter(|x| x.tune == t)
            .map(|x| x.archive_bytes)
            .min()
    };

    // Evaluate a *fixed* point list, `jobs` trials at a time. The encodes are
    // independent, so they run in parallel; receipts are then written serially in
    // tune order, which keeps the log byte-identical to a serial campaign.
    // Batching (rather than a free-for-all) bounds peak RAM to `jobs` models.
    let run_batch = |plan: &[u8],
                     tried: &mut std::collections::HashSet<u8>,
                     all: &mut Vec<zentropy::search::Trial>|
     -> Result<(), String> {
        for chunk in plan.chunks(jobs) {
            let todo: Vec<u8> = chunk
                .iter()
                .copied()
                .filter(|t| !tried.contains(t))
                .collect();
            if todo.is_empty() {
                continue;
            }
            // Concurrent passes would interleave their progress lines, so the
            // status line is silenced for the duration of the batch.
            #[cfg(feature = "progress")]
            zentropy::progress::set(false);
            #[cfg(feature = "parallel")]
            let mut measured: Vec<(u8, zentropy::search::Trial, Vec<u8>)> = {
                use rayon::prelude::*;
                todo.par_iter()
                    .map(|&t| {
                        let (tr, a) = encode_trial(&data, method, t);
                        (t, tr, a)
                    })
                    .collect()
            };
            #[cfg(not(feature = "parallel"))]
            let mut measured: Vec<(u8, zentropy::search::Trial, Vec<u8>)> = todo
                .iter()
                .map(|&t| {
                    let (tr, a) = encode_trial(&data, method, t);
                    (t, tr, a)
                })
                .collect();
            measured.sort_by_key(|(t, _, _)| *t);
            #[cfg(feature = "progress")]
            zentropy::progress::set(true);
            for (t, tr, arch) in measured {
                write_tune_receipt(method, t, &tr, &arch, path, &sha, receipt.as_deref())?;
                tried.insert(t);
                all.push(tr);
            }
        }
        Ok(())
    };

    // The starting point is the accepted tune.
    let start = zentropy::search::Knobs::from_tune(archive::ACCEPTED_TUNE);

    match explicit {
        Some(list) => {
            run_batch(&list, &mut tried, &mut all)?;
        }
        None => match schedule.as_str() {
            "full" => {
                let plan: Vec<u8> = (0u16..256).map(|t| t as u8).collect();
                run_batch(&plan, &mut tried, &mut all)?;
            }
            "coord" => {
                use zentropy::search::AXES;
                let mut plan: Vec<u8> = Vec::new();
                for a in AXES {
                    for lvl in 0..16u8 {
                        plan.push(start.set(a, lvl).tune());
                    }
                }
                run_batch(&plan, &mut tried, &mut all)?;
            }
            "fuzz" => {
                let mut k = start;
                let mut seed = 0x5EED_C0FF_EE12_3456u64;
                let mut plan: Vec<u8> = Vec::new();
                for _ in 0..fuzz_n {
                    plan.push(k.tune());
                    let (nk, ns) = zentropy::search::mutate(k, seed);
                    k = nk;
                    seed = ns;
                }
                run_batch(&plan, &mut tried, &mut all)?;
            }
            // The guided campaign is the phase's synthesis. The DSFB observer chooses
            // the next points (coordinate descent to a per-axis optimum), then the
            // frf-fuzz operator escapes that optimum with a deterministic, seeded
            // hill-climb that halts after `patience` consecutive non-improvements. The
            // Gemel memory is consulted throughout, so resuming costs nothing.
            "guided" => {
                use zentropy::search::AXES;
                let mut best = start;
                for _round in 0..4 {
                    let mut improved = false;
                    for a in AXES {
                        for lvl in 0..16u8 {
                            run(best.set(a, lvl).tune(), &mut tried, &mut all)?;
                        }
                        let mut best_lvl = best.level(a);
                        let mut best_bytes = u64::MAX;
                        for lvl in 0..16u8 {
                            if let Some(b) = min_archive(&all, best.set(a, lvl).tune()) {
                                if b < best_bytes {
                                    best_bytes = b;
                                    best_lvl = lvl;
                                }
                            }
                        }
                        if best_lvl != best.level(a) {
                            best = best.set(a, best_lvl);
                            improved = true;
                        }
                    }
                    if !improved {
                        break;
                    }
                }
                let patience = fuzz_n.clamp(1, 16);
                let mut seed = 0x5EED_C0FF_EE12_3456u64;
                let mut stale = 0usize;
                while stale < patience {
                    let (cand, ns) = zentropy::search::mutate(best, seed);
                    seed = ns;
                    run(cand.tune(), &mut tried, &mut all)?;
                    let cur = min_archive(&all, best.tune()).unwrap_or(u64::MAX);
                    let new = min_archive(&all, cand.tune()).unwrap_or(u64::MAX);
                    if new < cur {
                        best = cand;
                        stale = 0;
                    } else {
                        stale += 1;
                    }
                }
            }
            other => return Err(format!("sweep-tune: unknown schedule {other}")),
        },
    }

    let new_trials = all.len() - known;
    let o = zentropy::search::observe(&all);
    let best = zentropy::search::best_per_tune(&all);
    println!(
        "method={} corpus_sha={} trials={} (new {})",
        method.name(),
        &sha[..16],
        all.len(),
        new_trials
    );
    println!("best tune={} archive={} bytes", o.best_tune, o.best_archive);
    println!(
        "observer: lr_idx={} apm_sel={} coordinate_consistent={} lr_spread={:.0} apm_spread={:.0}",
        o.best_lr_idx, o.best_apm_sel, o.coordinate_consistent, o.lr_spread, o.apm_spread
    );
    println!("best 5:");
    for t in best.iter().take(5) {
        let k = zentropy::search::Knobs::from_tune(t.tune);
        println!(
            "  tune={:>3} lr_idx={:>2} apm_sel={:>2} archive={}",
            t.tune, k.lr_idx, k.apm_sel, t.archive_bytes
        );
    }
    Ok(())
}

/// Parallel-blocking probe (research). What would splitting the corpus into N
/// independently-coded blocks cost in *ratio*, and buy in wall-clock?
///
/// This is deliberately not a format change: each block is encoded as a
/// standalone archive with the accepted method, so the summed byte count is the
/// size a blocked format would store (plus a small container header). Every block
/// is decoded and the pieces are reassembled and compared against the input, so
/// the ratio cost reported here is exact rather than estimated. Promoting this to
/// the scored format is a separate decision that needs the number this prints.
#[cfg(not(feature = "submission"))]
fn cmd_pblocks(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or("pblocks: need <in>")?;
    let get = |k: &str| -> Option<String> {
        args.iter()
            .position(|a| a == k)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let blocks: usize = get("--blocks")
        .and_then(|v| v.parse().ok())
        .unwrap_or(8)
        .max(1);
    let jobs: usize = get("--jobs")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
        .max(1);
    let tune: u8 = get("--tune")
        .and_then(|v| v.parse().ok())
        .unwrap_or(archive::ACCEPTED_TUNE);
    let method = get("--method")
        .and_then(|s| Method::from_name(&s))
        .unwrap_or(archive::ACCEPTED_METHOD);
    let skip_full = args.iter().any(|a| a == "--no-full");

    let data = read(path)?;
    let n = data.len();
    let block_len = n.div_ceil(blocks);
    // Per-block models are smaller than one full-corpus model, but `jobs` of
    // them are live at once, so the guard is charged for that many.
    memory::check(
        memory::projected_encode(block_len as u64, 2).saturating_mul(jobs as u64),
        memory::research_budget(max_ram_override(args)),
    )?;

    let ranges: Vec<(usize, usize)> = (0..blocks)
        .map(|b| {
            let s = b * block_len;
            (s, (s + block_len).min(n))
        })
        .filter(|(s, e)| e > s)
        .collect();
    eprintln!(
        "pblocks: {} bytes, {} block(s), jobs={}, tune={tune}, method={}",
        n,
        ranges.len(),
        jobs,
        method.name()
    );

    // Reference: the single sequential pass.
    let mut full_bytes: Option<u64> = None;
    let mut full_s = 0.0f64;
    if !skip_full {
        let t = Instant::now();
        let full = archive::encode_tuned(&data, method, tune);
        full_s = t.elapsed().as_secs_f64();
        full_bytes = Some(full.len() as u64);
    }

    let t = Instant::now();
    let mut parts: Vec<Vec<u8>> = Vec::with_capacity(ranges.len());
    #[cfg(feature = "progress")]
    zentropy::progress::set(false);
    for chunk in ranges.chunks(jobs) {
        #[cfg(feature = "parallel")]
        let mut out: Vec<Vec<u8>> = {
            use rayon::prelude::*;
            chunk
                .par_iter()
                .map(|&(s, e)| archive::encode_tuned(&data[s..e], method, tune))
                .collect()
        };
        #[cfg(not(feature = "parallel"))]
        let mut out: Vec<Vec<u8>> = chunk
            .iter()
            .map(|&(s, e)| archive::encode_tuned(&data[s..e], method, tune))
            .collect();
        parts.append(&mut out);
    }
    #[cfg(feature = "progress")]
    zentropy::progress::set(true);
    let blocked_s = t.elapsed().as_secs_f64();

    // Exactness: decode every block, reassemble, compare.
    let mut rebuilt: Vec<u8> = Vec::with_capacity(n);
    let mut all_exact = true;
    for (i, p) in parts.iter().enumerate() {
        let (s, e) = ranges[i];
        match archive::decode(p) {
            Some(d) if d == data[s..e] => rebuilt.extend_from_slice(&d),
            _ => {
                all_exact = false;
                break;
            }
        }
    }
    let exact = all_exact && rebuilt == data;

    let blocked_bytes: u64 = parts.iter().map(|p| p.len() as u64).sum();
    println!(
        "input_bytes={n} blocks={} jobs={jobs} tune={tune}",
        ranges.len()
    );
    for (i, p) in parts.iter().enumerate() {
        let (s, e) = ranges[i];
        println!(
            "  block {:>3}  input {:>10}  archive {:>10}",
            i,
            e - s,
            p.len()
        );
    }
    println!("blocked_total_bytes = {blocked_bytes}  wall = {blocked_s:.3}s");
    if let Some(fb) = full_bytes {
        let delta = blocked_bytes as i64 - fb as i64;
        println!("single_pass_bytes   = {fb}  wall = {full_s:.3}s");
        println!(
            "ratio_cost          = {delta} bytes ({:+.4}%)",
            (delta as f64) * 100.0 / (fb as f64)
        );
        println!(
            "wall_speedup        = {:.2}x   (jobs={jobs}, blocks={})",
            full_s / blocked_s,
            ranges.len()
        );
    }
    println!("exact = {exact}");
    if !exact {
        return Err("pblocks: a block did not round-trip".into());
    }
    Ok(())
}

/// Phase 9: read a receipt log and print the Pareto frontier of search trials.
#[cfg(not(feature = "submission"))]
fn cmd_frontier(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or("frontier: need <receipt.jsonl>")?;
    let method = args
        .iter()
        .position(|a| a == "--method")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| archive::ACCEPTED_METHOD.name().to_string());
    let lines: Vec<String> = std::fs::read_to_string(path)
        .map_err(|e| format!("frontier: {e}"))?
        .lines()
        .map(|l| l.to_string())
        .collect();
    let trials = zentropy::search::trials_from_lines(&lines, &method);
    if trials.is_empty() {
        println!("frontier: no search trials for method={method}");
        return Ok(());
    }
    let o = zentropy::search::observe(&trials);
    let best = zentropy::search::best_per_tune(&trials);
    println!(
        "method={method} trials={} best_tune={} best_archive={}",
        o.n, o.best_tune, o.best_archive
    );
    println!("frontier (all tunes attaining the best archive):");
    for t in zentropy::search::pareto(&trials) {
        let k = zentropy::search::Knobs::from_tune(t.tune);
        println!(
            "  tune={:>3} lr_idx={:>2} apm_sel={:>2} archive={}",
            t.tune, k.lr_idx, k.apm_sel, t.archive_bytes
        );
    }
    println!("top 8 by archive:");
    for t in best.iter().take(8) {
        let k = zentropy::search::Knobs::from_tune(t.tune);
        println!(
            "  tune={:>3} lr_idx={:>2} apm_sel={:>2} archive={}",
            t.tune, k.lr_idx, k.apm_sel, t.archive_bytes
        );
    }
    Ok(())
}

/// Phase 9: the DSFB observer over a receipt log.
#[cfg(not(feature = "submission"))]
fn cmd_observe(args: &[String]) -> Result<(), String> {
    let path = args.first().ok_or("observe: need <receipt.jsonl>")?;
    let method = args
        .iter()
        .position(|a| a == "--method")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| archive::ACCEPTED_METHOD.name().to_string());
    let lines: Vec<String> = std::fs::read_to_string(path)
        .map_err(|e| format!("observe: {e}"))?
        .lines()
        .map(|l| l.to_string())
        .collect();
    let trials = zentropy::search::trials_from_lines(&lines, &method);
    if trials.is_empty() {
        println!("observe: no search trials for method={method}");
        return Ok(());
    }
    let o = zentropy::search::observe(&trials);
    println!(
        "trials={} best_tune={} best_archive={}",
        o.n, o.best_tune, o.best_archive
    );
    println!(
        "coordinate optimum: lr_idx={} apm_sel={} consistent={}",
        o.best_lr_idx, o.best_apm_sel, o.coordinate_consistent
    );
    println!("lr level means (lower is better):");
    for i in 0..16 {
        if o.lr_mean[i].is_finite() {
            println!("  lr_idx={:>2} mean={:.0}", i, o.lr_mean[i]);
        }
    }
    println!("apm level means:");
    for i in 0..16 {
        if o.apm_mean[i].is_finite() {
            println!("  apm_sel={:>2} mean={:.0}", i, o.apm_mean[i]);
        }
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
    // Reusing an already-measured parent is *not* estimating. The parent is the
    // accepted configuration, and its archive on this corpus is normally already
    // established by an accepted-baseline receipt. Re-encoding it and re-decoding
    // it costs two of the gate's four full passes — the dominant cost of a gate —
    // and proves nothing that the baseline receipt does not already prove, since
    // the encoder is deterministic. Supplying `--parent-archive-bytes <n>` skips
    // those two passes; the *candidate* is still encoded and decoded in this run,
    // and the receipt records where the parent number came from.
    let parent_known: Option<u64> = match get("--parent-archive-bytes") {
        Some(s) => Some(
            s.trim()
                .parse::<u64>()
                .map_err(|_| format!("eval: --parent-archive-bytes '{s}' is not a byte count"))?,
        ),
        None => None,
    };

    let data = read(path)?;
    guard_encode(data.len() as u64, max_ram_override(args))?;

    let (parent_bytes, parent_s, parent_ok, parent_src) = match parent_known {
        Some(n) => (
            n,
            std::time::Duration::ZERO,
            true,
            "supplied:receipted-baseline",
        ),
        None => {
            let t0 = Instant::now();
            let pa = archive::encode_tuned(&data, parent, parent_tune);
            let dt = t0.elapsed();
            let ok = archive::decode(&pa).as_deref() == Some(&data[..]);
            (pa.len() as u64, dt, ok, "measured:this-run")
        }
    };

    let t1 = Instant::now();
    eprintln!("eval: [1/2] encoding the candidate — this is the long pass");
    let cand_arch = archive::encode_tuned(&data, candidate, tune);
    let cand_s = t1.elapsed();

    // Decode the candidate once: the exactness court and the receipt's decoded
    // digest both need the result, and on enwik9 a second decode costs ~30
    // minutes.
    eprintln!("eval: [2/2] decoding the candidate to prove exactness");
    let cand_dec = archive::decode(&cand_arch);
    let cand_ok = cand_dec.as_deref() == Some(&data[..]);

    let n = data.len() as f64;
    let archive_delta = cand_arch.len() as i64 - parent_bytes as i64;
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
        "parent    {:<26} {:>12} bytes  {:.4} bpc  {:.3}s  exact={}  source={}",
        parent.name(),
        parent_bytes,
        (parent_bytes as f64 * 8.0) / n,
        parent_s.as_secs_f64(),
        parent_ok,
        parent_src
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
        r.extra.push(("tune".into(), tune.to_string()));
        r.extra
            .push(("parent_tune".into(), parent_tune.to_string()));
        r.extra
            .push(("parent_source".into(), parent_src.to_string()));
        r.extra.push((
            "bits_per_byte".into(),
            format!("{:.6}", (cand_arch.len() as f64 * 8.0) / n),
        ));
        r.extra
            .push(("parent_archive_bytes".into(), parent_bytes.to_string()));
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
    // The research budget is what this driver would actually apply; the judged
    // budget is what the submission stub applies. Printing both makes the
    // difference explicit (the reserve the driver keeps for the workstation).
    let b = memory::research_budget(max_ram_override(args));
    #[cfg(feature = "mem-floor")]
    println!("{}", memory::summary());
    #[cfg(not(feature = "mem-floor"))]
    println!(
        "{} bytes available; run floor compiled out (feature `mem-floor`)",
        memory::available_bytes().unwrap_or(0)
    );
    println!(
        "research_budget_bytes={} ({:.2} GiB)",
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
