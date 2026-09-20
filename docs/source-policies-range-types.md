# Source Policies and Range Types

This page explains how S:P:L:I:N:E:D evaluates artwork size, how global Resolution Range policy works, and how per-source overrides refine that policy.

> **Windows target:** v3.0.0 Stable with Config v5.
>
> **Repository note:** exact Config v5 key names for the finalized Windows source will be source-verified when that source is integrated. The behavioral rules below are the intended public contract.

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

# BelowMinimum fallback

`Allow BelowMinimum fallback` is a **per-source** rule.

When disabled:

```text
BelowMinimum -> REJECT
```

When enabled:

```text
BelowMinimum -> FALLBACK
```

Fallback is not equivalent to normal acceptance.

It means the candidate can remain available as a last-resort option when no normally acceptable candidate satisfies the effective policy.

This prevents a blanket global rule from discarding potentially useful artwork from providers whose catalog may contain lower-resolution images.

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

The GUI should make it clear when a value is derived from Range Type versus explicitly entered by the user.

---

# Width and height constraints

Explicit minimum width/height fields are additional geometry gates.

For example, a candidate may meet the short-side Range Type but still fail an explicit width requirement.

Use these only when a source needs stricter geometry than the global scale provides.

Avoid adding unnecessary advanced constraints if the normal Range Type model already expresses the intended policy.

---

# Primary image only

`Primary image only` relies on **provider metadata**.

It does not mean SPLINED visually analyzes artwork to decide whether an image is:

- front cover;
- back cover;
- booklet;
- disc;
- jewel case;
- promotional image.

If the provider exposes a reliable primary/front indicator, the option may use it.

If the provider does not expose suitable metadata, the control should be disabled with an explanatory tooltip.

Semantic image recognition is not part of Config v5 source policy.

---

# Global output formats

Output formats are global output policy, not source policy.

Supported static formats currently include:

```text
JPEG
PNG
WebP
```

A provider override should not silently become a per-provider output-format selector.

---

# Final-image evaluation

SPLINED should evaluate the image that would actually be written, not blindly trust provider-reported dimensions.

Important reasons:

- provider metadata may be inaccurate;
- downloaded content may differ from the advertised size;
- square/crop processing may change final geometry;
- output conversion may affect the final file;
- a candidate should not win based on dimensions that disappear after output policy is applied.

Policy principle:

> Candidate ranking should reflect the final image SPLINED would write.

---

# Square/crop behavior

Square/output behavior is separate from provider source selection.

Config v5 can control concepts including:

- square enforcement;
- crop mode;
- square rounding;
- whether upscaling below Ideal is allowed;
- whether final-image evaluation is enabled.

Source policy determines candidate eligibility. Output policy determines how an eligible candidate is prepared for installation.

---

# Source order vs source acceptance

Source order and source policy are related but different.

Source order determines discovery/query behavior.

Policy determines whether a returned candidate is eligible.

A source queried first does not automatically win if its candidate violates effective policy or is farther from the preferred target than another eligible candidate.

---

# Local artwork

Existing local artwork is evaluated before unnecessary replacement.

A valid local image may be retained if it already satisfies the effective policy and is otherwise authoritative for the current album.

Source override rules are primarily about remote/provider candidates and should not be interpreted as permission to replace good local artwork automatically.

---

# Candidate status language

The GUI may show candidate results such as:

```text
lower-range - Acceptable
below-minimum - Fallback
```

These labels describe the candidate's effective range/policy result.

They should not be confused with Media Filter/tree status colors, which represent album/artist execution state rather than image quality.

---

# Example policies

## Conservative provider

```text
Enabled: Yes
Source Override: Yes
Minimum Range Type: Ideal
Allow BelowMinimum fallback: No
```

Only Ideal-or-higher candidates are normally accepted from that source, subject to any upper/advanced constraints.

## Flexible provider

```text
Enabled: Yes
Source Override: Yes
Minimum Range Type: LowerRange
Allow BelowMinimum fallback: Yes
```

LowerRange and above are normally accepted; BelowMinimum may remain available only as fallback.

## Global/default behavior

```text
Enabled: Yes
Source Override: No
```

The provider follows the global SPLINED policy, even if custom override values are still stored for possible future use.

---

# Provider capability differences

Provider APIs differ in:

- available resolution;
- number of image variants;
- primary/front metadata;
- rate limits;
- authentication requirements;
- response quality;
- image URL stability.

SPLINED should expose only controls it can enforce with the metadata actually returned by that source.

---

# Troubleshooting source policy

If a candidate you expected to see is missing:

1. confirm the source is Enabled;
2. confirm Source Override state;
3. check Minimum Range Type;
4. check BelowMinimum fallback;
5. inspect advanced short-side/width/height limits;
6. check Primary image only if supported;
7. review the scan log for policy-filtered candidate counts;
8. verify the provider actually returned the candidate;
9. verify the downloaded image dimensions.

If the scan log reports candidates hidden by active source policy, discovery succeeded but the effective policy filtered those candidates from the review set.

---

# Relationship to Config v5

For the broader Windows configuration model, see [Config v5 reference](config-v5-reference.md).

Exact serialized Config v5 key names for new Windows source-policy fields should be verified against the finalized v3.0.0 source during repository integration.

---

# Related documentation

- [Config v5 reference](config-v5-reference.md)
- [Credentials and provider setup](credentials-providers.md)
- [Media Filter and status colors](media-filter-status-colors.md)
