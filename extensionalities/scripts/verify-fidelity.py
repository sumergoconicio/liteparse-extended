"""Offline external-tool checks. Every generated file stays under target/.

Usage: python3 -B scripts/verify-fidelity.py /absolute/path/to/litx /absolute/path/to/lit
"""
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path
import json
import os
import subprocess
import sys
import zipfile

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "target" / "verification" / datetime.now(timezone.utc).strftime("fidelity-%Y%m%dT%H%M%S%fZ")
OUT.mkdir(parents=True)
LITX, LIT = map(lambda s: str(Path(s).resolve()), sys.argv[1:3])


def run(args, **kwargs):
    return subprocess.run(args, capture_output=True, timeout=60, **kwargs)


results = {"ground_truth": [], "fidelity": []}
for name in ["notes.docx", "deck.pptx", "grid.xlsx", "notes.odt", "deck.odp", "grid.ods", "tables.docx"]:
    source = ROOT / "fixtures" / name
    work = OUT / name
    work.mkdir()
    native = run([LITX, "inspect", str(source), "--max-chars", "1000000"])
    assert native.returncode == 0, native.stderr
    report = json.loads(native.stdout)
    (work / "native.json").write_bytes(native.stdout)
    assert not report["errors"], report["errors"]
    listing = run(["unzip", "-l", str(source)])
    assert listing.returncode == 0, listing.stderr
    (work / "unzip-listing.log").write_bytes(listing.stdout)
    with zipfile.ZipFile(source) as archive:
        expected = [(i.filename, i.file_size, i.compress_size) for i in archive.infolist()]
        actual = [(e["name"], e["size"], e["compressed_size"]) for e in report["entries"]]
        assert actual == expected, name
        metadata_part = "meta.xml" if name.endswith((".odt", ".odp", ".ods")) else "docProps/core.xml"
        metadata = run(["unzip", "-p", str(source), metadata_part])
        assert metadata.returncode == 0, metadata.stderr
        title = run(["xmllint", "--nonet", "--xpath", "string(//*[local-name()='title' and namespace-uri()='http://purl.org/dc/elements/1.1/'])", "-"], input=metadata.stdout)
        assert title.returncode == 0, title.stderr
        assert any(c["key"] == "title" and c["value"] == title.stdout.decode().rstrip("\n") for c in report["claims"])
        thumbnail_part = "Thumbnails/thumbnail.png"
        if thumbnail_part in archive.namelist():
            thumbnail = work / "embedded.png"
            got = run([LITX, "inspect", str(source), "--thumbnail", str(thumbnail)])
            assert got.returncode == 0
            raw = run(["unzip", "-p", str(source), thumbnail_part])
            assert raw.returncode == 0
            assert thumbnail.read_bytes() == raw.stdout, name
    results["ground_truth"].append({"file": name, "listing": "pass", "xml_title": "pass", "embedded_bytes": "pass" if name.endswith((".odt", ".odp", ".ods")) else "not applicable"})
    # Isolated LibreOffice profile; never reuse or touch the user's profile.
    converted = run(["libreoffice", "--headless", "-env:UserInstallation=" + (work / "lo-profile").as_uri(),
                     "--convert-to", "pdf", "--outdir", str(work), str(source)])
    (work / "libreoffice.log").write_bytes(converted.stdout + converted.stderr)
    pdf = work / (source.stem + ".pdf")
    if converted.returncode != 0 or not pdf.exists():
        results["fidelity"].append({"file": name, "status": "blocked", "reason": "LibreOffice did not produce PDF"})
        continue
    rendered = run([LIT, "parse", str(pdf), "--no-ocr", "--format", "json", "--quiet"])
    (work / "rendered.json").write_bytes(rendered.stdout)
    (work / "liteparse.log").write_bytes(rendered.stderr)
    if rendered.returncode != 0:
        results["fidelity"].append({"file": name, "status": "blocked", "reason": "upstream PDF parse failed"})
        continue
    parsed = json.loads(rendered.stdout)
    text = parsed.get("text", "")
    if not text:
        text = "\n".join(p.get("text", "") for p in parsed.get("pages", []))
    left = Counter(report["text"]["content"].split())
    right = Counter(text.split())
    results["fidelity"].append({"file": name, "status": "compared", "native_chars": report["counts"]["chars"],
        "rendered_chars": len(text), "native_only_tokens": dict(left - right), "rendered_only_tokens": dict(right - left)})

(OUT / "summary.json").write_text(json.dumps(results, indent=2, ensure_ascii=False) + "\n")
print(str(OUT / "summary.json"))
print(json.dumps(results, indent=2, ensure_ascii=False))
