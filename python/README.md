# commonmeta-rs (Python bindings)

PyO3 bindings exposing commonmeta-rs's `list`-command batch functions to
Python: VRAIX daily-dump fetch with local caching, lossless Parquet
read/write, and batched archive rendering. Records cross the FFI boundary as
plain JSON-shaped dicts — the same shape `commonmeta-py`'s own readers and
writers already produce and consume.

## Development

```sh
uv venv .venv
uv pip install --python .venv/bin/python maturin pytest
source .venv/bin/activate
maturin develop
pytest tests/
```

## API

- `fetch_vraix(source, date, input_path=None, limit=None, offset=0, cache_ttl_days=30) -> list[dict]`
  Fetch records from a VRAIX daily dump (`source` is `"crossref"` or
  `"datacite"`). With `input_path`, reads a local SQLite file directly (no
  network); otherwise downloads `{source}-{date}.sqlite3.zst` from
  metadata.vraix.org, caching it locally for `cache_ttl_days`.
- `write_parquet(records: list[dict]) -> bytes`
  Write records as a single lossless Parquet file.
- `read_parquet(data: bytes) -> list[dict]`
  Read records back from Parquet bytes written by `write_parquet`.
- `write_archive(records: list[dict], to: str, base_name: str, batch_size=100_000) -> list[tuple[str, bytes]]`
  Render records to `to` format, split into batches of at most `batch_size`
  records, returned as `(entry_name, bytes)` pairs ready to pack into a
  zip/tar archive.
- `convert(from_: str, to: str, input: str) -> bytes`
  Convert a single record between formats.
