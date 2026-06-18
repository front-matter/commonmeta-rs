import sqlite3

import commonmeta_rs
import pytest


def test_convert_crossref_to_commonmeta():
    input_json = (
        '{"message": {"DOI": "10.1/a", "type": "journal-article", "title": ["A Title"]}}'
    )
    output = commonmeta_rs.convert("crossref", "commonmeta", input_json)
    assert b'"id":"https://doi.org/10.1/a"' in output


def test_write_read_parquet_roundtrip_is_lossless():
    records = [
        {
            "id": "https://doi.org/10.1/a",
            "type": "JournalArticle",
            "titles": [{"title": "A Title"}],
            "contributors": [
                {"givenName": "Jane", "familyName": "Doe"},
                {"givenName": "John", "familyName": "Smith"},
            ],
        }
    ]

    data = commonmeta_rs.write_parquet(records)
    assert isinstance(data, bytes)
    assert len(data) > 0

    roundtripped = commonmeta_rs.read_parquet(data)
    assert len(roundtripped) == 1
    # Both contributors must survive, not just the first.
    assert len(roundtripped[0]["contributors"]) == 2


def test_write_archive_batches_records():
    records = [
        {"id": "https://doi.org/10.1/a", "type": "JournalArticle"},
        {"id": "https://doi.org/10.1/b", "type": "JournalArticle"},
        {"id": "https://doi.org/10.1/c", "type": "JournalArticle"},
    ]

    entries = commonmeta_rs.write_archive(records, "commonmeta", "out.json", 1)
    assert [name for name, _ in entries] == ["out-00000.json", "out-00001.json", "out-00002.json"]


def test_write_archive_empty_list_raises():
    with pytest.raises(ValueError):
        commonmeta_rs.write_archive([], "commonmeta", "out.json", 100_000)


def test_fetch_vraix_from_local_sqlite_fixture(tmp_path):
    db_path = tmp_path / "crossref.sqlite3"
    connection = sqlite3.connect(db_path)
    connection.execute("CREATE TABLE works (pid TEXT, source_id INTEGER, raw_metadata TEXT)")
    connection.execute(
        "INSERT INTO works VALUES (?, ?, ?)",
        ("10.1234/a", 1, '{"DOI":"10.1234/a","type":"journal-article","title":["Hello"]}'),
    )
    connection.commit()
    connection.close()

    records = commonmeta_rs.fetch_vraix("crossref", "2026-06-14", str(db_path), None, 0, 30)
    assert len(records) == 1
    assert records[0]["id"] == "https://doi.org/10.1234/a"


def test_fetch_vraix_rejects_unsupported_source(tmp_path):
    db_path = tmp_path / "openalex.sqlite3"
    connection = sqlite3.connect(db_path)
    connection.execute("CREATE TABLE works (pid TEXT, source_id INTEGER, raw_metadata TEXT)")
    connection.execute("INSERT INTO works VALUES (?, ?, ?)", ("p", 1, "{}"))
    connection.commit()
    connection.close()

    with pytest.raises(ValueError):
        commonmeta_rs.fetch_vraix("openalex", "2026-06-14", str(db_path), None, 0, 30)
