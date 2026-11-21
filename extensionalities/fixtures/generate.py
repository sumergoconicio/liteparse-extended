"""Reproducible, self-authored tiny fixtures. No downloaded documents."""
from pathlib import Path
import struct
import zlib
import zipfile

ROOT = Path(__file__).resolve().parent


def archive(name, entries):
    with zipfile.ZipFile(ROOT / name, "w", zipfile.ZIP_DEFLATED) as out:
        for path, body in entries.items():
            info = zipfile.ZipInfo(path, (2026, 1, 2, 3, 4, 6))
            info.compress_type = zipfile.ZIP_STORED if name.endswith(".epub") and path == "mimetype" else zipfile.ZIP_DEFLATED
            out.writestr(info, body)


def png(w, h, document=False):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    raw = (b"\0" + b"\xff\xff\xff" * w) * h
    if document:
        raw = b"".join(b"\0" + b"".join(b"\0\0\0" if 50 < x < w - 50 and y % 24 < 3 else b"\xff\xff\xff" for x in range(w)) for y in range(h))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
            + (chunk(b"pHYs", struct.pack(">IIB", 11811, 11811, 1)) if document else b"")
            + chunk(b"IDAT", zlib.compress(raw))
            + chunk(b"IEND", b""))


archive("listing.zip", {"note.txt": "Hello archive\n", "../escape.txt": "Never extract me\n"})
(ROOT / "image.png").write_bytes(png(640, 400))
(ROOT / "document-image.png").write_bytes(png(600, 800, document=True))
(ROOT / "disguised.pdf").write_bytes(png(10, 10))
(ROOT / "truncated.zip").write_bytes(b"PK\x03\x04broken")
archive("entry-cap.zip", {f"entry-{i}": "" for i in range(4097)})
archive("bomb.zip", {"huge.bin": b""})
bomb = bytearray((ROOT / "bomb.zip").read_bytes())
central = bomb.index(b"PK\x01\x02")
struct.pack_into("<I", bomb, central + 24, 257 << 20)
(ROOT / "bomb.zip").write_bytes(bomb)

W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
A = "http://schemas.openxmlformats.org/drawingml/2006/main"
P = "http://schemas.openxmlformats.org/presentationml/2006/main"
S = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
PKG = "http://schemas.openxmlformats.org/package/2006/relationships"
CT = "http://schemas.openxmlformats.org/package/2006/content-types"
DC = "http://purl.org/dc/elements/1.1/"
CORE = f'''<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="{DC}"><dc:title>Field Notes</dc:title><dc:creator>Ada Example</dc:creator><dc:language>en-GB</dc:language></cp:coreProperties>'''


def ooxml(name, main, content_type, body, extra=None):
    entries = {
        "[Content_Types].xml": f'<Types xmlns="{CT}"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/{main}" ContentType="{content_type}"/></Types>',
        "_rels/.rels": f'<Relationships xmlns="{PKG}"><Relationship Id="rId1" Type="{R}/officeDocument" Target="{main}"/></Relationships>',
        "docProps/core.xml": CORE,
        "docProps/app.xml": '<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>Self-authored fixture</Application><Pages>999</Pages></Properties>',
        main: body,
    }
    entries.update(extra or {})
    archive(name, entries)


