use quick_xml::events::Event;
use quick_xml::Reader;

/// quick-xml is a streaming parser: explicitly require a complete single-root
/// document before merging retained fragments or editing configuration.
pub(super) fn validate_document(xml: &str, root: &str) -> anyhow::Result<()> {
    let mut reader = Reader::from_str(xml);
    let mut depth = 0usize;
    let mut seen = false;
    loop {
        let event = reader.read_event()?;
        match event {
            Event::Start(ref element) | Event::Empty(ref element) => {
                if depth == 0 {
                    anyhow::ensure!(
                        !seen && element.name().as_ref() == root.as_bytes(),
                        "unexpected or duplicate XML root"
                    );
                    seen = true;
                }
                for attribute in element.attributes() {
                    attribute?.normalized_value(quick_xml::XmlVersion::default())?;
                }
                if matches!(event, Event::Start(_)) {
                    depth += 1;
                }
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| anyhow::anyhow!("unexpected XML end"))?;
            }
            Event::Text(text) => {
                let text = text.decode()?;
                anyhow::ensure!(depth > 0 || text.trim().is_empty(), "text outside XML root");
            }
            Event::DocType(_) => anyhow::bail!("XML document types are unsupported"),
            Event::GeneralRef(reference) => {
                let name = reference.decode()?;
                anyhow::ensure!(
                    depth > 0
                        && (matches!(name.as_ref(), "amp" | "lt" | "gt" | "apos" | "quot")
                            || reference.resolve_char_ref()?.is_some()),
                    "unsupported XML entity"
                );
            }
            Event::Eof => break,
            _ => {}
        }
    }
    anyhow::ensure!(seen && depth == 0, "incomplete XML document");
    Ok(())
}
