//! The `markdown` native module — CommonMark + GFM to HTML, for the Code
//! programming language, written in Rust on [`code-native`] over
//! `pulldown-cmark`.
//!
//! Handlers:
//!
//! - `RenderMarkdown { src }` → `RenderedMarkdown { html, toc, links, title,
//!   slug }` — HTML, plus a flat table of contents (`{ level, text, slug }`
//!   per heading), a flat link list (`{ text, href }`), and the first
//!   heading's text and slug for convenience. Every heading in the HTML
//!   carries `id="<slug>"`.
//! - `SplitByHeading { src, level }` → `Chapters { chapters }` — the source
//!   cut into `{ slug, title, body }` at every heading of `level` (default
//!   2). Text before the first such heading is dropped.
//!
//! GFM tables, strikethrough, task lists and footnotes are on. Code fences
//! keep their `class="language-<lang>"` for a client-side highlighter; there
//! is no server-side highlighting here.
//!
//! Slugs are GitHub-style — lowercased, non-alphanumerics collapsed to `-`,
//! and a repeated heading gets `-1`, `-2`, … so every `id` is unique.
//!
//! `code_release` needs no code here — `code-native` links the vendored
//! `runtime.c` into the cdylib and re-exports it.

use code_native::*;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::collections::HashMap;

/// The ABI version this module speaks. Must equal `CODE_ABI_VERSION` or the
/// host refuses to load us.
#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// The single dispatch point: read `_class`, route to a handler. An
/// unhandled class is null; a handler that cannot do the work returns an
/// `Exception`. Neither ends the program.
///
/// # Safety
///
/// Both pointers must be valid for reads/writes for the duration of the
/// call and laid out per `code_abi.h` — the host guarantees this.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    guarded(&mut *out, "markdown", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "RenderMarkdown" => render_handler(out, particle),
            "SplitByHeading" => split_handler(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "markdown", &message);
        }
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `RenderMarkdown { src }` → `RenderedMarkdown { html, toc, links, title, slug }`.
fn render_handler(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let src = require_str(particle, "src", "RenderMarkdown")?;
    let rendered = render(src);
    let (title, slug) = rendered
        .toc
        .first()
        .map(|h| (h.text.clone(), h.slug.clone()))
        .unwrap_or_default();

    let mut toc = CodeValue::zeroed();
    array_of(&mut toc, &rendered.toc, |slot, h| {
        let mut b = SlotBuffer::new(4);
        borrowed_str(b.slot_mut(0), c"HeadingEntry");
        number(b.slot_mut(1), h.level as f64);
        owned_str(b.slot_mut(2), &h.text);
        owned_str(b.slot_mut(3), &h.slug);
        object(slot, &[c"_class", c"level", c"text", c"slug"], &mut b);
        b.release_all();
    });

    let mut links = CodeValue::zeroed();
    array_of(&mut links, &rendered.links, |slot, l| {
        let mut b = SlotBuffer::new(3);
        borrowed_str(b.slot_mut(0), c"LinkEntry");
        owned_str(b.slot_mut(1), &l.0);
        owned_str(b.slot_mut(2), &l.1);
        object(slot, &[c"_class", c"text", c"href"], &mut b);
        b.release_all();
    });

    let mut buf = SlotBuffer::new(6);
    borrowed_str(buf.slot_mut(0), c"RenderedMarkdown");
    owned_str(buf.slot_mut(1), &rendered.html);
    copy(buf.slot_mut(2), &toc);
    copy(buf.slot_mut(3), &links);
    owned_str(buf.slot_mut(4), &title);
    owned_str(buf.slot_mut(5), &slug);
    object(
        out,
        &[c"_class", c"html", c"toc", c"links", c"title", c"slug"],
        &mut buf,
    );
    buf.release_all();
    release(&mut toc);
    release(&mut links);
    Ok(())
}

