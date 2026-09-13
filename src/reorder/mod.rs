//! Phase 7 — the article-layout compiler.
//!
//! enwik9 is a concatenation of `<page>` blocks. Every start is the literal
//! `"  <page>\n"` and every end `"  </page>\n"`; the **page id** (the first
//! `<id>` inside a block) is strictly ascending across the corpus. Because the id
//! travels inside the block, the original order is recoverable by a stable sort
//! on that id: the permutation costs **zero archive bytes**, and only the
//! compressibility of the reordered stream can change.
//!
//! The encoder may therefore spend unbounded offline search on an order that
//! makes the downstream model's job easier; the decoder is a sort.
//!
//! Only compiled behind the `reorder` feature.

use std::collections::VecDeque;

/// The ordering the encoder applies. The decoder does not need it: every order
/// is restored by a page-id sort.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// Control: the split/permute/reassemble machinery with the original order.
    Identity,
    /// Bytewise title order (alphabetical grouping).
    Title,
    /// Page byte-length order (structural size grouping).
    Size,
    /// Namespace group, then title (templates/categories/etc. together).
    Struct,
    /// Word-shingle MinHash signature order (content similarity).
    MinHash,
    /// Windowed greedy nearest-neighbour over the MinHash order.
    Greedy,
    /// Boilerplate-signature order: MinHash over the template/category/link
    /// names a page uses, so pages sharing markup machinery cluster.
    Template,
    /// First-category order: group category members together, then by title.
    Category,
    /// All-categories order: group pages sharing a category *set*, then title.
    CategorySet,
    /// Combined markup order: category set, then template set, then title.
    Full,
    /// Residual order: category set, template set, then a cheap local-model
    /// novelty score (the page's residual under an order-2 model reset per page),
    /// then title. Groups large categories by how much *novel* structure they
    /// carry.
    FullResidual,
    /// First-template order: group pages whose leading markup is the same kind.
    TemplateKey,
    /// Negative control: a deterministic shuffle destroys all locality while
    /// keeping the page set and the permutation machinery identical.
    Shuffle,
}

impl Order {
    pub fn name(self) -> &'static str {
        match self {
            Order::Identity => "identity",
            Order::Title => "title",
            Order::Size => "size",
            Order::Struct => "struct",
            Order::MinHash => "minhash",
            Order::Greedy => "greedy",
            Order::Template => "template",
            Order::Category => "category",
            Order::CategorySet => "category-set",
            Order::Full => "full",
            Order::FullResidual => "full-residual",
            Order::TemplateKey => "template-key",
            Order::Shuffle => "shuffle",
        }
    }
}

const PAGE_OPEN: &[u8] = b"  <page>\n";
const PAGE_CLOSE: &[u8] = b"  </page>\n";

/// A corpus split into the bytes before the first page, the complete page
/// blocks, and everything after the last closing tag. Reassembly is
/// `header ++ pages ++ tail`, byte for byte, and the pieces tile the input.
pub struct Split<'a> {
    pub header: &'a [u8],
    pub pages: Vec<&'a [u8]>,
    pub tail: &'a [u8],
}

/// All start offsets of `needle` in `hay`, ascending, non-overlapping.
fn find_all(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    let n = needle.len();
    let mut out = Vec::new();
    if n == 0 || hay.len() < n {
        return out;
    }
    let last = hay.len() - n;
    let first = needle[0];
    let mut i = 0usize;
    while i <= last {
        if hay[i] == first && &hay[i..i + n] == needle {
            out.push(i);
            i += n;
        } else {
            i += 1;
        }
    }
    out
}

/// Split `input` into header, complete page blocks and tail. Returns `None` when
/// the input is not a clean page stream (no pages, mispaired tags, or bytes
/// between blocks that the split would drop), so callers fall back to identity.
pub fn split(input: &[u8]) -> Option<Split<'_>> {
    let starts = find_all(input, PAGE_OPEN);
    if starts.is_empty() {
        return None;
    }
    let ends = find_all(input, PAGE_CLOSE);
    // Either every page is closed, or exactly one trailing page is truncated.
    if ends.is_empty() || starts.len() < ends.len() || starts.len() > ends.len() + 1 {
        return None;
    }
    let complete = ends.len();
    let mut pages: Vec<&[u8]> = Vec::with_capacity(complete);
    for i in 0..complete {
        let s = starts[i];
        let e = ends[i];
        if e <= s {
            return None;
        }
        // Blocks must tile the input: nothing may sit between a close and the
        // next open, or the split would silently drop bytes.
        if i + 1 < complete && ends[i] + PAGE_CLOSE.len() != starts[i + 1] {
            return None;
        }
        if i + 1 == complete {
            // After the last complete page: either the trailing truncated page
            // (a further start) or the plain tail — and nothing between the
            // close and that start.
            if starts.len() == complete + 1 && ends[i] + PAGE_CLOSE.len() != starts[complete] {
                return None;
            }
        }
        pages.push(&input[s..e + PAGE_CLOSE.len()]);
    }
    if starts.len() != complete + 1 && starts.len() != complete {
        return None;
    }
    let header = &input[..starts[0]];
    let tail_start = ends[complete - 1] + PAGE_CLOSE.len();
    let tail = &input[tail_start..];
    Some(Split {
        header,
        pages,
        tail,
    })
}

