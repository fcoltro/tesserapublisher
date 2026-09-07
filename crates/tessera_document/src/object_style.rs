//! Object styles: a named set of an object's appearance, applied to many.
//!
//! The same idea as a paragraph style, and the same reason for wanting one: a
//! document with forty captions in the same box wants that box described once.
//!
//! **How an override survives a style edit, without an override list.** A style
//! that simply overwrote its objects would throw away every hand adjustment the
//! moment the style changed; a style that recorded which properties each object
//! had overridden would be a second description of a fact the values already
//! tell. So the cascade compares: when the style's old value for a property is
//! still what the object holds, the object was following the style and is
//! updated. When it differs, the object was overriding and is left alone.
//!
//! Nothing has to be recorded for that to work, and nothing can drift out of
//! step with it — which is why "differs from its style" is asked of the values
//! rather than looked up.

use serde::{Deserialize, Deserializer, Serialize};

use crate::blending::Blending;
use crate::nodes::{Stroke, TextWrap};
use crate::paint::Paint;
use crate::shadow::Shadow;

/// What a style says about an object, and what it leaves alone.
///
/// Every field is an `Option` in the sense the text styles use: `None` is "this
/// format does not speak about that property". Where the property is *itself*
/// optional — a stroke, a shadow — the field nests, and the two levels mean
/// different things: `None` is "says nothing", `Some(None)` is "says: none".
/// Collapsing them would make "no stroke" unstateable, which is exactly the
/// thing a style for a plain filled box needs to say.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ObjectFormat {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<Paint>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "stated"
    )]
    pub stroke: Option<Option<Stroke>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blend: Option<Blending>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "stated"
    )]
    pub shadow: Option<Option<Shadow>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wrap: Option<TextWrap>,
}

impl ObjectFormat {
    /// Whether this format says anything at all.
    ///
    /// A style stating nothing is a name attached to no appearance. Worth being
    /// able to ask about, because applying one must be allowed to do nothing
    /// rather than be treated as an error.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// This format over `base`: what `self` states wins, what it leaves `None`
    /// is inherited.
    ///
    /// The same operation the text styles fold a cascade with, and associative
    /// for the same reason — so a style based on another can be folded in either
    /// order without changing the answer.
    pub fn over(&self, base: &ObjectFormat) -> ObjectFormat {
        ObjectFormat {
            fill: self.fill.clone().or_else(|| base.fill.clone()),
            stroke: self.stroke.clone().or_else(|| base.stroke.clone()),
            blend: self.blend.or(base.blend),
            shadow: self.shadow.clone().or_else(|| base.shadow.clone()),
            wrap: self.wrap.or(base.wrap),
        }
    }
}

/// Read a nested option, keeping the difference JSON cannot express.
///
/// `Some(None)` writes as `null`, and serde reads a `null` back into the *outer*
/// `None` — so "says: no stroke" would come back as "says nothing about the
/// stroke", which is precisely the distinction the nesting exists for. Because
/// the field is skipped entirely when it says nothing, a key that is *present*
/// always means "stated"; this wraps whatever arrived, `null` included.
fn stated<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    T: Deserialize<'de>,
    D: Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

/// A named object appearance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObjectStyle {
    pub name: String,
    /// The style this one starts from, if any.
    ///
    /// A chain rather than a copy, so that changing the base changes everything
    /// built on it. Followed to a fixed depth when resolving: somebody who bases
    /// A on B on A has made a mistake and should see a style that stops, not an
    /// application that hangs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub based_on: Option<crate::ids::ObjectStyleId>,
    pub format: ObjectFormat,
}

impl ObjectStyle {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            based_on: None,
            format: ObjectFormat::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tessera_color::Color;

    fn red() -> Paint {
        Paint::Solid(Color::Rgb {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        })
    }

    #[test]
    fn a_format_states_nothing_until_it_is_given_something() {
        assert!(ObjectFormat::default().is_empty());
        assert!(
            !ObjectFormat {
                fill: Some(red()),
                ..Default::default()
            }
            .is_empty()
        );
    }

    #[test]
    fn what_a_format_states_wins_and_what_it_leaves_alone_is_inherited() {
        let base = ObjectFormat {
            fill: Some(red()),
            blend: Some(Blending::PLAIN),
            ..Default::default()
        };
        let over = ObjectFormat {
            blend: Some(Blending {
                opacity: 0.5,
                mode: crate::blending::BlendMode::Multiply,
            }),
            ..Default::default()
        };
        let folded = over.over(&base);
        assert_eq!(folded.fill, Some(red()), "inherited");
        assert_eq!(folded.blend, over.blend, "overridden");
    }

    #[test]
    fn saying_no_stroke_is_different_from_saying_nothing_about_the_stroke() {
        // The reason the field nests. A style for a plain filled box has to be
        // able to say "no stroke", and a single `Option` cannot.
        let silent = ObjectFormat::default();
        let explicit = ObjectFormat {
            stroke: Some(None),
            ..Default::default()
        };
        assert_ne!(silent.stroke, explicit.stroke);
        assert!(silent.is_empty());
        assert!(!explicit.is_empty());

        // And "no stroke" overrides an inherited stroke rather than falling
        // through to it.
        let based = ObjectFormat {
            stroke: Some(Some(Stroke::new(Color::BLACK, 2.0))),
            ..Default::default()
        };
        assert_eq!(explicit.over(&based).stroke, Some(None));
    }

    #[test]
    fn folding_a_cascade_is_associative() {
        // What lets a style based on another be folded in either order.
        let a = ObjectFormat {
            fill: Some(red()),
            ..Default::default()
        };
        let b = ObjectFormat {
            blend: Some(Blending::PLAIN),
            ..Default::default()
        };
        let c = ObjectFormat {
            shadow: Some(Some(Shadow::TYPICAL)),
            ..Default::default()
        };
        assert_eq!(c.over(&b.over(&a)), c.over(&b).over(&a));
    }

    #[test]
    fn saying_no_stroke_survives_a_round_trip_through_json() {
        // `Some(None)` writes as `null`, and serde reads a `null` back into the
        // *outer* `None` unless told otherwise — so without the custom reader
        // "no stroke" came back as "says nothing", which is the one distinction
        // the nesting exists for.
        let explicit = ObjectFormat {
            stroke: Some(None),
            shadow: Some(None),
            ..Default::default()
        };
        let text = serde_json::to_string(&explicit).expect("write");
        let back: ObjectFormat = serde_json::from_str(&text).expect("read");
        assert_eq!(back.stroke, Some(None), "wrote {text}");
        assert_eq!(back.shadow, Some(None));

        // And a silence still reads as a silence.
        let silent: ObjectFormat = serde_json::from_str("{}").expect("read");
        assert_eq!(silent.stroke, None);
        assert_eq!(silent.shadow, None);
    }

    #[test]
    fn a_style_round_trips_through_json_without_writing_what_it_leaves_alone() {
        let style = ObjectStyle {
            name: "Caption box".to_string(),
            based_on: None,
            format: ObjectFormat {
                fill: Some(red()),
                ..Default::default()
            },
        };
        let text = serde_json::to_string(&style).expect("write");
        assert!(
            !text.contains("blend"),
            "a format that says nothing about blending should write nothing: {text}"
        );
        let back: ObjectStyle = serde_json::from_str(&text).expect("read");
        assert_eq!(back, style);
    }
}
