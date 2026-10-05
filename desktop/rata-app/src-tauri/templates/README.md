# Blank Office files for Create file

`blank.docx`, `blank.xlsx` and `blank.pptx` are the files RATA writes when
the customer uses Create file (K6, `src/created.rs`). They are built into the
app with `include_bytes!` (`Format::blank`); Markdown, text and CSV files
start empty. The page never supplies a file's bytes (SEC-9): RATA opens the
new file in Word, Excel or PowerPoint at once and without a download mark,
so the file has to be one RATA knows, not one a page could fill with an
external template, a remote picture, an embedded object or a link to another
workbook.

## Where they come from

`make.py` writes all three, by hand, from nothing: no Office application,
no template downloaded from anywhere. Each holds only what its format needs
to open in Word, Excel, PowerPoint, LibreOffice, Pages, Numbers and Keynote:

- **docx**: one empty paragraph on an A4 page, Calibri 11 (`document.xml`,
  `styles.xml`, `settings.xml`).
- **xlsx**: one empty sheet named Sheet1, Calibri 11 (`workbook.xml`,
  `sheet1.xml`, `styles.xml`).
- **pptx**: one blank 16:9 slide on a Blank layout, with one slide master,
  the Office theme's colours and fonts, and `presProps.xml`, `viewProps.xml`
  and `tableStyles.xml`.

Each also has empty `docProps/core.xml` and `docProps/app.xml` (no author,
company or dates). Every relationship points inside the file; none has
`TargetMode="External"`. There are no macros, embedded objects, ActiveX
controls or external links.

To change one, edit `make.py` and run it here:

```sh
python3 make.py
```

The output is reproducible: entries are stored (not compressed), in a fixed
order, with a fixed time (1980-01-01 00:00), so the same script writes the
same bytes on any machine. Commit the script and the three files together.

## How they are checked

- `cargo test` (`created.rs`): each blank passes `check_body` for its own
  format and fails it for the other two; no entry's name or text holds
  `TargetMode`, `External`, `embeddings/`, `activeX/`, `externalLinks/`,
  `vbaProject`, `macroEnabled`, `oleObject`, `attachedTemplate`, `file:` or
  a UNC path; every relationship's target is a part in the file, every part
  is reached by a relationship and has a content type; and the pptx has its
  presentation, master, layout, slide, theme and three property parts. A
  file made with Create file is byte for byte the blank.
- When they were made (SEC-9, 2026-10): every part parsed as XML with
  Python's `xml.etree`; LibreOffice 24.2 (`soffice --headless --convert-to
  pdf`, and to odt, ods and odp) read each one: the docx as one A4 page in
  Calibri, the xlsx as one sheet named Sheet1, the pptx as one 16:9 slide;
  and python-docx, openpyxl and python-pptx opened each, added to it and
  saved it again. Not yet opened in Microsoft Office or Apple's apps: do that
  on a Mac and a Windows machine before relying on it (Word, Excel,
  PowerPoint, Pages, Numbers, Keynote), and after any change to `make.py`.
