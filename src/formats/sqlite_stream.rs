//! Streaming SQLite B-tree reader for zstd-compressed databases.
//!
//! Reads the SQLite binary format page-by-page from a zstd stream and
//! converts all rows from the VRAIX transport table into a commonmeta
//! SQLite database, without decompressing the source to disk.
//!
//! Pages that arrive before they are known to be relevant (out-of-order
//! B-tree children or overflow pages) are buffered in RAM up to
//! [`RAM_LIMIT`], then spilled to a temp file up to [`DISK_LIMIT`].
//! On a VACUUM'd database the buffer stays empty.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};

use indicatif::ProgressBar;

use crate::error::{Error, Result};
use crate::formats::commonmeta::{init_sqlite_writer, write_sqlite_batch_rows};
use crate::formats::vraix::parallel_convert_and_prepare_mixed;

// ── Limits ───────────────────────────────────────────────────────────────────

/// Maximum page-data held in RAM for out-of-order pages (first pass only).
const RAM_LIMIT: usize = 16 * 1024 * 1024 * 1024; // 16 GiB
/// Maximum spill to the temp page-buffer file (first pass only).
const DISK_LIMIT: u64 = 250 * 1024 * 1024 * 1024; // 250 GiB
/// Batch size for parallel JSON conversion and SQLite writes.
const BATCH: usize = 50_000;
/// Maximum additional scan passes over the zstd file after the first pass.
const MAX_EXTRA_PASSES: usize = 20;
/// Stop extra passes when fewer than this many pages were matched in the last pass.
const CONVERGENCE_THRESHOLD: usize = 10;
/// Fallback sliding-window RAM size (in GiB) when RAM cannot be auto-detected.
/// Auto-detection maps: ≥128 GiB→96, ≥64→32, ≥32→16, ≥16→8.
/// Override at runtime with COMMONMETA_SCAN_WINDOW_GIB.
const SCAN_WINDOW_DEFAULT_GIB: usize = 8;
/// Default disk-backed extension for the sliding window (in GiB).
/// Override with COMMONMETA_SCAN_DISK_GIB.
const SCAN_DISK_DEFAULT_GIB: u64 = 500;

// ── SQLite constants ─────────────────────────────────────────────────────────

const MAGIC: &[u8; 16] = b"SQLite format 3\x00";
const PAGE_INTERIOR_TABLE: u8 = 0x05;
const PAGE_LEAF_TABLE: u8 = 0x0d;

// ── Counting reader ───────────────────────────────────────────────────────────

struct ProgressReader<R> {
    inner: R,
    bar: ProgressBar,
    bytes: u64,
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.bytes += n as u64;
        self.bar.set_position(self.bytes);
        Ok(n)
    }
}

// ── Temp-file guard ───────────────────────────────────────────────────────────

struct TmpGuard(PathBuf);

impl TmpGuard {
    fn defuse(self) { std::mem::forget(self); }
}

impl Drop for TmpGuard {
    fn drop(&mut self) { std::fs::remove_file(&self.0).ok(); }
}

// ── File header ───────────────────────────────────────────────────────────────

pub(crate) struct FileHdr {
    pub page_size: usize,
    reserved: usize,
    /// Page count from the SQLite header (bytes 28–31).  May be 0 if the
    /// header was never written (e.g. WAL mode with a live writer).
    pub db_page_count: u32,
}

impl FileHdr {
    fn usable(&self) -> usize { self.page_size - self.reserved }
    fn max_local(&self) -> usize { (self.usable().saturating_sub(12)) * 64 / 255 - 23 }
    fn min_local(&self) -> usize { (self.usable().saturating_sub(12)) * 32 / 255 - 23 }

    pub fn local_payload(&self, total: usize) -> usize {
        let max = self.max_local();
        if total <= max { return total; }
        let min = self.min_local();
        let surplus = min + (total - min) % self.usable().saturating_sub(4);
        if surplus <= max { surplus } else { min }
    }
}

pub(crate) fn parse_file_hdr(raw: &[u8; 100]) -> Option<FileHdr> {
    if &raw[0..16] != MAGIC { return None; }
    let raw_ps = u16::from_be_bytes([raw[16], raw[17]]) as usize;
    let page_size = if raw_ps == 1 { 65536 } else { raw_ps };
    let db_page_count = u32::from_be_bytes([raw[28], raw[29], raw[30], raw[31]]);
    Some(FileHdr { page_size, reserved: raw[20] as usize, db_page_count })
}

// ── Varint ────────────────────────────────────────────────────────────────────

fn read_varint(data: &[u8]) -> (u64, usize) {
    let mut val: u64 = 0;
    for (i, &b) in data.iter().enumerate().take(9) {
        if i < 8 {
            val = (val << 7) | (b & 0x7f) as u64;
            if b & 0x80 == 0 { return (val, i + 1); }
        } else {
            val = (val << 8) | b as u64;
            return (val, 9);
        }
    }
    (val, data.len().min(9))
}

