# Glass panels, and how the blur is done

## The problem

A backdrop blur normally means: render the scene, copy the region behind the
panel, run a separable gaussian over the copy, composite the panel on top. That
is three or four render passes and a pile of WGSL — and every one of those passes
is something to get wrong on a driver nobody on this project owns. It is the
reason so many applications either skip glass or ship it broken on one platform.

## What Tessera does instead

The document is a **vector scene**. So it is rendered a second time, into a
texture a fraction of the size, and stretched back up by the linear filter egui
already samples every texture with.

A bilinear magnification of an *n*-times reduction is a blur of radius *n*. And
because Vello antialiased the small render properly rather than point-sampling
it, the result is *smoother* than box-blurring the full-size image would be —
every source pixel contributed, weighted by coverage.

Three things follow, and they are why this is the right answer rather than a
cheap one:

- **No shader.** Nothing to write, nothing to debug on somebody else's GPU,
  nothing to keep working across three backends.
- **A stronger blur is cheaper.** The backdrop is smaller. At the default divisor
  of six it is one thirty-sixth of the pixels of the main render; at sixteen, one
  two-hundred-and-fifty-sixth. Every conventional implementation gets *slower* as
  it blurs harder. This one gets faster.
- **The radius is one integer.** A preferences slider holds it directly, with no
  calibration curve nobody can explain.

The whole of it is `Scene::append` with a scale transform, a second
`render_to_texture`, and `FilterMode::Linear`. See
`crates/tessera_ui/src/view/vello_host.rs` and `.../view/glass.rs`.

### Two details that are not incidental

**The filter must be `Linear`.** It is not a quality nicety here — it *is* the
blur. `Nearest` gives visible squares.

**The floor is applied to the divisor, not to each axis.** A backdrop has a
minimum size, and clamping width and height independently would keep one axis at
its reduced size while the other hit the floor. The backdrop would then be a
different shape from the canvas, and the scene — scaled by a single factor —
would be letterboxed inside it. A tall thin window is where that shows.

## The layout change this required

Glass over nothing is just tint. For a panel to have anything behind it, the
document has to *be* behind it — so with glass on, the canvas takes the full
central area and the tool strip and the rail float over it as overlays. With
glass off they are ordinary panels beside the canvas and the canvas is narrower.

That is one question with one answer, asked by `glass::floating`. Two places
deciding it independently is how a rail ends up floating while the canvas still
leaves a gap where it used to be.

## Where glass is refused

This is a tool where colour is **judged**. A swatch, a gradient ramp, a fill
proxy and a soft proof are all answers to "what colour is this", and an answer
shown over a moving translucent background is not an answer — it is a colour that
changes as the page scrolls underneath it.

Those surfaces paint an opaque well first (`glass::opaque_well`) and their colour
on top. That is not a compromise on the look; it is the difference between
decoration and a lie. A colour carrying alpha is the case that makes it obvious:
without the well, a 50% red over a blurred page is simply a different red every
frame.

## Why it is a preference and not a decision

Three reasons, each sufficient on its own:

1. **Judgement.** Translucent chrome over a page is a real trade in a precision
   tool, and reasonable people differ.
2. **Cost.** A second render is nothing on a discrete card and not nothing on an
   old laptop.
3. **Legibility.** Some people cannot read text over a moving background. That is
   not a preference about taste.

Blur strength and panel opacity are separate sliders because they trade against
each other: a heavy blur reads well at low opacity, a light one needs more tint.
Tying them together would remove the adjustment that actually makes text
readable on a given screen.

## What has not been checked

The blur has not been seen. Everything here is reasoned and compiles and its
logic is tested, but a blur is judged by eye and no eye has been on it — the GPU
tests in this repository are `#[ignore]`d for want of a reliable adapter in the
build environment.

Specifically worth looking at on a first run: whether the default divisor of six
is too strong or too weak at 100% display scaling; whether the hairline edge
reads against a white page as well as against the pasteboard; and whether the
rail floating over the page is *right*, or whether solid panels are simply better
for laying out.
