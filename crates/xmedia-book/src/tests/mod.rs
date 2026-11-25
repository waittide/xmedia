mod catalog_and_storage;
mod formats_and_metadata;
mod online_e2e;
mod online_unit;

use std::io::{Cursor, Write};

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

pub(super) fn sample_epub_bytes() -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zw = ZipWriter::new(&mut cursor);
        let opts = SimpleFileOptions::default();

        zw.start_file("META-INF/container.xml", opts).unwrap();
        zw.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
        )
        .unwrap();

        zw.start_file("OEBPS/content.opf", opts).unwrap();
        zw.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<package version="3.0" xmlns="http://www.idpf.org/2007/opf" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Foundation</dc:title>
    <dc:creator>Isaac Asimov</dc:creator>
    <dc:language>en</dc:language>
    <dc:subject>Sci-Fi</dc:subject>
  </metadata>
  <manifest>
    <item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>
    <item id="ch1" href="ch1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>"#,
        )
        .unwrap();

        zw.start_file("OEBPS/nav.xhtml", opts).unwrap();
        zw.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">
<body>
  <nav epub:type="toc">
    <ol>
      <li><a href="ch1.xhtml">The Psychohistorians</a></li>
    </ol>
  </nav>
</body>
</html>"#,
        )
        .unwrap();

        zw.start_file("OEBPS/ch1.xhtml", opts).unwrap();
        zw.write_all(
            br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Hari Seldon</p></body></html>"#,
        )
        .unwrap();

        zw.finish().unwrap();
    }
    cursor.into_inner()
}