DOCX_TYPE = "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"
DOCX = f'''<w:document xmlns:w="{W}" xmlns:a="{A}"><w:body>
<w:p><w:r><w:t>This is a native text layer with an ampersand &amp; evidence.</w:t></w:r></w:p>
<w:p><w:pPr><w:sectPr/></w:pPr><w:r><w:t>Section one ends here.</w:t></w:r></w:p>
<w:p><w:r><w:t>Scan</w:t><w:drawing><a:blip/></w:drawing></w:r></w:p><w:sectPr/>
</w:body></w:document>'''
ooxml("notes.docx", "word/document.xml", DOCX_TYPE, DOCX, {"docProps/thumbnail.png": png(640, 400)})
ooxml("renamed.zip", "word/document.xml", DOCX_TYPE, DOCX)
ooxml("malformed.docx", "word/document.xml", DOCX_TYPE, '<w:document xmlns:w="' + W + '"><w:body>')
ooxml("deck.pptx", "ppt/presentation.xml",
      "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml",
      f'<p:presentation xmlns:p="{P}" xmlns:r="{R}"><p:sldIdLst><p:sldId id="256" r:id="second"/><p:sldId id="257" r:id="first"/></p:sldIdLst></p:presentation>', {
          "ppt/_rels/presentation.xml.rels": f'<Relationships xmlns="{PKG}"><Relationship Id="first" Type="{R}/slide" Target="slides/slide1.xml"/><Relationship Id="second" Type="{R}/slide" Target="slides/slide2.xml"/></Relationships>',
          "ppt/slides/slide1.xml": f'<p:sld xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree><p:pic><a:blip/></p:pic><p:sp><a:p><a:r><a:t>Scan</a:t></a:r></a:p></p:sp></p:spTree></p:cSld></p:sld>',
          "ppt/slides/slide2.xml": f'<p:sld xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree><p:sp><a:p><a:r><a:t>This is the first slide in presentation order.</a:t></a:r></a:p></p:sp></p:spTree></p:cSld></p:sld>',
      })

PROSE = "We can inspect the original source and verify the evidence in this answer."
SECOND_PROSE = "This is another native note about the source and its evidence."
ooxml("grid.xlsx", "xl/workbook.xml",
      "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml",
      f'<workbook xmlns="{S}" xmlns:r="{R}"><sheets><sheet name="Summary" sheetId="1" r:id="summary"/><sheet name="Notes" sheetId="2" r:id="notes"/></sheets></workbook>', {
          "xl/_rels/workbook.xml.rels": f'<Relationships xmlns="{PKG}"><Relationship Id="notes" Type="{R}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="summary" Type="{R}/worksheet" Target="worksheets/sheet2.xml"/><Relationship Id="strings" Type="{R}/sharedStrings" Target="sharedStrings.xml"/></Relationships>',
          "xl/sharedStrings.xml": f'<sst xmlns="{S}"><si><t>{PROSE}</t></si><si><r><t>This is another native note </t></r><r><t>about the source and its evidence.</t></r></si></sst>',
          "xl/worksheets/sheet2.xml": f'<worksheet xmlns="{S}"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Label</t></is></c><c r="B1" t="inlineStr"><is><t>Total</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>count</t></is></c><c r="B2"><v>7</v></c></row></sheetData></worksheet>',
          "xl/worksheets/sheet1.xml": f'''<worksheet xmlns="{S}"><dimension ref="A1:XFD1048576"/><sheetData>
<row r="1"><c r="A1" t="inlineStr"><is><t>ID</t></is></c><c r="B1" t="inlineStr"><is><t>Notes</t></is></c><c r="C1" t="inlineStr"><is><t>Flag</t></is></c></row>
<row r="2"><c r="A2"><v>1</v></c><c r="B2" t="s"><v>0</v></c><c r="C2" t="b"><v>1</v></c></row>
<row r="3"><c r="A3"><v>2</v></c><c r="B3" t="s"><v>0</v></c><c r="C3"><f>1+2</f><v>3</v></c></row>
<row r="4"><c r="A4"><v>3</v></c><c r="B4" t="s"><v>1</v></c></row></sheetData><mergeCells><mergeCell ref="B4:C4"/></mergeCells></worksheet>''',
      })

O = "urn:oasis:names:tc:opendocument:xmlns:office:1.0"
T = "urn:oasis:names:tc:opendocument:xmlns:text:1.0"
TABLE = "urn:oasis:names:tc:opendocument:xmlns:table:1.0"
DRAW = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0"
META = "urn:oasis:names:tc:opendocument:xmlns:meta:1.0"


