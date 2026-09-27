//! Which pages an export takes, and whether a spread goes out as one.
//!
//! Every export asks the same two questions: which pages, and pages or
//! spreads. Both are answered here once, on the resolved document, so a PDF,
//! a print and a picture of the same choice cannot disagree about it.

use tessera_geometry::DocRect;
use tessera_layout::ResolvedPage;
use tessera_layout::resolve::{LinkTarget, ResolvedDocument};

/// `text` read as InDesign reads a page range: page numbers and ranges
/// between commas — `1-3, 6, 9-` — one-based, `9-` running to the last page
/// and `-4` from the first. Empty is every page. The zero-based indices, in
/// document order, each once.
pub fn parse_range(text: &str, count: usize) -> Result<Vec<usize>, String> {
    if text.trim().is_empty() {
        return Ok((0..count).collect());
    }
    let page = |word: &str| -> Result<usize, String> {
        let n: usize = word
            .trim()
            .parse()
            .map_err(|_| format!("\u{201c}{}\u{201d} is not a page number.", word.trim()))?;
        if n == 0 || n > count {
            return Err(if count == 1 {
                format!("There is no page {n}: the document has one page.")
            } else {
                format!("There is no page {n}: the document has {count} pages.")
            });
        }
        Ok(n - 1)
    };
    let mut out = Vec::new();
    for part in text.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (from, to) = match part.split_once(['-', '\u{2013}']) {
            None => {
                let n = page(part)?;
                (n, n)
            }
            Some((a, b)) => {
                let from = if a.trim().is_empty() { 0 } else { page(a)? };
                let to = if b.trim().is_empty() {
                    count.saturating_sub(1)
                } else {
                    page(b)?
                };
                if to < from {
                    return Err(format!(
                        "\u{201c}{part}\u{201d} runs backwards: write the first page first."
                    ));
                }
                (from, to)
            }
        };
        out.extend(from..=to);
    }
    out.sort_unstable();
    out.dedup();
    if out.is_empty() {
        return Err("No pages are named.".to_string());
    }
    Ok(out)
}

/// `resolved` as the pages an export makes: one for each of `groups`, which
/// are the zero-based indices of the document's pages that go onto it. A
/// group of one is that page; a group of several is a spread, its trim the
/// pages side by side and its bleed and slug theirs around the whole.
///
/// Pages in no group are left out, with the bookmarks and links that named
/// them; the rest point at the page they are now on. The objects all stay:
/// the writer puts each where its geometry says, and one off every page is
/// on none.
pub fn assemble(resolved: &ResolvedDocument, groups: &[Vec<usize>]) -> ResolvedDocument {
    let group_of = |old: usize| groups.iter().position(|g| g.contains(&old));
    let union = |rects: &mut dyn Iterator<Item = DocRect>| -> Option<DocRect> {
        rects.reduce(|a, b| {
            let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
            let (x1, y1) = (
                (a.x + a.width).max(b.x + b.width),
                (a.y + a.height).max(b.y + b.height),
            );
            DocRect {
                x: x0,
                y: y0,
                width: x1 - x0,
                height: y1 - y0,
            }
        })
    };
    let pages = groups
        .iter()
        .filter_map(|group| {
            let members: Vec<&ResolvedPage> = group
                .iter()
                .filter_map(|i| resolved.pages.get(*i))
                .collect();
            // A group of one comes out as that page: the union of one
            // rectangle is itself.
            Some(ResolvedPage {
                bounds: union(&mut members.iter().map(|p| p.bounds))?,
                margins: union(&mut members.iter().map(|p| p.margins))?,
                bleed: union(&mut members.iter().map(|p| p.bleed))?,
                slug: union(&mut members.iter().map(|p| p.slug))?,
                columns: members.iter().flat_map(|p| p.columns.clone()).collect(),
            })
        })
        .collect();

    let mut out = ResolvedDocument {
        items: Vec::with_capacity(resolved.items.len()),
        pages,
        bookmarks: Vec::new(),
    };
    for item in &resolved.items {
        let mut item = item.clone();
        item.links.retain_mut(|link| match &mut link.target {
            LinkTarget::Page(index) => match group_of(*index) {
                Some(new) => {
                    *index = new;
                    true
                }
                None => false,
            },
            LinkTarget::Url(_) => true,
        });
        out.items.push(item);
    }
    for bookmark in &resolved.bookmarks {
        if let Some(new) = group_of(bookmark.page) {
            let mut bookmark = bookmark.clone();
            bookmark.page = new;
            out.bookmarks.push(bookmark);
        }
    }
    out
}

