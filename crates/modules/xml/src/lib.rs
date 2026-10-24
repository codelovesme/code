//! The `xml` native module (ticket 103): XML text in, a tree out.
//!
//! An element is `{ tag, attrs, children }` — the shape the language's
//! `Element` type describes — where `children` holds texts and elements in
//! document order. Text between elements that is only whitespace is dropped
//! (it is the document's layout, not its content); entities are decoded and
//! CDATA is text. Comments, processing instructions and the DOCTYPE are not
//! part of the tree. Names keep their prefix (`dc:creator`, `atom:link`):
//! a feed is read by the names it is written with.
//!
//! `Parse { value }` → `ParseResult { value }`, the root element. Text that
//! is not well-formed XML answers an `Exception` naming where it broke.
//! Stateless (`code_abi.h` item 12).

use code_native::*;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// This module keeps no state, so every link of it may share one instance
/// (`code_abi.h` item 12, ticket 177).
#[no_mangle]
pub extern "C" fn code_module_stateless() -> core::ffi::c_int {
    1
}

/// # Safety
/// The host supplies valid initialized ABI values for the duration of dispatch.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    guarded(&mut *out, "xml", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Parse" => parse(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "xml", &message);
        }
    })
}

/// One element, built before it is written out as a value.
struct Element {
    tag: String,
    attrs: Vec<(String, String)>,
    children: Vec<Child>,
}

enum Child {
    Text(String),
    Element(Element),
}

fn parse(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let text = find_field(particle, "value")
        .and_then(read_str)
        .ok_or("Parse requires a text 'value'")?;
    let root = read_tree(text)?;
    make_result(out, c"ParseResult", |slot| write_element(slot, &root));
    Ok(())
}

fn read_tree(text: &str) -> Result<Element, String> {
    let mut reader = Reader::from_str(text);
    let mut open: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;
    let at = |reader: &Reader<&[u8]>| reader.buffer_position();
    loop {
        let event = reader
            .read_event()
            .map_err(|e| format!("not well-formed XML at byte {}: {e}", at(&reader)))?;
        match event {
            Event::Start(start) => open.push(element(&start)?),
            Event::Empty(start) => {
                let done = element(&start)?;
                place(&mut open, &mut root, done, at(&reader))?;
            }
            Event::End(_) => {
                let done = open.pop().ok_or_else(|| {
                    format!("a closing tag with nothing open, at byte {}", at(&reader))
                })?;
                place(&mut open, &mut root, done, at(&reader))?;
            }
            Event::Text(bytes) => {
                let decoded = bytes
                    .unescape()
                    .map_err(|e| format!("a bad entity at byte {}: {e}", at(&reader)))?;
                add_text(&mut open, &decoded, at(&reader))?;
            }
            Event::CData(bytes) => {
                let decoded = String::from_utf8_lossy(&bytes).into_owned();
                add_text(&mut open, &decoded, at(&reader))?;
            }
            Event::Eof => break,
            // Comments, processing instructions, the declaration, DOCTYPE.
            _ => {}
        }
    }
    if let Some(unclosed) = open.last() {
        return Err(format!("<{}> is never closed", unclosed.tag));
    }
    root.ok_or_else(|| "no element in the text".to_string())
}

fn element(start: &BytesStart) -> Result<Element, String> {
    let tag = String::from_utf8_lossy(start.name().as_ref()).into_owned();
    let mut attrs = Vec::new();
    for attr in start.attributes() {
        let attr = attr.map_err(|e| format!("a bad attribute in <{tag}>: {e}"))?;
        let name = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
        let value = attr
            .unescape_value()
            .map_err(|e| format!("a bad entity in <{tag}> {name}: {e}"))?
            .into_owned();
        attrs.push((name, value));
    }
    Ok(Element {
        tag,
        attrs,
        children: Vec::new(),
    })
}

/// A finished element: into its parent, or it is the root.
fn place(
    open: &mut [Element],
    root: &mut Option<Element>,
    done: Element,
    at: u64,
) -> Result<(), String> {
    match open.last_mut() {
        Some(parent) => parent.children.push(Child::Element(done)),
        None if root.is_none() => *root = Some(done),
        None => return Err(format!("a second root element <{}> at byte {at}", done.tag)),
    }
    Ok(())
}

fn add_text(open: &mut [Element], text: &str, at: u64) -> Result<(), String> {
    match open.last_mut() {
        Some(parent) => {
            if !text.trim().is_empty() {
                // Next to text that came before it (an entity, a CDATA
                // section) it is one text, as it was in the document.
                if let Some(Child::Text(before)) = parent.children.last_mut() {
                    before.push_str(text);
                } else {
                    parent.children.push(Child::Text(text.to_string()));
                }
            }
            Ok(())
        }
        None if text.trim().is_empty() => Ok(()),
        None => Err(format!("text outside the root element at byte {at}")),
    }
}

fn write_element(out: &mut CodeValue, e: &Element) {
    let mut fields = SlotBuffer::new(3);
    owned_str(fields.slot_mut(0), &e.tag);
    let names: Vec<&str> = e.attrs.iter().map(|(n, _)| n.as_str()).collect();
    let mut values = SlotBuffer::new(e.attrs.len());
    for (i, (_, v)) in e.attrs.iter().enumerate() {
        owned_str(values.slot_mut(i as i64), v);
    }
    object_dyn(fields.slot_mut(1), &names, &mut values);
    values.release_all();
    let mut kids = SlotBuffer::new(e.children.len());
    for (i, child) in e.children.iter().enumerate() {
        match child {
            Child::Text(t) => owned_str(kids.slot_mut(i as i64), t),
            Child::Element(el) => write_element(kids.slot_mut(i as i64), el),
        }
    }
    array(fields.slot_mut(2), &mut kids);
    kids.release_all();
    object(out, &[c"tag", c"attrs", c"children"], &mut fields);
    fields.release_all();
}
