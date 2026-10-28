# Resource preview format fixtures

These small, generated documents/media are shared by the Web and Desktop cases in
group 21. `manifest.json` declares the expected authoritative Files MIME, viewer,
asserted content (including Chinese text for Word/slides/PDF) and default/opt-in admission. It is test data, not a product
allowlist or renderer registry. An explicit `fixture` entry may store a large sample
with lossless gzip compression; the shared harness materializes its original name
and identical bytes in the report directory before testing. All assets were generated locally; no external
user document or media is included.

The eleven default MIME types are exercised against an unmodified default
FileService configuration. HTML and ZIP are also submitted there and must return
`unsupportedMediaType`. The UI matrix uses explicit disposable opt-ins to test
additional formats; preview capability never changes product upload defaults.
Both shells upload with an untrusted generic client MIME, then assert the server's
detected MIME and actual rendering/decoding.

- Word DOC/DOCX/ODT and independent PDF: table, embedded image, Chinese content,
  two pages. Assert first/second page text and a rendered PDF canvas.
- Excel XLS/XLSX/ODS: formula total 42 and multiple sheets. Assert computed value
  and the second sheet in the PDF output.
- Presentation PPT/PPTX/ODP: two slides with text/image. Assert both pages.
- PNG/JPEG/WebP/GIF/BMP: assert decoded dimensions, not just an iframe element.
- SVG: existing Files detection yields `text/plain`, so the Files matrix asserts
  source text. It does not claim Files SVG visual rendering. Session resource
  SVG loading is separately covered by the Worker HTML assets scenario.
- Markdown/CSV/plain text/JSON/TypeScript/XML/HTML: assert visible content and
  format-specific table/form behavior where applicable.
- ZIP/TAR/TGZ: show package metadata only and assert byte-exact original downloads; no directory/member preview.
- WAV (PCM), MP3, Ogg (Vorbis), WebM (VP8), MP4 (H.264): assert media metadata,
  video dimensions and advancing playback. These samples establish those codecs
  on the tested browser/platform, not every audio/video codec.

Regeneration is optional and is not needed in CI. Install Pillow, python-docx,
openpyxl and python-pptx into a disposable Python environment; provide `soffice`
and `ffmpeg` on PATH, with stable LibreOffice and CJK fonts available to its conversion process, then run:

```sh
python3 talos-e2e/scripts/resource_preview/generate_format_fixtures.py
```

The generator uses those existing libraries/CLIs. CI consumes the checked-in
bytes and only needs the production converter (Writer, Calc and Impress) plus
its existing Chrome/Electron runtime. Install a CJK font package such as Noto CJK in the conversion environment; preview does not repair missing fonts or rewrite document text. Browser-native unsupported formats/codecs
are not promoted to supported formats by filename or upload permission.