// ── Serial types ──────────────────────────────────────────────────────────────

fn serial_size(st: u64) -> usize {
    match st {
        0 | 8 | 9 => 0,
        1 => 1, 2 => 2, 3 => 3, 4 => 4, 5 => 6, 6 | 7 => 8,
        n if n >= 12 && n % 2 == 0 => ((n - 12) / 2) as usize,
        n if n >= 13              => ((n - 13) / 2) as usize,
        _ => 0,
    }
}

fn serial_int(data: &[u8], st: u64) -> i64 {
    match st {
        0 => 0, 8 => 0, 9 => 1,
        1 => data[0] as i8 as i64,
        2 => i16::from_be_bytes([data[0], data[1]]) as i64,
        3 => {
            let v = (data[0] as i32) << 16 | (data[1] as i32) << 8 | data[2] as i32;
            if v & 0x80_0000 != 0 { v as i64 | (-1i64 << 24) } else { v as i64 }
        }
        4 => i32::from_be_bytes(data[..4].try_into().unwrap()) as i64,
        5 => {
            let mut b = [0u8; 8]; b[2..8].copy_from_slice(&data[..6]);
            let v = i64::from_be_bytes(b);
            if data[0] & 0x80 != 0 { v | (-1i64 << 48) } else { v }
        }
        6 => i64::from_be_bytes(data[..8].try_into().unwrap()),
        _ => 0,
    }
}

// ── Transport table schema ────────────────────────────────────────────────────

pub(crate) struct TableInfo {
    pub root_page: u32,
    pub source_id_col: usize,
    pub raw_metadata_col: usize,
    pub n_cols: usize,
}

fn payload_values(payload: &[u8], n_cols: usize) -> Vec<Option<String>> {
    if payload.is_empty() { return Vec::new(); }
    let (hdr_sz, mut hp) = read_varint(payload);
    let hdr_sz = hdr_sz as usize;
    let mut sts = Vec::with_capacity(n_cols);
    while hp < hdr_sz.min(payload.len()) && sts.len() < n_cols {
        let (st, n) = read_varint(&payload[hp..]); hp += n; sts.push(st);
    }
    let mut dp = hdr_sz;
    let mut out = Vec::with_capacity(sts.len());
    for st in sts {
        let sz = serial_size(st);
        if dp + sz > payload.len() { out.push(None); break; }
        let val = match st {
            0        => None,
            8        => Some("0".to_owned()),
            9        => Some("1".to_owned()),
            1..=6    => Some(serial_int(&payload[dp..], st).to_string()),
            7        => Some(f64::from_be_bytes(payload[dp..dp+8].try_into().unwrap()).to_string()),
            n if n >= 13 && n % 2 == 1 =>
                std::str::from_utf8(&payload[dp..dp+sz]).ok().map(|s| s.to_owned()),
            _        => None,
        };
        out.push(val); dp += sz;
    }
    out
}

fn ddl_column_names(sql: &str) -> Vec<String> {
    let s = sql.find('(').map(|i| &sql[i+1..]).unwrap_or(sql);
    let s = s.rfind(')').map(|i| &s[..i]).unwrap_or(s);
    const CONSTRAINT_KW: &[&str] = &["primary","unique","check","foreign","constraint"];
    s.split(',')
        .map(|col| col.trim().split_whitespace().next().unwrap_or("").to_ascii_lowercase())
        .filter(|n| !n.is_empty() && !CONSTRAINT_KW.contains(&n.as_str()))
        .collect()
}

pub(crate) fn find_table_info(page: &[u8], _fhdr: &FileHdr) -> Option<TableInfo> {
    let bth = 100usize;
    if page.len() <= bth + 8 || page[bth] != PAGE_LEAF_TABLE { return None; }
    let ncells = u16::from_be_bytes([page[bth+3], page[bth+4]]) as usize;
    for i in 0..ncells {
        let ptr_off = bth + 8 + i * 2;
        if ptr_off + 2 > page.len() { break; }
        let cell_at = u16::from_be_bytes([page[ptr_off], page[ptr_off+1]]) as usize;
        if cell_at >= page.len() { break; }
        let data = &page[cell_at..];
        let mut p = 0;
        let (total, n) = read_varint(&data[p..]); p += n;
        let (_rowid, n) = read_varint(&data[p..]); p += n;
        let total = total as usize;
        if p + total > data.len() { continue; }
        let vals = payload_values(&data[p..p+total], 5);
        if vals.len() < 5 { continue; }
        if !vals[0].as_deref().unwrap_or("").eq_ignore_ascii_case("table") { continue; }
        let sql = vals[4].as_deref().unwrap_or("");
        let cols = ddl_column_names(sql);
        if !cols.iter().any(|c| c == "pid") { continue; }
        let source_id_col = cols.iter().position(|c| c == "source_id")?;
        let raw_metadata_col = cols.iter().position(|c| c == "raw_metadata")?;
        let root_page = vals[3].as_deref().unwrap_or("0").parse::<u32>().ok()?;
        return Some(TableInfo { root_page, source_id_col, raw_metadata_col, n_cols: cols.len() });
    }
    None
}