def odf(name, kind, body):
    archive(name, {
        "mimetype": "application/vnd.oasis.opendocument." + kind,
        "meta.xml": f'<office:document-meta xmlns:office="{O}" xmlns:dc="{DC}" xmlns:meta="{META}"><office:meta><dc:title>Open Notes</dc:title><dc:creator>Grace Example</dc:creator><dc:language>fr-FR</dc:language></office:meta></office:document-meta>',
        "content.xml": f'<office:document-content xmlns:office="{O}" xmlns:text="{T}" xmlns:table="{TABLE}" xmlns:draw="{DRAW}"><office:body>{body}</office:body></office:document-content>',
        "Thumbnails/thumbnail.png": png(64, 40),
    })


odf("notes.odt", "text", '<office:text><text:section text:name="intro"><text:p>Café <text:span>et preuves</text:span> ici.<text:tab/>Texte<text:s text:c="2"/>natif.</text:p></text:section><text:section text:name="scan"><text:p>Scan</text:p><draw:frame><draw:image/></draw:frame></text:section></office:text>')
odf("deck.odp", "presentation", '<office:presentation><draw:page draw:name="one"><text:p>This is the first native ODF presentation slide.</text:p></draw:page><draw:page draw:name="two"><draw:image/><text:p>Scan</text:p></draw:page></office:presentation>')
odf("grid.ods", "spreadsheet", f'''<office:spreadsheet><table:table table:name="Notes">
<table:table-row><table:table-cell office:value-type="string"><text:p>ID</text:p></table:table-cell><table:table-cell office:value-type="string"><text:p>Notes</text:p></table:table-cell></table:table-row>
<table:table-row table:number-rows-repeated="2"><table:table-cell office:value-type="float" office:value="7"/><table:table-cell office:value-type="string"><text:p>{PROSE}</text:p></table:table-cell></table:table-row>
<table:table-row><table:table-cell office:value-type="float" office:value="3" table:formula="of:=1+2"/><table:table-cell office:value-type="string" table:number-columns-spanned="2"><text:p>{SECOND_PROSE}</text:p></table:table-cell><table:covered-table-cell/></table:table-row>
<table:table-row table:number-rows-repeated="1048572"><table:table-cell table:number-columns-repeated="16384"/></table:table-row>
</table:table></office:spreadsheet>''')


def repack(source, name, changes):
    with zipfile.ZipFile(ROOT / source) as z:
        entries = {n: z.read(n) for n in z.namelist()}
    entries.update(changes)
    archive(name, entries)


repack("notes.docx", "spoofed-language.docx", {
    "docProps/core.xml": '<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:fake="urn:not-dublin-core"><fake:language>en</fake:language></cp:coreProperties>'
})
repack("notes.docx", "entity.docx", {
    "word/document.xml": f'<!DOCTYPE w:document [<!ENTITY secret SYSTEM "file:///etc/passwd">]><w:document xmlns:w="{W}"><w:body><w:p><w:r><w:t>&secret;</w:t></w:r></w:p></w:body></w:document>'
})
repack("notes.docx", "read-cap.docx", {"word/document.xml": b" " * ((16 << 20) + 1)})
repack("grid.xlsx", "external.xlsx", {
    "xl/_rels/workbook.xml.rels": f'<Relationships xmlns="{PKG}"><Relationship Id="summary" Type="{R}/worksheet" Target="https://example.invalid/data" TargetMode="External"/><Relationship Id="notes" Type="{R}/worksheet" Target="../../outside.xml"/></Relationships>'
})
repack("notes.docx", "encrypted.docx", {})
encrypted = bytearray((ROOT / "encrypted.docx").read_bytes())
pos = 0
while pos < len(encrypted) - 4:
    if encrypted[pos:pos+4] == b"PK\x03\x04":
        flags = struct.unpack_from("<H", encrypted, pos + 6)[0]
        struct.pack_into("<H", encrypted, pos + 6, flags | 1)
    elif encrypted[pos:pos+4] == b"PK\x01\x02":
        flags = struct.unpack_from("<H", encrypted, pos + 8)[0]
        struct.pack_into("<H", encrypted, pos + 8, flags | 1)
    pos += 1