/// The page id: the first `<id>NNN</id>` after the `<page>` tag.
pub fn page_id(page: &[u8]) -> Option<u64> {
    let mut i = 0usize;
    while i + 4 <= page.len() {
        if &page[i..i + 4] == b"<id>" {
            let mut j = i + 4;
            let mut v: u64 = 0;
            let mut any = false;
            while j < page.len() && page[j].is_ascii_digit() {
                v = v.saturating_mul(10).saturating_add((page[j] - b'0') as u64);
                any = true;
                j += 1;
            }
            if any && j + 5 <= page.len() && &page[j..j + 5] == b"</id>" {
                return Some(v);
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    None
}

/// The page title bytes (inside `<title>...</title>`).
fn page_title(page: &[u8]) -> &[u8] {
    if let Some(a) = find_all(page, b"<title>").first() {
        let s = a + 7;
        if let Some(b) = find_all(&page[s..], b"</title>").first() {
            return &page[s..s + b];
        }
    }
    &[]
}

/// The namespace group of a title, for the structural order. Articles (no
/// recognised prefix) sort together; the named namespaces sort after them.
fn namespace_rank(title: &[u8]) -> u8 {
    const NS: [&[u8]; 11] = [
        b"Template",
        b"Category",
        b"Wikipedia",
        b"Image",
        b"User",
        b"Help",
        b"Portal",
        b"MediaWiki",
        b"Talk",
        b"Media",
        b"Special",
    ];
    if let Some(colon) = title.iter().position(|&b| b == b':') {
        let prefix = &title[..colon];
        for (i, ns) in NS.iter().enumerate() {
            if prefix.eq_ignore_ascii_case(ns) {
                return (i + 1) as u8;
            }
        }
    }
    0
}

/// splitmix64: a deterministic, dependency-free 64-bit mixer.
#[inline]
fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

const MINHASH_K: usize = 8;

/// A word-shingle MinHash signature. Two pages with high Jaccard similarity of
/// word sets agree on many of the `K` minima, so lexicographic signature order
/// places similar pages adjacently (LSH).
fn minhash_signature(page: &[u8]) -> [u64; MINHASH_K] {
    let mut sig = [u64::MAX; MINHASH_K];
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut have_word = false;
    let flush = |h: u64, sig: &mut [u64; MINHASH_K]| {
        for (k, s) in sig.iter_mut().enumerate() {
            let v = splitmix64(h ^ (k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            if v < *s {
                *s = v;
            }
        }
    };
    for &b in page {
        let lc = b.to_ascii_lowercase();
        if lc.is_ascii_alphabetic() {
            h = (h ^ lc as u64).wrapping_mul(0x100_0000_01b3);
            have_word = true;
        } else if have_word {
            flush(h, &mut sig);
            have_word = false;
            h = 0xcbf2_9ce4_8422_2325;
        }
    }
    if have_word {
        flush(h, &mut sig);
    }
    sig
}

#[inline]
fn sig_distance(a: &[u64; MINHASH_K], b: &[u64; MINHASH_K]) -> u64 {
    let mut d = 0u64;
    for k in 0..MINHASH_K {
        d += (a[k] ^ b[k]).count_ones() as u64;
    }
    d
}

/// MinHash over the *boilerplate* a page uses: the names in `{{template}}`,
/// `[[Category:...]]`, `[[File:...]]` and `[[Image:...]]`. Pages that share
/// markup machinery get similar signatures, so sorting by the signature places
/// them together.
fn boilerplate_signature(page: &[u8]) -> [u64; MINHASH_K] {
    let mut sig = [u64::MAX; MINHASH_K];
    let flush = |name: &[u8], sig: &mut [u64; MINHASH_K]| {
        if name.is_empty() {
            return;
        }
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in name {
            h = (h ^ b.to_ascii_lowercase() as u64).wrapping_mul(0x100_0000_01b3);
        }
        for (k, s) in sig.iter_mut().enumerate() {
            let v = splitmix64(h ^ (k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            if v < *s {
                *s = v;
            }
        }
    };
    let n = page.len();
    let mut i = 0usize;
    while i + 1 < n {
        if page[i] == b'{' && page[i + 1] == b'{' {
            let mut j = i + 2;
            while j < n && page[j] != b'|' && page[j] != b'}' && page[j] != b'\n' {
                j += 1;
            }
            flush(&page[i + 2..j], &mut sig);
            i = j;
        } else if page[i] == b'['
            && page[i + 1] == b'['
            && (starts_with_ci(&page[i + 2..], b"category:")
                || starts_with_ci(&page[i + 2..], b"file:")
                || starts_with_ci(&page[i + 2..], b"image:"))
        {
            let mut j = i + 2;
            while j < n && page[j] != b'|' && page[j] != b']' && page[j] != b'\n' {
                j += 1;
            }
            flush(&page[i + 2..j], &mut sig);
            i = j;
        } else {
            i += 1;
        }
    }
    sig
}

fn starts_with_ci(hay: &[u8], needle: &[u8]) -> bool {
    hay.len() >= needle.len()
        && hay[..needle.len()]
            .iter()
            .zip(needle)
            .all(|(a, b)| a.to_ascii_lowercase() == *b)
}

/// The first `[[Category:Name...]]` in a page, lowercased, or empty.
fn first_category(page: &[u8]) -> Vec<u8> {
    let n = page.len();
    let mut i = 0usize;
    while i + 1 < n {
        if page[i] == b'[' && page[i + 1] == b'[' && starts_with_ci(&page[i + 2..], b"category:") {
            let mut j = i + 11;
            while j < n && page[j] != b'|' && page[j] != b']' && page[j] != b'\n' {
                j += 1;
            }
            return page[i + 11..j]
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
        }
        i += 1;
    }
    Vec::new()
}

/// Every `[[Category:Name...]]` in a page, lowercased, sorted, joined with NUL.
fn all_categories(page: &[u8]) -> Vec<u8> {
    let n = page.len();
    let mut cats: Vec<Vec<u8>> = Vec::new();
    let mut i = 0usize;
    while i + 1 < n {
        if page[i] == b'[' && page[i + 1] == b'[' && starts_with_ci(&page[i + 2..], b"category:") {
            let mut j = i + 11;
            while j < n && page[j] != b'|' && page[j] != b']' && page[j] != b'\n' {
                j += 1;
            }
            cats.push(
                page[i + 11..j]
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect(),
            );
            i = j;
        } else {
            i += 1;
        }
    }
    cats.sort();
    cats.dedup();
    let mut out = Vec::new();
    for (k, c) in cats.iter().enumerate() {
        if k > 0 {
            out.push(0);
        }
        out.extend_from_slice(c);
    }
    out
}

/// Every `{{Name...}}` in a page, lowercased, sorted, joined with NUL.
fn all_templates(page: &[u8]) -> Vec<u8> {
    let n = page.len();
    let mut ts: Vec<Vec<u8>> = Vec::new();
    let mut i = 0usize;
    while i + 1 < n {
        if page[i] == b'{' && page[i + 1] == b'{' {
            let mut j = i + 2;
            while j < n && page[j] != b'|' && page[j] != b'}' && page[j] != b'\n' {
                j += 1;
            }
            ts.push(
                page[i + 2..j]
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect(),
            );
            i = j;
        } else {
            i += 1;
        }
    }
    ts.sort();
    ts.dedup();
    let mut out = Vec::new();
    for (k, t) in ts.iter().enumerate() {
        if k > 0 {
            out.push(0);
        }
        out.extend_from_slice(t);
    }
    out
}

/// A reusable scratch table for the residual proxy: fixed-size, generation-
/// stamped so it never has to be cleared between pages.
struct NoveltyScratch {
    seen: Vec<u32>,
    gen: u32,
    mask: usize,
}

impl NoveltyScratch {
    fn new(bits: u32) -> Self {
        let n = 1usize << bits;
        NoveltyScratch {
            seen: vec![0u32; n],
            gen: 0,
            mask: n - 1,
        }
    }

    /// Count first-time `(order-2 context, byte)` events in `page`. Higher means
    /// the page carries more structure a local model has not seen.
    fn novelty(&mut self, page: &[u8]) -> u64 {
        self.gen = self.gen.wrapping_add(1);
        if self.gen == 0 {
            self.seen.iter_mut().for_each(|x| *x = 0);
            self.gen = 1;
        }
        let mut ctx: u32 = 0;
        let mut n = 0u64;
        for &b in page {
            let idx = (ctx.wrapping_mul(0x9E37_79B1) ^ b as u32) as usize & self.mask;
            if self.seen[idx] != self.gen {
                self.seen[idx] = self.gen;
                n += 1;
            }
            ctx = (ctx << 8) | b as u32;
        }
        n
    }
}

/// The first `{{Name...}}` in a page, lowercased, or empty.
fn first_template(page: &[u8]) -> Vec<u8> {
    let n = page.len();
    let mut i = 0usize;
    while i + 1 < n {
        if page[i] == b'{' && page[i + 1] == b'{' {
            let mut j = i + 2;
            while j < n && page[j] != b'|' && page[j] != b'}' && page[j] != b'\n' {
                j += 1;
            }
            return page[i + 2..j]
                .iter()
                .map(|b| b.to_ascii_lowercase())
                .collect();
        }
        i += 1;
    }
    Vec::new()
}

/// A deterministic page-index shuffle for the negative control.
fn shuffled_index(n: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..n).collect();
    let mut state = 0x5EED_1234_ABCD_9876u64;
    for i in (1..n).rev() {
        state = splitmix64(state);
        let j = (state % (i as u64 + 1)) as usize;
        idx.swap(i, j);
    }
    idx
}

/// Precondition for free restoration: every page has an id and the ids are
/// strictly ascending in the original order.
fn ascending_ids(pages: &[&[u8]]) -> Option<Vec<u64>> {
    let mut ids = Vec::with_capacity(pages.len());
    for p in pages {
        ids.push(page_id(p)?);
    }
    if ids.windows(2).any(|w| w[0] >= w[1]) {
        return None;
    }
    Some(ids)
}

/// Reassemble `header ++ pages[idx] ++ tail`.
fn assemble(
    input_len: usize,
    header: &[u8],
    pages: &[&[u8]],
    idx: &[usize],
    tail: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(input_len);
    out.extend_from_slice(header);
    for &i in idx {
        out.extend_from_slice(pages[i]);
    }
    out.extend_from_slice(tail);
    out
}

/// Reorder `input` under `order`. Returns `None` when the corpus does not
/// satisfy the free-restoration precondition, so the caller can fall back to the
/// parent method (and the decoder will never sort).
pub fn encode(input: &[u8], order: Order) -> Option<Vec<u8>> {
    let s = split(input)?;
    if s.pages.is_empty() {
        return None;
    }
    let ids = ascending_ids(&s.pages)?;
    let n = s.pages.len();
    let mut idx: Vec<usize> = (0..n).collect();

    match order {
        Order::Identity => {}
        Order::Title => {
            let mut key: Vec<(&[u8], usize)> = s
                .pages
                .iter()
                .enumerate()
                .map(|(i, p)| (page_title(p), i))
                .collect();
            key.sort_by(|a, b| a.0.cmp(b.0));
            idx = key.into_iter().map(|(_, i)| i).collect();
        }
        Order::Size => {
            let mut key: Vec<(usize, u64, usize)> = s
                .pages
                .iter()
                .enumerate()
                .map(|(i, p)| (p.len(), ids[i], i))
                .collect();
            key.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
            idx = key.into_iter().map(|(_, _, i)| i).collect();
        }
        Order::Struct => {
            let mut key: Vec<(u8, &[u8], usize)> = s
                .pages
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let t = page_title(p);
                    (namespace_rank(t), t, i)
                })
                .collect();
            key.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(b.1)));
            idx = key.into_iter().map(|(_, _, i)| i).collect();
        }
        Order::MinHash => {
            let mut key: Vec<([u64; MINHASH_K], usize)> = s
                .pages
                .iter()
                .enumerate()
                .map(|(i, p)| (minhash_signature(p), i))
                .collect();
            key.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
            idx = key.into_iter().map(|(_, i)| i).collect();
        }
        Order::Greedy => {
            // Seed with signature order, then a bounded greedy walk: maintain a
            // window of upcoming candidates and always take the closest, pulling
            // the next sorted page into the window as one leaves.
            let sigs: Vec<[u64; MINHASH_K]> =
                s.pages.iter().map(|p| minhash_signature(p)).collect();
            let mut sorted: Vec<usize> = (0..n).collect();
            sorted.sort_by(|&a, &b| sigs[a].cmp(&sigs[b]).then(a.cmp(&b)));
            const W: usize = 64;
            let mut window: VecDeque<usize> = VecDeque::with_capacity(W + 1);
            let mut ptr = 0usize;
            while ptr < n && window.len() < W {
                window.push_back(sorted[ptr]);
                ptr += 1;
            }
            let mut order: Vec<usize> = Vec::with_capacity(n);
            let mut cur = window.pop_front().unwrap();
            order.push(cur);
            while !window.is_empty() {
                let mut best_pos = 0usize;
                let mut best_d = u64::MAX;
                for (k, &j) in window.iter().enumerate() {
                    let d = sig_distance(&sigs[cur], &sigs[j]);
                    if d < best_d {
                        best_d = d;
                        best_pos = k;
                    }
                }
                cur = window.remove(best_pos).unwrap();
                order.push(cur);
                if ptr < n {
                    window.push_back(sorted[ptr]);
                    ptr += 1;
                }
            }
            idx = order;
        }
        Order::Template => {
            let mut key: Vec<([u64; MINHASH_K], usize)> = s
                .pages
                .iter()
                .enumerate()
                .map(|(i, p)| (boilerplate_signature(p), i))
                .collect();
            key.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
            idx = key.into_iter().map(|(_, i)| i).collect();
        }
        Order::Category => {
            let mut key: Vec<(Vec<u8>, &[u8], usize)> = s
                .pages
                .iter()
                .enumerate()
                .map(|(i, p)| (first_category(p), page_title(p), i))
                .collect();
            key.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(b.1)).then(a.2.cmp(&b.2)));
            idx = key.into_iter().map(|(_, _, i)| i).collect();
        }
        Order::CategorySet => {
            let mut key: Vec<(Vec<u8>, &[u8], usize)> = s
                .pages
                .iter()
                .enumerate()
                .map(|(i, p)| (all_categories(p), page_title(p), i))
                .collect();
            key.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(b.1)).then(a.2.cmp(&b.2)));
            idx = key.into_iter().map(|(_, _, i)| i).collect();
        }
        Order::Full => {
            let mut key: Vec<(Vec<u8>, Vec<u8>, &[u8], usize)> = s
                .pages
                .iter()
                .enumerate()
                .map(|(i, p)| (all_categories(p), all_templates(p), page_title(p), i))
                .collect();
            key.sort_by(|a, b| {
                a.0.cmp(&b.0)
                    .then(a.1.cmp(&b.1))
                    .then(a.2.cmp(b.2))
                    .then(a.3.cmp(&b.3))
            });
            idx = key.into_iter().map(|(_, _, _, i)| i).collect();
        }
        Order::FullResidual => {
            let mut scratch = NoveltyScratch::new(18);
            let novelty: Vec<u64> = s.pages.iter().map(|p| scratch.novelty(p)).collect();
            let mut key: Vec<(Vec<u8>, Vec<u8>, u64, &[u8], usize)> = s
                .pages
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    (
                        all_categories(p),
                        all_templates(p),
                        novelty[i],
                        page_title(p),
                        i,
                    )
                })
                .collect();
            key.sort_by(|a, b| {
                a.0.cmp(&b.0)
                    .then(a.1.cmp(&b.1))
                    .then(a.2.cmp(&b.2))
                    .then(a.3.cmp(b.3))
                    .then(a.4.cmp(&b.4))
            });
            idx = key.into_iter().map(|(_, _, _, _, i)| i).collect();
        }
        Order::TemplateKey => {
            let mut key: Vec<(Vec<u8>, &[u8], usize)> = s
                .pages
                .iter()
                .enumerate()
                .map(|(i, p)| (first_template(p), page_title(p), i))
                .collect();
            key.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(b.1)).then(a.2.cmp(&b.2)));
            idx = key.into_iter().map(|(_, _, i)| i).collect();
        }
        Order::Shuffle => {
            idx = shuffled_index(n);
        }
    }

    let out = assemble(input.len(), s.header, &s.pages, &idx, s.tail);
    debug_assert_eq!(out.len(), input.len());
    Some(out)
}