// ── Interior page ─────────────────────────────────────────────────────────────

pub(crate) fn interior_children(page: &[u8], btree_off: usize) -> Vec<u32> {
    let pg = match page.get(btree_off..) {
        Some(s) if !s.is_empty() && s[0] == PAGE_INTERIOR_TABLE => s,
        _ => return Vec::new(),
    };
    let ncells = u16::from_be_bytes([pg[3], pg[4]]) as usize;
    let right = u32::from_be_bytes([pg[8], pg[9], pg[10], pg[11]]);
    let mut children = Vec::with_capacity(ncells + 1);
    for i in 0..ncells {
        let off = 12 + i * 2;
        if off + 2 > pg.len() { break; }
        let cell = u16::from_be_bytes([pg[off], pg[off+1]]) as usize;
        if cell + 4 > page.len() { break; }
        children.push(u32::from_be_bytes([page[cell], page[cell+1], page[cell+2], page[cell+3]]));
    }
    children.push(right);
    children
}

// ── Leaf page cells ───────────────────────────────────────────────────────────

pub(crate) struct LeafCell {
    pub source_id: i64,
    pub raw_inline: Vec<u8>,
    pub overflow: Option<(u32, usize)>,
}

pub(crate) fn leaf_cells(page: &[u8], btree_off: usize, fhdr: &FileHdr, tbl: &TableInfo) -> Vec<LeafCell> {
    let pg = match page.get(btree_off..) {
        Some(s) if !s.is_empty() && s[0] == PAGE_LEAF_TABLE => s,
        _ => return Vec::new(),
    };
    let ncells = u16::from_be_bytes([pg[3], pg[4]]) as usize;
    let mut out = Vec::with_capacity(ncells);
    for i in 0..ncells {
        let ptr_off = 8 + i * 2;
        if ptr_off + 2 > pg.len() { break; }
        let cell_at = u16::from_be_bytes([pg[ptr_off], pg[ptr_off+1]]) as usize;
        if cell_at >= page.len() { break; }
        if let Some(c) = parse_cell(&page[cell_at..], fhdr, tbl) { out.push(c); }
    }
    out
}

fn parse_cell(data: &[u8], fhdr: &FileHdr, tbl: &TableInfo) -> Option<LeafCell> {
    let mut p = 0;
    let (total_payload, n) = read_varint(&data[p..]); p += n;
    let total_payload = total_payload as usize;
    let (_, n) = read_varint(&data[p..]); p += n;

    let local = fhdr.local_payload(total_payload);
    let has_overflow = total_payload > fhdr.max_local();
    let overflow_page_num = if has_overflow {
        let op_at = p + local;
        if op_at + 4 > data.len() { return None; }
        Some(u32::from_be_bytes([data[op_at], data[op_at+1], data[op_at+2], data[op_at+3]]))
    } else { None };

    let payload = data.get(p..p + local)?;
    let (hdr_sz, mut hp) = read_varint(payload);
    let hdr_sz = hdr_sz as usize;
    let mut sts: Vec<u64> = Vec::with_capacity(tbl.n_cols);
    while hp < hdr_sz.min(payload.len()) && sts.len() < tbl.n_cols {
        let (st, n) = read_varint(&payload[hp..]); hp += n; sts.push(st);
    }

    let mut dp = hdr_sz;
    let mut source_id = 0i64;
    let mut raw_start = dp;
    let mut raw_total = 0usize;
    for (col, &st) in sts.iter().enumerate() {
        let sz = serial_size(st);
        if col == tbl.source_id_col && dp + sz <= payload.len() {
            source_id = serial_int(&payload[dp..], st);
        }
        if col == tbl.raw_metadata_col {
            raw_start = dp;
            raw_total = if st >= 13 && st % 2 == 1 { ((st - 13) / 2) as usize } else { 0 };
            break;
        }
        dp += sz;
    }

    let raw_inline = payload.get(raw_start..).unwrap_or(&[]).to_vec();
    let overflow = if has_overflow {
        let remaining = raw_total.saturating_sub(raw_inline.len());
        if remaining > 0 { Some((overflow_page_num?, remaining)) } else { None }
    } else { None };

    Some(LeafCell { source_id, raw_inline, overflow })
}

// ── Pending overflow record ───────────────────────────────────────────────────

struct Pending {
    source_id: i64,
    raw: Vec<u8>,
    remaining: usize,
}

// ── Out-of-order page buffer ──────────────────────────────────────────────────