(ROOT / "encrypted.docx").write_bytes(encrypted)
(ROOT / "image.docx").write_bytes(png(640, 400))


def wp(text):
    return f"<w:p><w:r><w:t>{text}</w:t></w:r></w:p>"


WORD_TABLE = f'''<w:tbl><w:tblGrid><w:gridCol w:w="1000"/><w:gridCol w:w="2000"/><w:gridCol w:w="2000"/></w:tblGrid>
<w:tr><w:trPr><w:tblHeader/></w:trPr><w:tc>{wp("Name")}</w:tc><w:tc>{wp("Analysis")}</w:tc><w:tc>{wp("Status")}</w:tc></w:tr>
<w:tr><w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr>{wp("Group")}</w:tc><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr>{wp("Shared analysis across two columns.")}</w:tc></w:tr>
<w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr>{wp("")}</w:tc><w:tc>{wp("Before nested.")}<w:tbl><w:tblGrid><w:gridCol/></w:tblGrid><w:tr><w:tc>{wp("Nested evidence.")}</w:tc></w:tr></w:tbl>{wp("After nested.")}</w:tc><w:tc>{wp("")}</w:tc></w:tr>
</w:tbl>'''
ooxml("tables.docx", "word/document.xml", DOCX_TYPE, f'<w:document xmlns:w="{W}"><w:body>{wp("A document with a structured table.")}{WORD_TABLE}<w:sectPr/></w:body></w:document>')
ooxml("bad-merge.docx", "word/document.xml", DOCX_TYPE, f'<w:document xmlns:w="{W}"><w:body><w:tbl><w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr>{wp("Orphan continuation")}</w:tc></w:tr></w:tbl></w:body></w:document>')


def ac(text, attrs=""):
    return f'<a:tc {attrs}><a:txBody><a:p><a:r><a:t>{text}</a:t></a:r></a:p></a:txBody></a:tc>'


DRAWING_TABLE = f'''<a:tbl><a:tblPr firstRow="1"/><a:tblGrid><a:gridCol w="100"/><a:gridCol w="100"/><a:gridCol w="100"/></a:tblGrid>
<a:tr h="100">{ac("Wide", 'gridSpan="2"')}{ac("", 'hMerge="1"')}{ac("Right")}</a:tr>
<a:tr h="100">{ac("Tall", 'rowSpan="2"')}{ac("B")}{ac("C")}</a:tr>
<a:tr h="100">{ac("", 'vMerge="1"')}{ac("Bottom", 'gridSpan="2"')}{ac("", 'hMerge="1"')}</a:tr></a:tbl>'''
repack("deck.pptx", "tables.pptx", {
    "ppt/slides/slide2.xml": f'<p:sld xmlns:p="{P}" xmlns:a="{A}"><p:cSld><p:spTree><p:graphicFrame><a:graphic><a:graphicData>{DRAWING_TABLE}</a:graphicData></a:graphic></p:graphicFrame></p:spTree></p:cSld></p:sld>'
})