/// Restore the original order of a (reordered) page stream by sorting the page
/// blocks on their embedded id. A stream the splitter does not recognise is
/// returned unchanged.
pub fn restore(input: &[u8]) -> Vec<u8> {
    match split(input) {
        None => input.to_vec(),
        Some(s) => {
            let mut idx: Vec<usize> = (0..s.pages.len()).collect();
            idx.sort_by_key(|&i| page_id(s.pages[i]).unwrap_or(u64::MAX));
            assemble(input.len(), s.header, &s.pages, &idx, s.tail)
        }
    }
}

/// Number of pages and whether the free-restoration precondition holds. Research
/// helper for the probe and receipts.
pub fn info(input: &[u8]) -> (usize, bool) {
    match split(input) {
        None => (0, false),
        Some(s) => (s.pages.len(), ascending_ids(&s.pages).is_some()),
    }
}

/// Cost of an explicit permutation of the pages, in bytes, to quantify what the
/// free id-sort saves. Research helper (7.7).
pub fn explicit_permutation_bytes(n: usize) -> u64 {
    if n < 2 {
        return 0;
    }
    let mut bits = 0.0f64;
    for i in 2..=n {
        bits += (i as f64).log2();
    }
    (bits / 8.0).ceil() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corpus() -> Vec<u8> {
        let mut v = b"<mediawiki>\n".to_vec();
        for (id, title, body) in [
            (1u64, "Beta", "beta body words"),
            (2, "Alpha", "alpha body words"),
            (10, "Gamma", "gamma body words"),
        ] {
            v.extend_from_slice(
                format!(
                    "  <page>\n    <title>{title}</title>\n    <id>{id}</id>\n    <revision>\n      <text>{body}</text>\n    </revision>\n  </page>\n"
                )
                .as_bytes(),
            );
        }
        v.extend_from_slice(b"</mediawiki>\n");
        v
    }

    #[test]
    fn split_reassembles_exactly() {
        let c = corpus();
        let s = split(&c).unwrap();
        assert_eq!(s.pages.len(), 3);
        let mut back = s.header.to_vec();
        for p in &s.pages {
            back.extend_from_slice(p);
        }
        back.extend_from_slice(s.tail);
        assert_eq!(back, c);
    }

    #[test]
    fn page_ids_are_parsed() {
        let c = corpus();
        let s = split(&c).unwrap();
        let ids: Vec<u64> = s.pages.iter().map(|p| page_id(p).unwrap()).collect();
        assert_eq!(ids, vec![1, 2, 10]);
    }

    #[test]
    fn every_order_restores_exactly() {
        let c = corpus();
        for order in [
            Order::Identity,
            Order::Title,
            Order::Size,
            Order::Struct,
            Order::MinHash,
            Order::Greedy,
            Order::Template,
            Order::Category,
            Order::CategorySet,
            Order::Full,
            Order::FullResidual,
            Order::TemplateKey,
            Order::Shuffle,
        ] {
            let r = encode(&c, order).unwrap();
            assert_eq!(r.len(), c.len());
            assert_eq!(restore(&r), c, "order {order:?} did not restore");
        }
    }

    #[test]
    fn title_order_actually_reorders() {
        let c = corpus();
        let r = encode(&c, Order::Title).unwrap();
        let s2 = split(&r).unwrap();
        let ids2: Vec<u64> = s2.pages.iter().map(|p| page_id(p).unwrap()).collect();
        // Titles Alpha, Beta, Gamma -> ids 2, 1, 10.
        assert_eq!(ids2, vec![2, 1, 10]);
    }

    #[test]
    fn unsorted_ids_are_rejected() {
        let mut c = b"<m>\n".to_vec();
        for (id, t) in [(9u64, "A"), (3, "B")] {
            c.extend_from_slice(
                format!("  <page>\n    <title>{t}</title>\n    <id>{id}</id>\n  </page>\n")
                    .as_bytes(),
            );
        }
        assert!(encode(&c, Order::Title).is_none());
    }

    #[test]
    fn non_page_input_is_rejected() {
        assert!(encode(b"ordinary text with no pages", Order::Title).is_none());
        assert_eq!(restore(b"ordinary text"), b"ordinary text");
    }

    #[test]
    fn truncated_tail_is_preserved_and_sorted_last() {
        // A real truncation cuts mid-page with no trailer after the last close.
        let c0 = corpus();
        let body = &c0[..c0.len() - b"</mediawiki>\n".len()];
        let mut c = body.to_vec();
        c.extend_from_slice(b"  <page>\n    <title>Delta</title>\n    <id>99</id>\n");
        let r = encode(&c, Order::Title).unwrap();
        assert_eq!(r.len(), c.len());
        assert_eq!(restore(&r), c);
    }
}
