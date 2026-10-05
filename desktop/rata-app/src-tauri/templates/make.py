#!/usr/bin/env python3
"""Write RATA's three blank Office files: blank.docx, blank.xlsx, blank.pptx.

Create file (K6, src/created.rs) embeds these with include_bytes! and writes
them as they are; the page never supplies a file's bytes (SEC-9). See
README.md beside this script for why, and for how they were checked.

Every part is written here by hand, from the smallest set each format needs
to open in Word, Excel, PowerPoint, LibreOffice, Pages, Numbers and Keynote.
Nothing else goes in: no relationship leaves the package, no macros, no
embedded objects, no ActiveX, no external links, no author, no dates.

Reproducible: the entries are stored (not compressed), in a fixed order,
with a fixed time (1980-01-01 00:00), so running this again writes the same
bytes on any machine and any Python 3.

    python3 make.py          # writes the three files beside this script
"""

import os
import zipfile

HERE = os.path.dirname(os.path.abspath(__file__))

DECL = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'

NS_CT = "http://schemas.openxmlformats.org/package/2006/content-types"
NS_PR = "http://schemas.openxmlformats.org/package/2006/relationships"
R_DOC = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
R_CORE = "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties"
R_APP = R_DOC + "/extended-properties"

CT_RELS = "application/vnd.openxmlformats-package.relationships+xml"
CT_CORE = "application/vnd.openxmlformats-package.core-properties+xml"
CT_APP = "application/vnd.openxmlformats-officedocument.extended-properties+xml"

# Document properties with nothing in them: no author, no company, no time.
CORE = (
    DECL
    + '<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties"'
    ' xmlns:dc="http://purl.org/dc/elements/1.1/"'
    ' xmlns:dcterms="http://purl.org/dc/terms/"'
    ' xmlns:dcmitype="http://purl.org/dc/dcmitype/"'
    ' xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"/>'
)
APP = (
    DECL
    + '<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"'
    ' xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"/>'
)


def types(overrides):
    """[Content_Types].xml: rels and xml by extension, each part by name."""
    out = [
        DECL,
        f'<Types xmlns="{NS_CT}">',
        f'<Default Extension="rels" ContentType="{CT_RELS}"/>',
        '<Default Extension="xml" ContentType="application/xml"/>',
    ]
    for part, kind in overrides:
        out.append(f'<Override PartName="/{part}" ContentType="{kind}"/>')
    out.append("</Types>")
    return "".join(out)


def rels(items):
    """A relationships part: (id, type, target), every target inside."""
    out = [DECL, f'<Relationships xmlns="{NS_PR}">']
    for rid, kind, target in items:
        out.append(f'<Relationship Id="{rid}" Type="{kind}" Target="{target}"/>')
    out.append("</Relationships>")
    return "".join(out)


def package_rels(main):
    return rels(
        [
            ("rId1", R_DOC + "/officeDocument", main),
            ("rId2", R_CORE, "docProps/core.xml"),
            ("rId3", R_APP, "docProps/app.xml"),
        ]
    )


