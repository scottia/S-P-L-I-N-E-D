# Source Policies and Range Types

This page explains how S:P:L:I:N:E:D evaluates artwork size, how global Resolution Range policy works, how per-source overrides refine that policy, and how AISPLINE remediation remains separate from provider acceptance.

> **Windows target:** v3.0.0 Stable with Config v5.

---

## Core model

SPLINED does not simply choose the largest available image.

Candidate evaluation considers:

- effective artwork Resolution Range;
- actual downloaded image dimensions;
- final output geometry/policy;
- source-specific constraints where enabled;
- local artwork state;
- provider metadata/capabilities;
- candidate eligibility and fallback rules.

The normal target is the configured **Ideal** size, not maximum size at any cost.

---

# Global Resolution Range

SPLINED classifies artwork by the **short side** of the image.

Default anchors:

```text
Minimum = 1200
Ideal   = 1800
Maximum = 2400
Ladder  = 3600
```

Default Range Types:

| Range Type | Short side |
| --- | ---: |
| `BelowMinimum` | `< 1200` |
| `LowerRange` | `1200–1799` |
| `Ideal` | `1800` |
| `UpperRange` | `1801–2400` |
| `Ladder` | `2401–3600` |
| `AboveLadder` | `> 3600` |

The short side is used so a very wide/tall image cannot appear high-resolution merely because one dimension is large.

Examples:

```text
900 x 900    -> BelowMinimum
1200 x 1600  -> LowerRange
1800 x 1800  -> Ideal
2000 x 2400  -> UpperRange
3000 x 4200  -> Ladder
4000 x 5000  -> AboveLadder
```

---

# Range Type is the primary policy language

Range Type is the normal user-facing authority for source acceptance.

Advanced numeric fields may add tighter constraints, but they do not replace the Range Type model.

Example:

```text
Minimum Range Type = LowerRange
```

means candidates below `LowerRange` are not normally accepted by that source unless a fallback policy explicitly allows them.

---

# Source Enabled vs Source Override

These are separate controls.

## Source Enabled

Determines whether SPLINED may query the provider.

```text
Enabled = No
```

Provider is not queried.

```text
Enabled = Yes
```

Provider can participate in discovery.

## Source Override

Determines whether a provider uses custom policy instead of the global policy.

```text
Source Override = No
```

The provider follows normal global SPLINED policy.

```text
Source Override = Yes
```

The provider uses its saved source-specific policy.

Turning Source Override off must **not erase** its saved custom values. Those values should remain available if the override is turned back on later.

---

# Per-source policy controls

Where supported, a source may have custom values for:

- Source Enabled;
- Source Override;
- Minimum Range Type;
- Allow BelowMinimum fallback;
- advanced minimum short side;
- advanced maximum short side;
- minimum width;
- minimum height;
- Primary image only.

Not every source exposes metadata needed for every control. Unsupported controls should be disabled or explained rather than pretending to enforce data the provider does not expose.

Python/TUI should expose the same practical Config v5 source controls as Windows where supported. The goal is to eliminate candidates that policy already knows are irrelevant before they consume unnecessary download, AI-review, ranking, and screen space.

---

# Minimum Range Type

When Source Override is enabled, Minimum Range Type sets the normal category floor for that provider.

Example:

```text
Provider: Discogs
Source Override: Yes
Minimum Range Type: LowerRange
```

Conceptual result:

| Candidate | Result |
| --- | --- |
| BelowMinimum | Reject, or fallback if specifically permitted |
| LowerRange | Accept |
| Ideal | Accept |
| UpperRange | Accept |
| Ladder | Accept |
| AboveLadder | Accept unless another constraint rejects it |

A source-specific minimum affects only that provider.

---

# Adjacent-lower fallback

`allow_below_minimum_fallback` is a **per-source** rule. Despite its historical
field name, it admits only the one Range Type immediately below the configured
minimum.

For example, with `Minimum Range Type = LowerRange`, when disabled:

```text
BelowMinimum -> REJECT
```

When enabled:

```text
BelowMinimum -> FALLBACK
```

Fallback is not equivalent to normal acceptance.

It means the candidate can remain available as a last-resort option when no normally acceptable candidate satisfies the effective policy.

With `Minimum Range Type = Ideal`, `LowerRange` is the only fallback;
`BelowMinimum` remains rejected. This prevents the option from becoming an
unbounded relaxation of the source policy.

---

# Advanced dimension constraints

Advanced constraints refine the effective source policy.

Blank values mean no additional restriction beyond the Range Type/global policy.

Example:

```text
Minimum Range Type = LowerRange
```

implies a normal short-side floor of 1200 using the default scale.

If the user also sets:

```text
Minimum short side = 1500
```

then:

| Short side | Result |
| --- | --- |
| `< 1200` | Reject/fallback according to BelowMinimum policy |
| `1200–1499` | Reject due to explicit advanced minimum |
| `1500–1799` | Accept |
| `1800+` | Accept unless another rule rejects it |

The UI should make it clear when a value is derived from Range Type versus explicitly entered by the user.

---

# Width and height constraints

Explicit minimum width/height fields are additional geometry gates.

For example, a candidate may meet the short-side Range Type but still fail an explicit width requirement.

Use these only when a source needs stricter geometry than the global scale provides.

Avoid adding unnecessary advanced constraints if the normal Range Type model already expresses the intended policy.

---

# Primary image only