/// `SplitByHeading { src, level }` → `Chapters { chapters }`.
fn split_handler(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let src = require_str(particle, "src", "SplitByHeading")?;
    let level = match find_field(particle, "level") {
        None => 2,
        Some(v) => {
            let n = read_number(v).ok_or("'level' must be a number")?;
            if n.fract() != 0.0 || !(1.0..=6.0).contains(&n) {
                return Err("'level' must be a whole number in 1..=6".to_string());
            }
            n as u8
        }
    };

    let chapters = split_by_heading(src, level);
    let mut arr = CodeValue::zeroed();
    array_of(&mut arr, &chapters, |slot, c| {
        let mut b = SlotBuffer::new(4);
        borrowed_str(b.slot_mut(0), c"Chapter");
        owned_str(b.slot_mut(1), &c.slug);
        owned_str(b.slot_mut(2), &c.title);
        owned_str(b.slot_mut(3), &c.body);
        object(slot, &[c"_class", c"slug", c"title", c"body"], &mut b);
        b.release_all();
    });

    let mut buf = SlotBuffer::new(2);
    borrowed_str(buf.slot_mut(0), c"Chapters");
    copy(buf.slot_mut(1), &arr);
    object(out, &[c"_class", c"chapters"], &mut buf);
    buf.release_all();
    release(&mut arr);
    Ok(())
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

struct Heading {
    level: u8,
    text: String,
    slug: String,
}

struct Rendered {
    html: String,
    toc: Vec<Heading>,
    links: Vec<(String, String)>,
}

fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn render(src: &str) -> Rendered {
    let mut events: Vec<Event> = Vec::new();
    let mut toc: Vec<Heading> = Vec::new();
    let mut links: Vec<(String, String)> = Vec::new();
    let mut slugs = SlugMaker::default();

    let mut heading_at: Option<(usize, u8)> = None;
    let mut heading_text = String::new();
    let mut link_href: Option<String> = None;
    let mut link_text = String::new();

    for event in Parser::new_ext(src, options()) {
        match &event {
            Event::Start(Tag::Heading { level, .. }) => {
                heading_at = Some((events.len(), heading_level(*level)));
                heading_text.clear();
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some((idx, level)) = heading_at.take() {
                    let slug = slugs.make(&heading_text);
                    // pulldown-cmark's HTML renderer emits `id="…"` from the
                    // heading tag's own `id` field — set it now that the
                    // whole heading text (and so its slug) is known.
                    if let Event::Start(Tag::Heading { id, .. }) = &mut events[idx] {
                        *id = Some(slug.clone().into());
                    }
                    toc.push(Heading {
                        level,
                        text: heading_text.clone(),
                        slug,
                    });
                }
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                link_href = Some(dest_url.to_string());
                link_text.clear();
            }
            Event::End(TagEnd::Link) => {
                if let Some(href) = link_href.take() {
                    links.push((link_text.clone(), href));
                }
            }
            Event::Text(t) | Event::Code(t) => {
                if heading_at.is_some() {
                    heading_text.push_str(t);
                }
                if link_href.is_some() {
                    link_text.push_str(t);
                }
            }
            _ => {}
        }
        events.push(event);
    }

    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events.into_iter());
    Rendered { html, toc, links }
}

/// GitHub-style slugs, with GitHub-style disambiguation: the second `## Notes`
/// is `notes-1`, the third `notes-2`.
#[derive(Default)]
struct SlugMaker {
    seen: HashMap<String, u32>,
}

impl SlugMaker {
    fn make(&mut self, text: &str) -> String {
        let base = slugify(text);
        let count = self.seen.entry(base.clone()).or_insert(0);
        let slug = if *count == 0 {
            base.clone()
        } else {
            format!("{base}-{count}")
        };
        *count += 1;
        slug
    }
}

/// Lowercase, non-alphanumerics collapsed to a single `-`, no leading or
/// trailing `-`. An empty result becomes `section` so an `id` is never `""`.
fn slugify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_dash = false;
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.extend(ch.to_lowercase());
        } else {
            pending_dash = true;
        }
    }
    if out.is_empty() {
        out.push_str("section");
    }
    out
}

struct Chapter {
    slug: String,
    title: String,
    body: String,
}

/// Split at every ATX heading of exactly `level` (`## ` for level 2). The
/// preamble before the first such heading is dropped, matching the euglena
/// organelle this was ported from.
fn split_by_heading(src: &str, level: u8) -> Vec<Chapter> {
    let prefix = format!("{} ", "#".repeat(level as usize));
    let mut chapters: Vec<Chapter> = Vec::new();
    let mut current: Option<Chapter> = None;

    for line in src.lines() {
        if let Some(title) = line.strip_prefix(&prefix) {
            if let Some(done) = current.take() {
                chapters.push(done);
            }
            let title = title.trim().to_string();
            current = Some(Chapter {
                slug: slugify(&title),
                title,
                body: String::new(),
            });
        } else if let Some(chapter) = current.as_mut() {
            chapter.body.push_str(line);
            chapter.body.push('\n');
        }
    }
    if let Some(done) = current.take() {
        chapters.push(done);
    }
    chapters
}

// ---------------------------------------------------------------------------
// Shared
// ---------------------------------------------------------------------------

/// A required string field. `""` is a valid document to render or split, so
/// only a missing or non-string value is refused.
fn require_str<'a>(particle: &'a CodeValue, name: &str, class: &str) -> Result<&'a str, String> {
    find_field(particle, name)
        .and_then(read_str)
        .ok_or_else(|| format!("{class} requires a string '{name}'"))
}

/// Build an Array of `items`, each element written by `build`.
fn array_of<T>(out: &mut CodeValue, items: &[T], build: impl Fn(&mut CodeValue, &T)) {
    let mut buf = SlotBuffer::new(items.len());
    for (i, item) in items.iter().enumerate() {
        build(buf.slot_mut(i as i64), item);
    }
    array(out, &mut buf);
    buf.release_all();
}