ODF_TABLE = '''<table:table table:name="Evidence"><table:table-header-rows><table:table-row>
<table:table-cell><text:p>Name</text:p></table:table-cell><table:table-cell><text:p>Analysis</text:p></table:table-cell><table:table-cell><text:p>Status</text:p></table:table-cell>
</table:table-row></table:table-header-rows><table:table-row>
<table:table-cell table:number-rows-spanned="2"><text:p>Group</text:p></table:table-cell><table:table-cell table:number-columns-spanned="2"><text:p>Shared analysis across two columns.</text:p></table:table-cell><table:covered-table-cell/>
</table:table-row><table:table-row><table:covered-table-cell/><table:table-cell><text:p>Before nested.</text:p>
<table:table table:name="Nested"><table:table-row><table:table-cell><text:p>Nested evidence.</text:p></table:table-cell></table:table-row></table:table>
<text:p>After nested.</text:p></table:table-cell><table:table-cell><text:p/></table:table-cell></table:table-row></table:table>'''
odf("tables.odt", "text", f"<office:text><text:p>A structured ODF table.</text:p>{ODF_TABLE}</office:text>")
odf("tables.odp", "presentation", f'<office:presentation><draw:page draw:name="one"><text:p>Introduction</text:p></draw:page><draw:page draw:name="two">{ODF_TABLE}</draw:page></office:presentation>')
archive("directory-cap.zip", {})
directory = bytearray((ROOT / "directory-cap.zip").read_bytes())
struct.pack_into("<HH", directory, 8, 4097, 4097)
(ROOT / "directory-cap.zip").write_bytes(directory)
archive("zip64-directory.zip", {})
end = (ROOT / "zip64-directory.zip").read_bytes()
(ROOT / "zip64-directory.zip").write_bytes(struct.pack("<4sIQI", b"PK\x06\x07", 0, 0, 1) + end)

OPF = "http://www.idpf.org/2007/opf"
CONTAINER = "urn:oasis:names:tc:opendocument:xmlns:container"
archive("native.epub", {
    "mimetype": "application/epub+zip",
    "META-INF/container.xml": f'<container xmlns="{CONTAINER}"><rootfiles><rootfile full-path="Book/package.opf" media-type="application/oebps-package+xml"/></rootfiles></container>',
    "Book/package.opf": f'''<package xmlns="{OPF}" version="2.0"><metadata xmlns:dc="{DC}"><dc:title>Bounded Book</dc:title><dc:creator>Example Writer</dc:creator><dc:language>es</dc:language><meta name="cover" content="cover"/></metadata>
<manifest><item id="last" href="last.xhtml" media-type="application/xhtml+xml"/><item id="first" href="first.xhtml" media-type="application/xhtml+xml"/><item id="cover" href="images/cover.png" media-type="image/png"/></manifest>
<spine><itemref idref="first"/><itemref idref="last"/></spine></package>''',
    "Book/last.xhtml": '<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Not body</title></head><body><p>Second chapter.</p></body></html>',
    "Book/first.xhtml": '<html xmlns="http://www.w3.org/1999/xhtml"><body><p>First <em>chapter</em> &amp; evidence.</p><script>never execute or include</script></body></html>',
    "Book/images/cover.png": png(64, 40),
})
repack("native.epub", "escaping.epub", {
    "META-INF/container.xml": f'<container xmlns="{CONTAINER}"><rootfiles><rootfile full-path="../outside.opf" media-type="application/oebps-package+xml"/></rootfiles></container>'
})

def mobi(name, version=6, compression=2, encrypted=0, payload=None):
    text = b"<html><body><p>Native MOBI abcabc &amp; evidence.</p></body></html>"
    encoded = text.replace(b" M", b"\xcd").replace(b"abcabc", b"abc\x80\x18") if compression == 2 else text
    if payload is not None:
        encoded = payload
    records = []
    for kind, value in [(100, b"Example Author"), (101, b"Example Press"), (504, b"TEST-ASIN"), (524, b"de"), (201, struct.pack(">I", 0))]:
        records.append(struct.pack(">II", kind, 8 + len(value)) + value)
    exth = b"EXTH" + struct.pack(">II", 12 + sum(map(len, records)), len(records)) + b"".join(records)
    title = b"Native Pocket Book"
    head = bytearray(248)
    struct.pack_into(">HHIHHHH", head, 0, compression, 0, len(text), 1, 4096, encrypted, 0)
    head[16:20] = b"MOBI"
    for offset, value in [(20, 232), (24, 2), (28, 65001), (36, version), (84, len(head) + len(exth)), (88, len(title)), (92, 1031), (108, 2), (128, 0x40)]:
        struct.pack_into(">I", head, offset, value)
    parts = [bytes(head) + exth + title, encoded, png(64, 40)]
    pdb = bytearray(78)
    pdb[60:68] = b"BOOKMOBI"
    struct.pack_into(">H", pdb, 76, len(parts))
    offset = 78 + 8 * len(parts) + 2
    for part in parts:
        pdb.extend(struct.pack(">II", offset, 0))
        offset += len(part)
    (ROOT / name).write_bytes(pdb + b"\0\0" + b"".join(parts))