`Primary image only` relies on **provider metadata**.

It does not mean SPLINED visually analyzes artwork to decide whether an image is front cover, back cover, booklet, disc, jewel case, or promotional image.

If the provider exposes a reliable primary/front indicator, the option may use it. If the provider does not expose suitable metadata, the control should be disabled with an explanation.

---

# Early filtering and performance

Source policy should be applied as early as the available metadata permits.

Preferred sequence:

```text
provider discovery
-> provider/source policy
-> geometry/metadata rejection where possible
-> candidate download as required
-> normal SPLINED evaluation/ranking
-> optional AISPLINE review of relevant candidates
-> TUI/source summary
```

Avoid an architecture that downloads and AI-reviews every returned variant only to hide most of them later.

This optimization must not change normal SPLINED ranking semantics among candidates that remain eligible.

---

# Final-image evaluation

SPLINED should evaluate the image that would actually be written, not blindly trust provider-reported dimensions.

Important reasons include inaccurate provider metadata, output crop/square transforms, output conversion, and downloaded dimensions that differ from advertised dimensions.

Policy principle:

> Candidate ranking should reflect the final image SPLINED would write.

---

# Local artwork

Existing local artwork is evaluated before unnecessary replacement.

A valid local image may be retained if it already satisfies the effective policy and is otherwise authoritative for the current album.

Local artwork is also a first-class potential AISPLINE input when AI integration is enabled. A user should not need to select and install a remote candidate merely to create an image that AISPLINE can review or enhance.

---

# AISPLINE remediation policy

A:I:S:P:L:I:N:E:D is not a retrieval provider.

Normal SPLINED source policies determine whether provider artwork may participate in candidate selection.

When AISPLINE integration is enabled, an already-obtained local or remote candidate may subsequently be evaluated under AISPLINE remediation policy.

Typical baseline relationship:

```text
BelowMinimum 600–1199
    -> high-value remediation candidate

LowerRange 1200–1799
    -> remediation candidate

Ideal 1800
    -> target met
```

AISPLINE does not change the meaning of SPLINED Range Types. It operates after an image candidate exists.

## User floor vs hard lock

The baseline AISPLINE user floor is:

```text
minimum short side = 600 px
```

This is a practical default for normal use, not an absolute quality law. Low-resolution images may be the only available artwork, and some can remain usable despite their dimensions/compression.

Therefore policy may allow deliberate experimentation below the floor. Such attempts must be explicit, user-controlled, and documented as higher-risk/lower-confidence processing. They must never silently weaken the configured floor.

Config v5 may express the baseline as:

```toml
[aisplined]
enabled = true
endpoint = ""
minimum_short_side = 600
allow_below_minimum_override = false
```

No Config v5 version bump is required simply because AISPLINE-specific policy is added inside the existing `[aisplined]` section.

## AISPLINE source-summary semantics

When AI is enabled, AISPLINE review may occur before the candidate summary is presented.

```text
AI SPLINED
    = yes/no result of completed AISPLINE review

AI ENHANCED
    = optional user enhancement action
    = checkbox + required enhancement delta
    = examples: ☐ +300 EH, ☐ +1200 EH, N/A
```

The review result does not mean the image has already been enhanced.

At most one candidate per album may be selected for enhancement. Selecting one `AI ENHANCED` option disables/grays the other enhancement choices for the same album until the selection changes.

A suggested remote candidate can be handed directly to AISPLINE without first being installed into the album directory.

## Previously Enhanced provenance

A previously successful AISPLINE result known through retained history may be presented as an Enhanced candidate only when the corresponding file still exists and validates.

User-facing provenance markers are:

```text
[LOCAL]
[URL]
[Enhanced]
```

`[Enhanced]` is prior-result provenance. It is not the same thing as the current `AI ENHANCED` checkbox.

## `upscale_below_ideal` runtime override

If the user chooses AISPLINE enhancement while ordinary SPLINED output policy has:

```toml
[output]
upscale_below_ideal = false
```

interactive mode should warn clearly and offer a runtime-only override for that one album/candidate/attempt.

The override must not rewrite Config v5, affect later albums, or silently enable ordinary SPLINED upscaling.

---

# Candidate status language

The GUI/TUI may show candidate results such as:

```text
lower-range - Acceptable
below-minimum - Fallback
```

These labels describe the candidate's effective range/policy result. They should not be confused with Select Media/tree status colors, which represent album/artist execution state rather than image quality.

---

# Troubleshooting source policy

If a candidate you expected to see is missing:

1. confirm the source is Enabled;
2. confirm Source Override state;
3. check Minimum Range Type;
4. check adjacent-lower fallback;
5. inspect advanced short-side/width/height limits;
6. check Primary image only if supported;
7. review the scan log for policy-filtered candidate counts;
8. verify the provider actually returned the candidate;
9. verify the downloaded image dimensions.

If the scan log reports candidates hidden by active source policy, discovery succeeded but effective policy filtered those candidates from the review set.

---

# Relationship to Config v5

For the broader configuration model, see [Config v5 reference](config-v5-reference.md).

Exact serialized Config v5 keys/defaults are listed in the Config v5 reference and examples.

---

# Related documentation

- [Config v5 reference](config-v5-reference.md)
- [Credentials and provider setup](credentials-providers.md)
- [Select Media and status colors](media-filter-status-colors.md)
- [Python Ratatui TUI](ratatui-tui.md)