/// `chosen` pages as the groups [`assemble`] takes: each alone, or — for
/// spreads — every spread of `spreads` holding any of them, whole, since
/// half a spread is not one.
pub fn groups(chosen: &[usize], spreads: Option<&[Vec<usize>]>) -> Vec<Vec<usize>> {
    match spreads {
        None => chosen.iter().map(|i| vec![*i]).collect(),
        Some(spreads) => spreads
            .iter()
            .filter(|spread| spread.iter().any(|i| chosen.contains(i)))
            .cloned()
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_layout::resolve::Bookmark;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> DocRect {
        DocRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn a_range_is_read_as_indesign_reads_one() {
        assert_eq!(
            parse_range("", 4),
            Ok(vec![0, 1, 2, 3]),
            "empty is every page"
        );
        assert_eq!(parse_range("2", 4), Ok(vec![1]));
        assert_eq!(parse_range("1-2, 4", 4), Ok(vec![0, 1, 3]));
        assert_eq!(parse_range("3-", 5), Ok(vec![2, 3, 4]), "to the last page");
        assert_eq!(parse_range("-2", 5), Ok(vec![0, 1]), "from the first");
        assert_eq!(
            parse_range("4, 1-2, 2", 4),
            Ok(vec![0, 1, 3]),
            "in order, once"
        );
        assert_eq!(
            parse_range("2\u{2013}3", 4),
            Ok(vec![1, 2]),
            "an en dash too"
        );
        assert_eq!(parse_range(" 1 , ,3 ", 4), Ok(vec![0, 2]));
    }

    #[test]
    fn a_range_that_cannot_be_read_says_why() {
        assert_eq!(
            parse_range("7", 4),
            Err("There is no page 7: the document has 4 pages.".into())
        );
        assert_eq!(
            parse_range("2", 1),
            Err("There is no page 2: the document has one page.".into())
        );
        assert_eq!(
            parse_range("0", 4),
            Err("There is no page 0: the document has 4 pages.".into())
        );
        assert_eq!(
            parse_range("two", 4),
            Err("\u{201c}two\u{201d} is not a page number.".into())
        );
        assert_eq!(
            parse_range("3-1", 4),
            Err("\u{201c}3-1\u{201d} runs backwards: write the first page first.".into())
        );
        assert_eq!(parse_range(" , ", 4), Err("No pages are named.".into()));
    }

    fn three_pages() -> ResolvedDocument {
        let page = |x: f64| {
            let trim = rect(x, 0.0, 100.0, 200.0);
            ResolvedPage {
                bounds: trim,
                margins: rect(x + 10.0, 10.0, 80.0, 180.0),
                bleed: rect(x - 5.0, -5.0, 110.0, 210.0),
                slug: rect(x - 20.0, -20.0, 140.0, 240.0),
                columns: Vec::new(),
            }
        };
        ResolvedDocument {
            items: Vec::new(),
            pages: vec![page(0.0), page(200.0), page(300.0)],
            bookmarks: vec![
                Bookmark {
                    title: "One".into(),
                    page: 0,
                    level: 0,
                },
                Bookmark {
                    title: "Three".into(),
                    page: 2,
                    level: 0,
                },
            ],
        }
    }

    #[test]
    fn a_spread_is_one_page_as_wide_as_both() {
        let doc = three_pages();
        let spread = assemble(&doc, &[vec![0], vec![1, 2]]);
        assert_eq!(spread.pages.len(), 2);
        assert_eq!(spread.pages[0], doc.pages[0], "a page alone is itself");
        assert_eq!(spread.pages[1].bounds, rect(200.0, 0.0, 200.0, 200.0));
        assert_eq!(spread.pages[1].bleed, rect(195.0, -5.0, 210.0, 210.0));
        assert_eq!(spread.pages[1].slug, rect(180.0, -20.0, 240.0, 240.0));
        assert_eq!(
            spread.bookmarks.iter().map(|b| b.page).collect::<Vec<_>>(),
            [0, 1],
            "a bookmark follows its page onto the spread"
        );
    }

    #[test]
    fn pages_left_out_take_their_bookmarks_with_them() {
        let cut = assemble(&three_pages(), &[vec![2]]);
        assert_eq!(cut.pages.len(), 1);
        assert_eq!(cut.pages[0].bounds.x, 300.0);
        assert_eq!(cut.bookmarks.len(), 1);
        assert_eq!(
            (cut.bookmarks[0].title.as_str(), cut.bookmarks[0].page),
            ("Three", 0)
        );
    }

    #[test]
    fn a_link_points_at_the_page_it_is_now_on_or_goes() {
        use tessera_layout::resolve::{ResolvedItem, ResolvedKind, ResolvedLink};
        let mut doc = three_pages();
        let link = |target| ResolvedLink {
            rects: vec![rect(0.0, 0.0, 10.0, 10.0)],
            target,
        };
        doc.items.push(ResolvedItem {
            frame: Default::default(),
            links: vec![
                link(LinkTarget::Page(2)),
                link(LinkTarget::Page(0)),
                link(LinkTarget::Url("https://example.org".into())),
            ],
            on: None,
            bounds: rect(0.0, 0.0, 10.0, 10.0),
            transform: tessera_geometry::Transform::IDENTITY,
            spread_area: None,
            blend: tessera_document::blending::Blending::PLAIN,
            shadow: None,
            kind: ResolvedKind::Rectangle {
                fill: tessera_document::paint::Paint::default(),
                stroke: None,
                outline: None,
            },
        });
        let cut = assemble(&doc, &[vec![1, 2]]);
        let targets: Vec<_> = cut.items[0]
            .links
            .iter()
            .map(|l| l.target.clone())
            .collect();
        assert_eq!(
            targets,
            [
                LinkTarget::Page(0),
                LinkTarget::Url("https://example.org".into())
            ],
            "page 3 is on the one spread; page 1 is gone, and so is the link to it"
        );
    }

    #[test]
    fn chosen_pages_bring_their_whole_spread() {
        let spreads = [vec![0], vec![1, 2], vec![3, 4]];
        assert_eq!(groups(&[2], Some(&spreads)), [vec![1, 2]]);
        assert_eq!(groups(&[0, 4], Some(&spreads)), [vec![0], vec![3, 4]]);
        assert_eq!(groups(&[0, 2], None), [vec![0], vec![2]]);
    }
}