/// Two-tier buffer for pages that arrive before they are recognised as
/// belonging to the transport table or an overflow chain. RAM up to
/// [`RAM_LIMIT`], temp-file spill up to [`DISK_LIMIT`]. Pages over budget
/// are silently dropped (they would only be out-of-order in a non-VACUUM'd
/// database, and the user receives a warning at the end).
struct PageBuffer {
    ram: HashMap<u32, Vec<u8>>,
    ram_bytes: usize,
    disk: Option<File>,
    disk_path: PathBuf,
    disk_index: HashMap<u32, u64>, // page_num → byte offset in disk file
    disk_bytes: u64,
    page_size: usize,
}

impl PageBuffer {
    fn new(page_size: usize, disk_path: PathBuf) -> Self {
        Self {
            ram: HashMap::new(), ram_bytes: 0,
            disk: None, disk_path,
            disk_index: HashMap::new(), disk_bytes: 0,
            page_size,
        }
    }

    fn store(&mut self, page_num: u32, data: &[u8]) -> Result<()> {
        if self.ram_bytes + data.len() <= RAM_LIMIT {
            self.ram_bytes += data.len();
            self.ram.insert(page_num, data.to_vec());
        } else if self.disk_bytes + data.len() as u64 <= DISK_LIMIT {
            if self.disk.is_none() {
                self.disk = Some(
                    File::options()
                        .read(true).write(true).create_new(true)
                        .open(&self.disk_path)
                        .map_err(|e| Error::Parse(format!(
                            "create page buffer {}: {e}", self.disk_path.display()
                        )))?
                );
            }
            let f = self.disk.as_mut().unwrap();
            f.write_all(data).map_err(|e| Error::Parse(format!("write page buffer: {e}")))?;
            self.disk_index.insert(page_num, self.disk_bytes);
            self.disk_bytes += data.len() as u64;
        }
        // Over both limits: silently drop. warn_if_leftover() can't see these,
        // but a low record count will make the situation obvious.
        Ok(())
    }

    fn remove(&mut self, page_num: &u32) -> Option<Vec<u8>> {
        if let Some(data) = self.ram.remove(page_num) {
            self.ram_bytes -= data.len();
            return Some(data);
        }
        if let Some(offset) = self.disk_index.remove(page_num) {
            let f = self.disk.as_mut()?;
            let mut data = vec![0u8; self.page_size];
            f.seek(io::SeekFrom::Start(offset)).ok()?;
            f.read_exact(&mut data).ok()?;
            return Some(data);
        }
        None
    }

    fn contains(&self, page_num: u32) -> bool {
        self.ram.contains_key(&page_num) || self.disk_index.contains_key(&page_num)
    }

    fn leftover_count(&self) -> usize {
        self.ram.len() + self.disk_index.len()
    }
}

impl Drop for PageBuffer {
    fn drop(&mut self) {
        drop(self.disk.take());
        std::fs::remove_file(&self.disk_path).ok();
    }
}

// ── Page-lookup abstraction ───────────────────────────────────────────────────

