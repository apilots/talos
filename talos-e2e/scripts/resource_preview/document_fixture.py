"""Small real document/source fixtures shared by Web and Desktop acceptance."""
import io
from pathlib import Path
import zipfile


def office_document():
    """A two-page OOXML document; production LibreOffice performs conversion."""
    result = io.BytesIO()
    with zipfile.ZipFile(result, 'w', zipfile.ZIP_DEFLATED) as package:
        package.writestr('[Content_Types].xml', '''<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>''')
        package.writestr('_rels/.rels', '''<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>''')
        package.writestr('word/document.xml', '''<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Office preview first page</w:t><w:br w:type="page"/></w:r></w:p><w:p><w:r><w:t>Office preview second page</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr></w:body></w:document>''')
    return result.getvalue()


def workspace_files(root):
    """Populate the execution Worker's actual authorized directory, never Server storage."""
    root = Path(root)
    root.mkdir(parents=True, exist_ok=True)
    (root / 'assets').mkdir(exist_ok=True)
    (root / 'reports').mkdir(exist_ok=True)
    (root / 'assets/theme.css').write_text('h1 { color: rgb(17, 102, 51); }')
    (root / 'assets/app.js').write_text('document.querySelector("#script").textContent="Worker script loaded";')
    (root / 'assets/mark.svg').write_text('<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><rect width="20" height="20" fill="green"/></svg>')
    (root / 'reports/next.html').write_text('<h1>Worker next page</h1><a href="report.html">Back to report</a>')
    (root / 'reports/report.html').write_text('''<!doctype html><title>Worker file report</title><link rel="stylesheet" href="../assets/theme.css"><h1>Worker file report</h1><p id="script"></p><script src="/assets/app.js"></script><img alt="Worker asset" src="/assets/mark.svg"><a href="next.html">Next file</a><label>Worker draft<input aria-label="Worker draft"></label>''')
    (root / 'large.txt').write_text(''.join(f'{line:06d} Worker source line\n' for line in range(1, 55001)))
    (root / 'encoded.txt').write_bytes('第一行\n第二行\n'.encode('utf-16le'))
    return root
