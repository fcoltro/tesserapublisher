# Glass panels, and how the blur is done

## What is behind the glass

A frosted panel over a **ground the interface draws for itself**: three soft
lights in the theme's own accent, plus one warm complement — the arrangement
every glassmorphism reference uses.

**Not the document.** A first attempt had panels blurring the page behind them,
which is a different effect wearing the same name and was wrong twice over. It is
not what the look actually is; and in a tool where colour is *judged*, it means
reading a swatch against a page that moves. The document is opaque, sits beside
the chrome, and is never seen through.

## The blur is generated, not filtered

A backdrop blur normally means: copy the region behind the panel, run a separable
gaussian over the copy, composite. That is three or four passes of WGSL, and
every one of them is something to get wrong on a driver nobody on this project
owns. It is why so many applications either skip glass or ship it broken on one
platform.

None of it is needed here, because **the ground is ours**. It is a handful of
radial falloffs, so a blurrier version of it is *the same function evaluated more
coarsely*.

Two small images come out of one generator: 128 pixels across for the open
ground, and between 6 and 40 for behind the glass. Both are built on the CPU in
well under a millisecond, and stretched back up by the linear filter egui already
samples every texture with. Stretching a dozen pixels across a panel is a very
wide box blur — and because the source is a smooth function rather than a
photograph, there is nothing sharp for it to lose.

- **No shader.** Nothing to write, nothing to debug on somebody else's GPU,
  nothing to keep working across three backends.
- **A stronger blur is cheaper**, because it is a smaller picture. Every
  conventional implementation gets slower as it blurs harder.
- **The blur is one integer**, which a slider holds directly with no calibration
  curve nobody can explain.

The generator is a pure function of size and palette, so it is tested like any
other function: no context, no frame, no adapter. See
`crates/tessera_ui/src/view/ambient.rs` and `.../view/glass.rs`.

### Three details that are not incidental

**The panel frame must be transparent.** egui fills a panel before its closure
runs, so a glass panel asks for no fill — otherwise the frost would be painted
over an opaque rectangle and nothing would show through. A test holds it.

**Distance is measured in window proportions, not pixels.** A light stays round
in a wide window instead of stretching into an ellipse.

**The falloff is a smoothstep.** A linear one leaves a visible edge at the
radius; an inverse-square never quite ends, so the ground never settles to its
base colour.

## Why it is restrained

The reference posters can be as loud as they like. This sits behind a tool
somebody uses for eight hours to judge colour, so the ground is built from the
theme's own accent and one warm complement at low strength. It should read as
depth, not as decoration competing with the page.

The ground is generated from the palette in force, so the light theme gets a
light ground rather than inheriting one designed for the dark.

## Where glass is refused

Even over a still ground, a colour carrying alpha shown over a coloured one is a
different colour. A swatch, a gradient ramp and a fill proxy are all answers to
"what colour is this", so they paint an **opaque well** first
(`glass::opaque_well`) and their colour on top.

That is not a compromise on the look; it is the difference between decoration and
a lie.

## Why it is a preference and not a decision

Three reasons, each sufficient on its own:

1. **Judgement.** A decorative ground behind the chrome is a real trade in a
   precision tool, and reasonable people differ.
2. **Cost.** It is small, but it is not nothing on an old machine.
3. **Legibility.** Some people cannot read text over a patterned background. That
   is not a preference about taste.

Blur strength and panel opacity are separate sliders because they trade against
each other: a heavy blur reads well at low opacity, a light one needs more tint.
Tying them together would remove the adjustment that actually makes text readable
on a given screen.

## What has not been checked

The glass has never been seen. Everything here is reasoned, compiles, and its
logic is tested — but a blur is judged by eye and no eye has been on it.

Worth looking at on a first run: whether the three lights are too strong, too
weak, or in the wrong places; whether the default blur reads as frost or as fog;
and whether an accent-derived ground fights the page rather than sitting behind
it. All three are one slider or one constant away.