mobi("native.mobi")
mobi("native.azw3", version=8, compression=1)
mobi("encrypted.azw", encrypted=2)
mobi("huff.mobi", compression=17480)
mobi("broken-lz77.mobi", payload=b"\x80\x00")
archive("pages.cbz", {"ComicInfo.xml": "<ComicInfo/>", "02.png": png(64, 40), "01.png": png(128, 80)})

def iff(kind, data):
    return kind + struct.pack(">I", len(data)) + data + (b"\0" if len(data) & 1 else b"")


def djvu_page(width, height):
    info = struct.pack(">HHBB", width, height, 26, 0) + struct.pack("<H", 300) + b"\x16\x01"
    return iff(b"FORM", b"DJVU" + iff(b"INFO", info))


components = [iff(b"FORM", b"DJVI"), djvu_page(600, 800), djvu_page(900, 1200)]
directory_size = 3 + 4 * len(components) + 1
offset = 16 + 8 + directory_size + (directory_size & 1)
offsets = []
for component in components:
    offsets.append(offset)
    offset += len(component)
directory = b"\x81" + struct.pack(">H", len(components)) + b"".join(struct.pack(">I", o) for o in offsets) + b"\0"
(ROOT / "bundled.djvu").write_bytes(b"AT&T" + iff(b"FORM", b"DJVM" + iff(b"DIRM", directory) + b"".join(components)))
(ROOT / "single.djvu").write_bytes(b"AT&T" + djvu_page(600, 800))
(ROOT / "indirect.djvu").write_bytes(b"AT&T" + iff(b"FORM", b"DJVM" + iff(b"DIRM", b"\x01\0\x03\0")))
(ROOT / "truncated.djvu").write_bytes((ROOT / "bundled.djvu").read_bytes()[:-3])

def rar4_block(kind, flags, body=b"", data=b""):
    header = struct.pack("<BHH", kind, flags, 7 + len(body)) + body
    return struct.pack("<H", zlib.crc32(header) & 0xffff) + header + data


rar4 = bytearray(b"Rar!\x1a\x07\x00" + rar4_block(0x73, 0, b"\0" * 6))
for name in [b"02.png", b"01.png"]:
    data = png(64, 40)
    body = struct.pack("<IIBIIBBHI", len(data), len(data), 3, zlib.crc32(data), 0, 20, 0x30, len(name), 0) + name
    rar4.extend(rar4_block(0x74, 0x8000, body, data))
rar4.extend(rar4_block(0x7b, 0x4000))
(ROOT / "pages4.cbr").write_bytes(rar4)

def vint(n):
    result = bytearray()
    while n >= 128:
        result.append((n & 127) | 128)
        n >>= 7
    return result + bytes([n])


def rar5_block(kind, flags, body=b"", data=b""):
    header = vint(kind) + vint(flags) + (vint(len(data)) if flags & 2 else b"") + body
    checked = vint(len(header)) + header
    return struct.pack("<I", zlib.crc32(checked)) + checked + data


rar5 = bytearray(b"Rar!\x1a\x07\x01\x00" + rar5_block(1, 0, vint(0)))
for name in [b"02.png", b"01.png"]:
    data = png(64, 40)
    body = vint(0) + vint(len(data)) + vint(0) + vint(0) + vint(1) + vint(len(name)) + name
    rar5.extend(rar5_block(2, 2, body, data))