def write(name, parts):
    """A zip of `parts` ((name, text) in order), stored, fixed time."""
    path = os.path.join(HERE, name)
    with zipfile.ZipFile(path, "w", zipfile.ZIP_STORED) as z:
        for part, text in parts:
            info = zipfile.ZipInfo(part, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_STORED
            info.create_system = 0
            info.external_attr = 0
            z.writestr(info, text.encode("utf-8"))
    print(f"{name}: {os.path.getsize(path)} bytes")


# ---------------------------------------------------------------------------
# Word: one empty paragraph on an A4 page, Calibri 11.

W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
CT_W = "application/vnd.openxmlformats-officedocument.wordprocessingml"

DOCX = [
    (
        "[Content_Types].xml",
        types(
            [
                ("word/document.xml", CT_W + ".document.main+xml"),
                ("word/styles.xml", CT_W + ".styles+xml"),
                ("word/settings.xml", CT_W + ".settings+xml"),
                ("docProps/core.xml", CT_CORE),
                ("docProps/app.xml", CT_APP),
            ]
        ),
    ),
    ("_rels/.rels", package_rels("word/document.xml")),
    (
        "word/document.xml",
        DECL
        + f'<w:document xmlns:w="{W}" xmlns:r="{R_DOC}"><w:body><w:p/>'
        '<w:sectPr><w:pgSz w:w="11906" w:h="16838"/>'
        '<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"'
        ' w:header="708" w:footer="708" w:gutter="0"/>'
        '<w:cols w:space="708"/></w:sectPr></w:body></w:document>',
    ),
    (
        "word/_rels/document.xml.rels",
        rels(
            [
                ("rId1", R_DOC + "/styles", "styles.xml"),
                ("rId2", R_DOC + "/settings", "settings.xml"),
            ]
        ),
    ),
    (
        "word/styles.xml",
        DECL
        + f'<w:styles xmlns:w="{W}"><w:docDefaults><w:rPrDefault><w:rPr>'
        '<w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="Calibri" w:cs="Calibri"/>'
        '<w:sz w:val="22"/><w:szCs w:val="22"/>'
        "</w:rPr></w:rPrDefault><w:pPrDefault><w:pPr>"
        '<w:spacing w:after="160" w:line="259" w:lineRule="auto"/>'
        "</w:pPr></w:pPrDefault></w:docDefaults>"
        '<w:style w:type="paragraph" w:default="1" w:styleId="Normal">'
        '<w:name w:val="Normal"/><w:qFormat/></w:style>'
        "</w:styles>",
    ),
    (
        "word/settings.xml",
        DECL
        + f'<w:settings xmlns:w="{W}"><w:defaultTabStop w:val="720"/>'
        '<w:characterSpacingControl w:val="doNotCompress"/>'
        '<w:compat><w:compatSetting w:name="compatibilityMode"'
        ' w:uri="http://schemas.microsoft.com/office/word" w:val="15"/></w:compat>'
        "</w:settings>",
    ),
    ("docProps/core.xml", CORE),
    ("docProps/app.xml", APP),
]

# ---------------------------------------------------------------------------
# Excel: one empty sheet, Sheet1, Calibri 11.

S = "http://schemas.openxmlformats.org/spreadsheetml/2006/main"
CT_S = "application/vnd.openxmlformats-officedocument.spreadsheetml"

XLSX = [
    (
        "[Content_Types].xml",
        types(
            [
                ("xl/workbook.xml", CT_S + ".sheet.main+xml"),
                ("xl/worksheets/sheet1.xml", CT_S + ".worksheet+xml"),
                ("xl/styles.xml", CT_S + ".styles+xml"),
                ("docProps/core.xml", CT_CORE),
                ("docProps/app.xml", CT_APP),
            ]
        ),
    ),
    ("_rels/.rels", package_rels("xl/workbook.xml")),
    (
        "xl/workbook.xml",
        DECL
        + f'<workbook xmlns="{S}" xmlns:r="{R_DOC}">'
        '<bookViews><workbookView/></bookViews>'
        '<sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets>'
        "</workbook>",
    ),
    (
        "xl/_rels/workbook.xml.rels",
        rels(
            [
                ("rId1", R_DOC + "/worksheet", "worksheets/sheet1.xml"),
                ("rId2", R_DOC + "/styles", "styles.xml"),
            ]
        ),
    ),
    (
        "xl/worksheets/sheet1.xml",
        DECL
        + f'<worksheet xmlns="{S}" xmlns:r="{R_DOC}">'
        '<dimension ref="A1"/>'
        '<sheetViews><sheetView tabSelected="1" workbookViewId="0"/></sheetViews>'
        '<sheetFormatPr defaultRowHeight="15"/>'
        "<sheetData/>"
        '<pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/>'
        "</worksheet>",
    ),
    (
        "xl/styles.xml",
        DECL
        + f'<styleSheet xmlns="{S}">'
        '<fonts count="1"><font><sz val="11"/><name val="Calibri"/><family val="2"/></font></fonts>'
        '<fills count="2"><fill><patternFill patternType="none"/></fill>'
        '<fill><patternFill patternType="gray125"/></fill></fills>'
        '<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>'
        '<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>'
        '<cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs>'
        '<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>'
        "</styleSheet>",
    ),
    ("docProps/core.xml", CORE),
    ("docProps/app.xml", APP),
]

# ---------------------------------------------------------------------------
# PowerPoint: one blank 16:9 slide on a Blank layout, one master, one theme.

P = "http://schemas.openxmlformats.org/presentationml/2006/main"
A = "http://schemas.openxmlformats.org/drawingml/2006/main"
CT_P = "application/vnd.openxmlformats-officedocument.presentationml"
CT_THEME = "application/vnd.openxmlformats-officedocument.theme+xml"

# An empty shape tree, which every slide, layout and master needs.
SP_TREE = (
    '<p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>'
    '<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/>'
    '<a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree>'
)
P_NS = f'xmlns:a="{A}" xmlns:r="{R_DOC}" xmlns:p="{P}"'


def solid(color):
    return f'<a:solidFill><a:schemeClr val="{color}"/></a:solidFill>'


# The Office theme's colours and fonts, flat: three of each style list, the
# least the schema allows.
THEME = (
    DECL
    + f'<a:theme xmlns:a="{A}" name="Office Theme"><a:themeElements>'
    '<a:clrScheme name="Office">'
    '<a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1>'
    '<a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1>'
    '<a:dk2><a:srgbClr val="44546A"/></a:dk2>'
    '<a:lt2><a:srgbClr val="E7E6E6"/></a:lt2>'
    '<a:accent1><a:srgbClr val="4472C4"/></a:accent1>'
    '<a:accent2><a:srgbClr val="ED7D31"/></a:accent2>'
    '<a:accent3><a:srgbClr val="A5A5A5"/></a:accent3>'
    '<a:accent4><a:srgbClr val="FFC000"/></a:accent4>'
    '<a:accent5><a:srgbClr val="5B9BD5"/></a:accent5>'
    '<a:accent6><a:srgbClr val="70AD47"/></a:accent6>'
    '<a:hlink><a:srgbClr val="0563C1"/></a:hlink>'
    '<a:folHlink><a:srgbClr val="954F72"/></a:folHlink>'
    "</a:clrScheme>"
    '<a:fontScheme name="Office">'
    '<a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont>'
    '<a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont>'
    "</a:fontScheme>"
    '<a:fmtScheme name="Office">'
    "<a:fillStyleLst>" + solid("phClr") * 3 + "</a:fillStyleLst>"
    "<a:lnStyleLst>"
    + ('<a:ln w="6350">' + solid("phClr") + "</a:ln>")
    + ('<a:ln w="12700">' + solid("phClr") + "</a:ln>")
    + ('<a:ln w="19050">' + solid("phClr") + "</a:ln>")
    + "</a:lnStyleLst>"
    "<a:effectStyleLst>"
    + "<a:effectStyle><a:effectLst/></a:effectStyle>" * 3
    + "</a:effectStyleLst>"
    "<a:bgFillStyleLst>" + solid("phClr") * 3 + "</a:bgFillStyleLst>"
    "</a:fmtScheme></a:themeElements></a:theme>"
)

CLR_MAP = (
    '<p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1"'
    ' accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5"'
    ' accent6="accent6" hlink="hlink" folHlink="folHlink"/>'
)

PPTX = [
    (
        "[Content_Types].xml",
        types(
            [
                ("ppt/presentation.xml", CT_P + ".presentation.main+xml"),
                ("ppt/slideMasters/slideMaster1.xml", CT_P + ".slideMaster+xml"),
                ("ppt/slideLayouts/slideLayout1.xml", CT_P + ".slideLayout+xml"),
                ("ppt/slides/slide1.xml", CT_P + ".slide+xml"),
                ("ppt/theme/theme1.xml", CT_THEME),
                ("ppt/presProps.xml", CT_P + ".presProps+xml"),
                ("ppt/viewProps.xml", CT_P + ".viewProps+xml"),
                ("ppt/tableStyles.xml", CT_P + ".tableStyles+xml"),
                ("docProps/core.xml", CT_CORE),
                ("docProps/app.xml", CT_APP),
            ]
        ),
    ),
    ("_rels/.rels", package_rels("ppt/presentation.xml")),
    (
        "ppt/presentation.xml",
        DECL
        + f'<p:presentation {P_NS} saveSubsetFonts="1">'
        '<p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst>'
        '<p:sldIdLst><p:sldId id="256" r:id="rId2"/></p:sldIdLst>'
        '<p:sldSz cx="12192000" cy="6858000"/>'
        '<p:notesSz cx="6858000" cy="9144000"/>'
        "</p:presentation>",
    ),
    (
        "ppt/_rels/presentation.xml.rels",
        rels(
            [
                ("rId1", R_DOC + "/slideMaster", "slideMasters/slideMaster1.xml"),
                ("rId2", R_DOC + "/slide", "slides/slide1.xml"),
                ("rId3", R_DOC + "/presProps", "presProps.xml"),
                ("rId4", R_DOC + "/viewProps", "viewProps.xml"),
                ("rId5", R_DOC + "/theme", "theme/theme1.xml"),
                ("rId6", R_DOC + "/tableStyles", "tableStyles.xml"),
            ]
        ),
    ),
    (
        "ppt/slideMasters/slideMaster1.xml",
        DECL
        + f"<p:sldMaster {P_NS}>"
        "<p:cSld><p:bg><p:bgRef idx=\"1001\"><a:schemeClr val=\"bg1\"/></p:bgRef></p:bg>"
        + SP_TREE
        + "</p:cSld>"
        + CLR_MAP
        + '<p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst>'
        "<p:txStyles><p:titleStyle/><p:bodyStyle/><p:otherStyle/></p:txStyles>"
        "</p:sldMaster>",
    ),
    (
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        rels(
            [
                ("rId1", R_DOC + "/slideLayout", "../slideLayouts/slideLayout1.xml"),
                ("rId2", R_DOC + "/theme", "../theme/theme1.xml"),
            ]
        ),
    ),
    (
        "ppt/slideLayouts/slideLayout1.xml",
        DECL
        + f'<p:sldLayout {P_NS} type="blank" preserve="1">'
        '<p:cSld name="Blank">' + SP_TREE + "</p:cSld>"
        "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>"
        "</p:sldLayout>",
    ),
    (
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        rels([("rId1", R_DOC + "/slideMaster", "../slideMasters/slideMaster1.xml")]),
    ),
    (
        "ppt/slides/slide1.xml",
        DECL
        + f"<p:sld {P_NS}>"
        "<p:cSld>" + SP_TREE + "</p:cSld>"
        "<p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>"
        "</p:sld>",
    ),
    (
        "ppt/slides/_rels/slide1.xml.rels",
        rels([("rId1", R_DOC + "/slideLayout", "../slideLayouts/slideLayout1.xml")]),
    ),
    ("ppt/theme/theme1.xml", THEME),
    ("ppt/presProps.xml", DECL + f"<p:presentationPr {P_NS}/>"),
    (
        "ppt/viewProps.xml",
        DECL
        + f"<p:viewPr {P_NS}>"
        '<p:normalViewPr><p:restoredLeft sz="15620"/><p:restoredTop sz="94660"/></p:normalViewPr>'
        '<p:gridSpacing cx="76200" cy="76200"/>'
        "</p:viewPr>",
    ),
    (
        "ppt/tableStyles.xml",
        DECL
        + f'<a:tblStyleLst xmlns:a="{A}" def="{{5C22544A-7EE6-4342-B048-85BDC9FD1C3A}}"/>',
    ),
    ("docProps/core.xml", CORE),
    ("docProps/app.xml", APP),
]

if __name__ == "__main__":
    write("blank.docx", DOCX)
    write("blank.xlsx", XLSX)
    write("blank.pptx", PPTX)