/// Returns total installed RAM in GiB (0 if detection fails).
fn total_ram_gib() -> usize {
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = std::fs::read_to_string("/proc/meminfo") {
            for line in content.lines() {
                if line.starts_with("MemTotal:") {
                    if let Some(kb) = line.split_whitespace().nth(1)
                        .and_then(|s| s.parse::<u64>().ok())
                    {
                        return (kb / (1024 * 1024)) as usize;
                    }
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
        {
            if let Ok(s) = std::str::from_utf8(&out.stdout) {
                if let Ok(bytes) = s.trim().parse::<u64>() {
                    return (bytes / (1024 * 1024 * 1024)) as usize;
                }
            }
        }
    }
    0
}

/// Choose a scan-window size based on detected RAM, leaving headroom for the OS.
/// Override at runtime with COMMONMETA_SCAN_WINDOW_GIB.
fn auto_window_gib() -> usize {
    match total_ram_gib() {
        n if n >= 128 => 96,
        n if n >= 64  => 32,
        n if n >= 32  => 16,
        n if n >= 16  => 8,
        _             => SCAN_WINDOW_DEFAULT_GIB,
    }
}

/// Minimal interface needed by [`handle_page`] to check whether a page is
/// already held in some buffer so it can be added to `known_buffered` for
/// immediate retroactive processing.
trait PageLookup {
    fn contains(&self, page_num: u32) -> bool;
}

impl PageLookup for PageBuffer {
    fn contains(&self, page_num: u32) -> bool { self.contains(page_num) }
}

/// Two-tier sliding-window (LRU) buffer used during extra scan passes.
///
/// Keeps the most-recently-seen `max_ram_pages` non-target pages in RAM.
/// When RAM is full, the oldest pages spill to a disk temp file up to
/// `max_disk_bytes`.  When disk is also full, the oldest disk entries are
/// evicted (data dropped but further lookups simply miss).
///
/// This lets backward overflow-chain links within the window be resolved
/// within the same scan pass instead of requiring another 300 GiB re-scan.
struct SlidingBuf {
    // RAM tier
    ram: HashMap<u32, Vec<u8>>,
    ram_order: BTreeSet<u32>,
    max_ram_pages: usize,
    // Disk tier
    disk: Option<File>,
    disk_path: PathBuf,
    disk_index: HashMap<u32, u64>, // page_num → byte offset
    disk_order: BTreeSet<u32>,
    disk_bytes: u64,
    max_disk_bytes: u64,
    page_size: usize,
}

impl SlidingBuf {
    fn new(max_ram_pages: usize, max_disk_bytes: u64, page_size: usize, disk_path: PathBuf) -> Self {
        Self {
            ram: HashMap::new(), ram_order: BTreeSet::new(), max_ram_pages,
            disk: None, disk_path,
            disk_index: HashMap::new(), disk_order: BTreeSet::new(),
            disk_bytes: 0, max_disk_bytes,
            page_size,
        }
    }

    fn store(&mut self, page_num: u32, data: Vec<u8>) {
        if self.max_ram_pages == 0 && self.max_disk_bytes == 0 { return; }

        // Make room in RAM (evict oldest to disk if possible, else drop).
        while self.ram.len() >= self.max_ram_pages.max(1) {
            let Some(&oldest) = self.ram_order.iter().next() else { break };
            self.ram_order.remove(&oldest);
            if let Some(evicted) = self.ram.remove(&oldest) {
                self.spill_to_disk(oldest, evicted);
            }
        }

        if self.max_ram_pages > 0 {
            self.ram.insert(page_num, data);
            self.ram_order.insert(page_num);
        } else {
            self.spill_to_disk(page_num, data);
        }
    }

    fn spill_to_disk(&mut self, page_num: u32, data: Vec<u8>) {
        if self.max_disk_bytes == 0 { return; }

        // Evict oldest disk entries until we have room.
        while self.disk_bytes + data.len() as u64 > self.max_disk_bytes {
            let Some(&oldest) = self.disk_order.iter().next() else { break };
            self.disk_order.remove(&oldest);
            if let Some(offset) = self.disk_index.remove(&oldest) {
                // Disk space is not reclaimed in-place; the file grows until drop.
                // Eviction just removes the index entry so lookups miss.
                let _ = offset;
            }
        }
        if self.disk_bytes + data.len() as u64 > self.max_disk_bytes { return; }

        // Lazy-open the disk file.
        if self.disk.is_none() {
            match File::options().read(true).write(true).create_new(true).open(&self.disk_path) {
                Ok(f) => self.disk = Some(f),
                Err(_) => return, // can't open temp file; just drop the page
            }
        }
        let f = self.disk.as_mut().unwrap();
        if f.write_all(&data).is_ok() {
            self.disk_index.insert(page_num, self.disk_bytes);
            self.disk_order.insert(page_num);
            self.disk_bytes += data.len() as u64;
        }
    }

    fn remove(&mut self, page_num: u32) -> Option<Vec<u8>> {
        if let Some(data) = self.ram.remove(&page_num) {
            self.ram_order.remove(&page_num);
            return Some(data);
        }
        if let Some(offset) = self.disk_index.remove(&page_num) {
            self.disk_order.remove(&page_num);
            let f = self.disk.as_mut()?;
            let mut data = vec![0u8; self.page_size];
            f.seek(io::SeekFrom::Start(offset)).ok()?;
            f.read_exact(&mut data).ok()?;
            return Some(data);
        }
        None
    }
}

impl PageLookup for SlidingBuf {
    fn contains(&self, page_num: u32) -> bool {
        self.ram.contains_key(&page_num) || self.disk_index.contains_key(&page_num)
    }
}

impl Drop for SlidingBuf {
    fn drop(&mut self) {
        drop(self.disk.take());
        std::fs::remove_file(&self.disk_path).ok();
    }
}

// ── Page dispatch ─────────────────────────────────────────────────────────────

/// Process one page that is known to be in `target` or `overflow_map`.
/// Newly discovered page references are checked against `page_buf`;
/// if found, they are added to `known_buffered` for retroactive processing.
fn handle_page<B: PageLookup>(
    page_num: u32,
    page: &[u8],
    fhdr: &FileHdr,
    tbl: &TableInfo,
    target: &mut HashSet<u32>,
    overflow_map: &mut HashMap<u32, Pending>,
    page_buf: &B,
    known_buffered: &mut HashSet<u32>,
    batch: &mut Vec<(i64, String)>,
) {
    if let Some(mut pend) = overflow_map.remove(&page_num) {
        let next_page = u32::from_be_bytes([page[0], page[1], page[2], page[3]]);
        let chunk = &page[4..];
        let take = pend.remaining.min(chunk.len());
        pend.raw.extend_from_slice(&chunk[..take]);
        pend.remaining -= take;

        if pend.remaining == 0 {
            if let Ok(s) = String::from_utf8(pend.raw) { batch.push((pend.source_id, s)); }
        } else if next_page != 0 {
            overflow_map.insert(next_page, pend);
            if page_buf.contains(next_page) { known_buffered.insert(next_page); }
        }

    } else if target.remove(&page_num) {
        let page_type = page[0];
        if page_type == PAGE_INTERIOR_TABLE {
            for child in interior_children(page, 0) {
                target.insert(child);
                if page_buf.contains(child) { known_buffered.insert(child); }
            }
        } else if page_type == PAGE_LEAF_TABLE {
            for cell in leaf_cells(page, 0, fhdr, tbl) {
                if cell.source_id == 3 { continue; } // ROR — skip
                if let Some((first_op, remaining)) = cell.overflow {
                    overflow_map.insert(first_op, Pending {
                        source_id: cell.source_id,
                        raw: cell.raw_inline,
                        remaining,
                    });
                    if page_buf.contains(first_op) { known_buffered.insert(first_op); }
                } else if let Ok(s) = String::from_utf8(cell.raw_inline) {
                    batch.push((cell.source_id, s));
                }
            }
        }
    }
}

// ── Extra scan pass (multi-pass recovery) ────────────────────────────────────

/// One additional sequential scan over the zstd file.
///
/// Pages currently in `target` or `overflow_map` are processed immediately.
/// All other pages are kept in a sliding RAM window (`SlidingBuf`) so that
/// backward overflow-chain references — where the next chain link has a lower
/// page number than its predecessor — can be resolved retroactively within the
/// same scan rather than requiring another full re-scan of the file.
///
/// Window size defaults to `SCAN_WINDOW_DEFAULT_GIB` GiB and is overridable
/// via the `COMMONMETA_SCAN_WINDOW_GIB` environment variable.
fn scan_pass(
    zst_path: &Path,
    fhdr: &FileHdr,
    tbl: &TableInfo,
    target: &mut HashSet<u32>,
    overflow_map: &mut HashMap<u32, Pending>,
    known_buffered: &mut HashSet<u32>,
    batch: &mut Vec<(i64, String)>,
    written: &mut usize,
    out_conn: &rusqlite::Connection,
    limit: usize,
) -> Result<usize> {
    let window_gib = std::env::var("COMMONMETA_SCAN_WINDOW_GIB")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or_else(auto_window_gib);
    let disk_gib = std::env::var("COMMONMETA_SCAN_DISK_GIB")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(SCAN_DISK_DEFAULT_GIB);
    let max_ram_pages = (window_gib * 1024 * 1024 * 1024) / fhdr.page_size.max(1);
    let max_disk_bytes = disk_gib * 1024 * 1024 * 1024;
    let slide_disk_path = {
        let mut s = zst_path.as_os_str().to_os_string();
        s.push(format!(".scanpass-{}.pagebuf", std::process::id()));
        PathBuf::from(s)
    };
    let mut slide = SlidingBuf::new(max_ram_pages, max_disk_bytes, fhdr.page_size, slide_disk_path);

    let src = File::open(zst_path)
        .map_err(|e| Error::Parse(format!("open {}: {e}", zst_path.display())))?;
    let mut dec = zstd::Decoder::new(src)
        .map_err(|e| Error::Parse(format!("zstd init (extra pass): {e}")))?;

    // Skip page 1 (already parsed for header and table info).
    let mut skip = vec![0u8; fhdr.page_size];
    dec.read_exact(&mut skip)
        .map_err(|e| Error::Parse(format!("skip page 1 (extra pass): {e}")))?;

    let mut page_raw = vec![0u8; fhdr.page_size];
    let mut page_num: u32 = 2;
    let mut matched = 0usize;

    'scan: loop {
        if limit > 0 && *written >= limit { break; }

        // Retroactively process any backward-chain pages now in the window.
        while let Some(&pnum) = known_buffered.iter().next() {
            known_buffered.remove(&pnum);
            if let Some(data) = slide.remove(pnum) {
                handle_page(pnum, &data, fhdr, tbl, target, overflow_map,
                            &slide, known_buffered, batch);
                flush_batch(batch, written, out_conn, limit)?;
                matched += 1;
                if limit > 0 && *written >= limit { break 'scan; }
            }
        }

        match dec.read_exact(&mut page_raw) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(Error::Parse(format!("read page {page_num} (extra pass): {e}"))),
        }

        if overflow_map.contains_key(&page_num) || target.contains(&page_num) {
            handle_page(page_num, &page_raw, fhdr, tbl, target, overflow_map,
                        &slide, known_buffered, batch);
            flush_batch(batch, written, out_conn, limit)?;
            matched += 1;
        } else {
            slide.store(page_num, page_raw.clone());
        }

        page_num += 1;
    }

    // Drain any remaining known_buffered pages after the stream ends.
    while let Some(&pnum) = known_buffered.iter().next() {
        known_buffered.remove(&pnum);
        if let Some(data) = slide.remove(pnum) {
            handle_page(pnum, &data, fhdr, tbl, target, overflow_map,
                        &slide, known_buffered, batch);
            flush_batch(batch, written, out_conn, limit)?;
            matched += 1;
        }
    }

    Ok(matched)
}

// ── Batch flush helper ────────────────────────────────────────────────────────

fn flush_batch(
    batch: &mut Vec<(i64, String)>,
    written: &mut usize,
    conn: &rusqlite::Connection,
    limit: usize,
) -> Result<()> {
    let ready = batch.len() >= BATCH || (limit > 0 && *written + batch.len() >= limit);
    if !ready { return Ok(()); }
    let to_write: Vec<_> = if limit > 0 {
        batch.drain(..batch.len().min(limit - *written)).collect()
    } else {
        batch.drain(..).collect()
    };
    let prepared = parallel_convert_and_prepare_mixed(&to_write);
    *written += prepared.len();
    write_sqlite_batch_rows(conn, prepared)
}

// ── Main entry point ──────────────────────────────────────────────────────────

/// Stream-decompress `zst_path` (a zstd-compressed SQLite database) and
/// convert all rows in the VRAIX transport table directly into a commonmeta
/// SQLite database at `output_path`, without writing the decompressed SQLite
/// to disk.
///
/// Out-of-order pages are buffered in RAM (up to 16 GiB) and then on disk
/// (up to 250 GiB), so the approach remains correct even if the source
/// database was not VACUUM'd.
///
/// Writes to a `.tmp` sibling of `output_path` and renames atomically on
/// success, so an interrupted run never corrupts an existing database.
pub fn stream_zst_pidbox_to_sqlite(
    zst_path: &Path,
    output_path: &Path,
    limit: usize,
    _overwrite: bool,
) -> Result<usize> {
    // ── Progress bar (compressed bytes) ──────────────────────────────────
    let zst_total = std::fs::metadata(zst_path).map(|m| m.len()).unwrap_or(0);
    let bar = crate::progress::bytes_bar("converting", zst_total);

    // ── Open decompressor ──────────────────────────────────────────────
    let src = File::open(zst_path)
        .map_err(|e| Error::Parse(format!("open {}: {e}", zst_path.display())))?;
    let counted = ProgressReader { inner: src, bar: bar.clone(), bytes: 0 };
    let mut dec = zstd::Decoder::new(counted)
        .map_err(|e| Error::Parse(format!("zstd init: {e}")))?;

    // ── Page 1 → file header + sqlite_master ──────────────────────────
    let mut hdr100 = [0u8; 100];
    dec.read_exact(&mut hdr100)
        .map_err(|e| Error::Parse(format!("read file header: {e}")))?;
    let fhdr = parse_file_hdr(&hdr100)
        .ok_or_else(|| Error::Parse("not a SQLite 3 database".to_owned()))?;

    let mut page1 = vec![0u8; fhdr.page_size];
    page1[..100].copy_from_slice(&hdr100);
    dec.read_exact(&mut page1[100..])
        .map_err(|e| Error::Parse(format!("read page 1: {e}")))?;

    let tbl = find_table_info(&page1, &fhdr).ok_or_else(|| {
        Error::Parse(
            "no VRAIX transport table found in sqlite_master (need pid/source_id/raw_metadata)".into()
        )
    })?;

    // ── Temp output file (renamed on success) ─────────────────────────
    let tmp_out = {
        let mut s = output_path.as_os_str().to_os_string();
        s.push(format!(".pidbox-{}.tmp", std::process::id()));
        PathBuf::from(s)
    };
    let out_guard = TmpGuard(tmp_out.clone());
    let out_conn = init_sqlite_writer(&tmp_out, true)?;

    // ── Temp page-buffer file ─────────────────────────────────────────
    let buf_path = {
        let mut s = output_path.as_os_str().to_os_string();
        s.push(format!(".pagebuf-{}.tmp", std::process::id()));
        PathBuf::from(s)
    };
    let mut page_buf = PageBuffer::new(fhdr.page_size, buf_path);

    // ── Traversal state ────────────────────────────────────────────────
    let mut target: HashSet<u32> = HashSet::new();
    target.insert(tbl.root_page);

    let mut overflow_map: HashMap<u32, Pending> = HashMap::new();

    // Pages that are in page_buf AND have been discovered as relevant:
    // processed retroactively before moving to the next streamed page.
    let mut known_buffered: HashSet<u32> = HashSet::new();

    let mut batch: Vec<(i64, String)> = Vec::with_capacity(BATCH);
    let mut written = 0usize;
    let mut page_num: u32 = 2;

    let mut page_raw = vec![0u8; fhdr.page_size];

    eprintln!(
        "  SQLite page_size={}, db_pages={}, table root={}",
        fhdr.page_size,
        if fhdr.db_page_count > 0 { fhdr.db_page_count.to_string() } else { "unknown".to_string() },
        tbl.root_page,
    );

    // ── Streaming loop (first pass) ────────────────────────────────────
    'stream: loop {
        if limit > 0 && written >= limit { break; }

        // Retroactively process any buffered pages now known to be relevant.
        while let Some(&pnum) = known_buffered.iter().next() {
            known_buffered.remove(&pnum);
            if let Some(data) = page_buf.remove(&pnum) {
                handle_page(pnum, &data, &fhdr, &tbl, &mut target, &mut overflow_map,
                            &page_buf, &mut known_buffered, &mut batch);
                flush_batch(&mut batch, &mut written, &out_conn, limit)?;
                if limit > 0 && written >= limit { break 'stream; }
            }
        }

        // Read next page from the compressed stream.
        match dec.read_exact(&mut page_raw) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break 'stream,
            Err(e) => return Err(Error::Parse(format!("read page {page_num}: {e}"))),
        }

        if overflow_map.contains_key(&page_num) || target.contains(&page_num) {
            handle_page(page_num, &page_raw, &fhdr, &tbl, &mut target, &mut overflow_map,
                        &page_buf, &mut known_buffered, &mut batch);
            flush_batch(&mut batch, &mut written, &out_conn, limit)?;
        } else {
            // Page not yet known to be relevant — buffer it.
            page_buf.store(page_num, &page_raw)?;
        }

        page_num += 1;
    }

    // ── Drain any remaining known_buffered pages ───────────────────────
    while let Some(&pnum) = known_buffered.iter().next() {
        known_buffered.remove(&pnum);
        if let Some(data) = page_buf.remove(&pnum) {
            handle_page(pnum, &data, &fhdr, &tbl, &mut target, &mut overflow_map,
                        &page_buf, &mut known_buffered, &mut batch);
        }
    }

    // ── Report first-pass diagnostics ──────────────────────────────────
    {
        let leftover_buf = page_buf.leftover_count();
        if leftover_buf > 0 {
            eprintln!(
                "  pass 1: {leftover_buf} buffered pages from other tables/indexes (expected)"
            );
        }
    }
    drop(page_buf); // free buffer memory before extra passes

    // ── Multi-pass recovery for missed B-tree pages ────────────────────
    // If target or overflow_map are non-empty, some pages were dropped from
    // the buffer (buffer too small for the full file).  Re-scan the zstd
    // file, this time only picking up the specific pages we still need.
    // Each pass can recover one more level of B-tree depth; repeat until
    // converged or MAX_EXTRA_PASSES reached.
    for pass in 1..=MAX_EXTRA_PASSES {
        let still_needed = target.len() + overflow_map.len();
        if still_needed == 0 || (limit > 0 && written >= limit) { break; }

        eprintln!(
            "  pass {}: {} pages still needed — rescanning {} GiB compressed file …",
            pass + 1,
            still_needed,
            zst_total / (1024 * 1024 * 1024).max(1),
        );

        let matched = scan_pass(
            zst_path, &fhdr, &tbl,
            &mut target, &mut overflow_map, &mut known_buffered,
            &mut batch, &mut written, &out_conn, limit,
        )?;

        if matched < CONVERGENCE_THRESHOLD {
            eprintln!(
                "  pass {}: {} pages matched — converged, stopping",
                pass + 1, matched
            );
            break;
        }
    }

    // ── Flush tail batch ───────────────────────────────────────────────
    if !batch.is_empty() && (limit == 0 || written < limit) {
        let tail: Vec<_> = if limit > 0 {
            batch.into_iter().take(limit - written).collect()
        } else {
            batch
        };
        let n = tail.len();
        let prepared = parallel_convert_and_prepare_mixed(&tail);
        let converted = prepared.len();
        written += converted;
        if converted < n {
            eprintln!("  tail: {n} records found, {converted} converted ({} failed)", n - converted);
        }
        write_sqlite_batch_rows(&out_conn, prepared)?;
    }

    // ── Final diagnostics ──────────────────────────────────────────────
    if !target.is_empty() {
        eprintln!(
            "  warning: {} B-tree pages still unresolved after {} passes — \
             some records may be missing",
            target.len(),
            MAX_EXTRA_PASSES + 1,
        );
    }
    if !overflow_map.is_empty() {
        eprintln!(
            "  warning: {} overflow chains unresolved — \
             {} records with large raw_metadata may be truncated",
            overflow_map.len(),
            overflow_map.len(),
        );
    }

    bar.finish_and_clear();

    // ── Record installation date ───────────────────────────────────────
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    out_conn.execute(
        "INSERT OR REPLACE INTO settings (key, value) VALUES ('vraix_date', ?1)",
        rusqlite::params![today],
    ).map_err(|e| Error::Parse(format!("store vraix_date: {e}")))?;

    // ── Atomic rename ─────────────────────────────────────────────────
    drop(out_conn); // release SQLite lock before rename
    std::fs::rename(&tmp_out, output_path).map_err(|e| Error::Parse(format!(
        "rename {} → {}: {e}", tmp_out.display(), output_path.display()
    )))?;
    out_guard.defuse();

    Ok(written)
}
