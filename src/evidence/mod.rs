//! Evidence-first discipline: immutable experiment receipts and the Gemel
//! research memory.
//!
//! Two constitutional rules live here:
//!
//! * **Nothing becomes a claim without a court.** Every result binds the source
//!   revision, compiler identity, flags, input hash, produced artefact hashes,
//!   sizes, resource measurements and environment (§24).
//! * **Never forget a failed idea.** Failed experiments are first-class and are
//!   never deleted; before implementing something, the Gemel memory is queried
//!   to avoid circular rediscovery (§23).
//!
//! Records are JSON Lines: one object per line, append-only. The format is
//! deliberately simple and dependency-free so it stays readable for decades and
//! costs nothing in the scored binary (the submission plane never links it).

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Minimal JSON string escaping. The only characters that must be escaped.
fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// A measured run of a candidate representation.
#[derive(Debug, Clone, Default)]
pub struct RunReceipt {
    /// Stable identifier: `<mechanism>/<config-hash>/<timestamp-or-counter>`.
    pub id: String,
    /// Hypothesis this run tests.
    pub hypothesis: String,
    /// Parent candidate id, for the experiment DAG.
    pub parent: String,
    /// Source revision (e.g. git describe) or `"<untracked>"`.
    pub revision: String,
    /// Compiler identity, e.g. `"rustc 1.98.0 LTO=fat opt=3"`.
    pub compiler: String,
    /// Input corpus name.
    pub corpus: String,
    /// Input SHA-256 (lowercase hex).
    pub input_sha256: String,
    /// Produced archive SHA-256.
    pub archive_sha256: String,
    /// SHA-256 of the bytes produced by decoding the archive.
    pub decoded_sha256: String,
    /// Whether `decoded == input`.
    pub exact: bool,
    /// Compressor bytes.
    pub compressor_bytes: u64,
    /// Archive bytes.
    pub archive_bytes: u64,
    /// Wall time, seconds (compression + decompression as recorded).
    pub wall_seconds: f64,
    /// CPU time, seconds.
    pub cpu_seconds: f64,
    /// Peak resident set size, bytes.
    pub peak_rss_bytes: u64,
    /// Peak temporary disk, bytes.
    pub temp_disk_bytes: u64,
    /// Free-form environment descriptor.
    pub environment: String,
    /// Attribution note (leave-one-out, ablation id, ...).
    pub attribution: String,
    /// Decision: PROPOSED/MEASURED/ADOPTED/REJECTED/SUPERSEDED/INCONCLUSIVE/RESEARCH_ONLY.
    pub decision: String,
    /// Unexpected residuals worth following up.
    pub notes: String,
    /// Extra key/value pairs (e.g. `bits_per_byte`, `delta_vs_parent`).
    pub extra: Vec<(String, String)>,
}

impl RunReceipt {
    /// Serialise to a single JSON object (no trailing newline).
    pub fn to_json(&self) -> String {
        let mut s = String::new();
        s.push('{');
        let mut first = true;
        let mut field = |s: &mut String, k: &str, v: &str| {
            if !first {
                s.push(',');
            }
            first = false;
            s.push('"');
            s.push_str(&escape_json(k));
            s.push_str("\":\"");
            s.push_str(&escape_json(v));
            s.push('"');
        };
        field(&mut s, "id", &self.id);
        field(&mut s, "hypothesis", &self.hypothesis);
        field(&mut s, "parent", &self.parent);
        field(&mut s, "revision", &self.revision);
        field(&mut s, "compiler", &self.compiler);
        field(&mut s, "corpus", &self.corpus);
        field(&mut s, "input_sha256", &self.input_sha256);
        field(&mut s, "archive_sha256", &self.archive_sha256);
        field(&mut s, "decoded_sha256", &self.decoded_sha256);
        field(&mut s, "exact", if self.exact { "true" } else { "false" });
        field(
            &mut s,
            "compressor_bytes",
            &self.compressor_bytes.to_string(),
        );
        field(&mut s, "archive_bytes", &self.archive_bytes.to_string());
        field(
            &mut s,
            "score_total",
            &(self.compressor_bytes + self.archive_bytes).to_string(),
        );
        field(&mut s, "wall_seconds", &format!("{:.6}", self.wall_seconds));
        field(&mut s, "cpu_seconds", &format!("{:.6}", self.cpu_seconds));
        field(&mut s, "peak_rss_bytes", &self.peak_rss_bytes.to_string());
        field(&mut s, "temp_disk_bytes", &self.temp_disk_bytes.to_string());
        field(&mut s, "environment", &self.environment);
        field(&mut s, "attribution", &self.attribution);
        field(&mut s, "decision", &self.decision);
        field(&mut s, "notes", &self.notes);
        for (k, v) in &self.extra {
            field(&mut s, k, v);
        }
        s.push('}');
        s
    }
}

/// Append-only store rooted at a directory (conventionally `evidence/runs/`).
#[derive(Debug, Clone)]
pub struct Store {
    file: PathBuf,
}

impl Store {
    /// Open (creating if needed) the store at `dir/receipts.jsonl`.
    pub fn open_dir(dir: impl AsRef<Path>) -> io::Result<Self> {
        fs::create_dir_all(dir.as_ref())?;
        Ok(Store {
            file: dir.as_ref().join("receipts.jsonl"),
        })
    }

    /// Append a receipt. Never overwrites. Returns the exact line written.
    pub fn append(&self, r: &RunReceipt) -> io::Result<String> {
        let line = r.to_json();
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file)?;
        writeln!(f, "{line}")?;
        Ok(line)
    }

    /// Read every receipt line (raw). Used by the Gemel query path.
    pub fn read_lines(&self) -> io::Result<Vec<String>> {
        match fs::read_to_string(&self.file) {
            Ok(s) => Ok(s.lines().map(|l| l.to_string()).collect()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    /// Naive substring query over receipts, used as the "has this been tried?"
    /// oracle before starting new work.
    pub fn query(&self, needle: &str) -> io::Result<Vec<String>> {
        Ok(self
            .read_lines()?
            .into_iter()
            .filter(|l| l.contains(needle))
            .collect())
    }

    pub fn path(&self) -> &Path {
        &self.file
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_escaping() {
        let mut r = RunReceipt::default();
        r.id = "a\"b\\c".into();
        r.hypothesis = "line1\nline2\ttab".into();
        r.exact = true;
        r.compressor_bytes = 10;
        r.archive_bytes = 20;
        let j = r.to_json();
        assert!(j.contains("\\\""));
        assert!(j.contains("\\n"));
        assert!(j.contains("\"score_total\":\"30\""));
        assert!(j.contains("\"exact\":\"true\""));
    }

    #[test]
    fn store_append_and_query() {
        let dir = std::env::temp_dir().join(format!("zentropy_ev_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = Store::open_dir(&dir).unwrap();
        let mut r = RunReceipt::default();
        r.id = "grammar/oracle/1".into();
        r.decision = "REJECTED".into();
        r.notes = "negative value: costs 400KB, saves 300KB".into();
        store.append(&r).unwrap();
        let hits = store.query("grammar").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(store.query("nonexistent").unwrap().len(), 0);
        let _ = fs::remove_dir_all(&dir);
    }
}