rar5.extend(rar5_block(5, 0, vint(0)))
(ROOT / "pages5.cbr").write_bytes(rar5)
(ROOT / "truncated.cbr").write_bytes(rar5[:-3])
(ROOT / "encrypted.cbr").write_bytes(b"Rar!\x1a\x07\x00" + rar4_block(0x73, 0x80, b"\0" * 6))
with zipfile.ZipFile(ROOT / "native.epub") as book:
    package = book.read("Book/package.opf").decode().replace('href="first.xhtml"', 'href="chapter%20one.xhtml"')
repack("native.epub", "doctype.epub", {
    "Book/package.opf": package,
    "Book/chapter one.xhtml": '<!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "https://example.invalid/no-fetch.dtd"><html xmlns="http://www.w3.org/1999/xhtml"><body><p>First&nbsp;chapter.</p></body></html>',
})
repack("native.epub", "xxe.epub", {
    "Book/first.xhtml": '<!DOCTYPE html [<!ENTITY secret SYSTEM "file:///etc/passwd">]><html xmlns="http://www.w3.org/1999/xhtml"><body><p>&secret;</p></body></html>',
})

def pdf(multipage=False):
    def stream(data, attributes=b""):
        return b"<< /Length " + str(len(data)).encode() + b" " + attributes + b" >>\nstream\n" + data + b"\nendstream"
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << /Font << /F1 5 0 R >> >> /Contents 6 0 R >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << /XObject << /Scan 8 0 R >> >> /Contents 7 0 R >>",
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        stream(b"BT /F1 20 Tf 50 700 Td (Native PDF evidence for bounded preview.) Tj ET"),
        stream(b"q 600 0 0 800 0 0 cm /Scan Do Q"),
        stream(b"\xff\xff\xff", b"/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceRGB /BitsPerComponent 8"),
        b"<< /Title (Native PDF) /Author (Example Author) >>",
    ]
    if multipage:
        kids = [3, 4]
        dense = b"BT /F1 14 Tf 40 740 Td " + b" ".join(
            (b"0 -28 Td " if n else b"") + b"(Native text paragraphs provide substantial document evidence.) Tj"
            for n in range(20)) + b" ET"
        for body, width, height in [
            (dense, 600, 800),
            (b"", 600, 800),
            (b"q 200 0 0 200 200 350 cm /Scan Do Q BT /F1 20 Tf 50 700 Td (Caption with an embedded figure.) Tj ET", 600, 800),
            (b"BT /F1 20 Tf 50 500 Td (Final page confirms full-document extraction.) Tj ET", 800, 600),
        ]:
            objects.append(stream(body))
            content = len(objects)
            objects.append(f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Resources << /Font << /F1 5 0 R >> /XObject << /Scan 8 0 R >> >> /Contents {content} 0 R >>".encode())
            kids.append(len(objects))
        objects[1] = b"<< /Type /Pages /Kids [" + b" ".join(f"{n} 0 R".encode() for n in kids) + b"] /Count 6 >>"
    data = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets = [0]
    for n, obj in enumerate(objects, 1):
        offsets.append(len(data))
        data.extend(f"{n} 0 obj\n".encode() + obj + b"\nendobj\n")
    xref = len(data)
    data.extend(f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode())
    for offset in offsets[1:]:
        data.extend(f"{offset:010} 00000 n \n".encode())
    data.extend(f"trailer\n<< /Size {len(offsets)} /Root 1 0 R /Info 9 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode())
    return data


(ROOT / "native.pdf").write_bytes(pdf())
(ROOT / "multipage.pdf").write_bytes(pdf(multipage=True))
(ROOT / "broken.pdf").write_bytes(b"%PDF-1.7\nbroken")
